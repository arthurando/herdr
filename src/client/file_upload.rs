use super::endpoint::ClientEndpointId;
use crate::protocol::{ClientClipboardImageTarget, ClientInputEvent};
use sha2::{Digest, Sha256};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const MAX_FILE_BYTES: u64 = 1024 * 1024 * 1024;

pub(super) struct UploadResult {
    pub endpoint: ClientEndpointId,
    pub target: ClientClipboardImageTarget,
    pub generation: u64,
    pub result: Result<String, String>,
}

pub(super) struct PendingUpload {
    pub endpoint: ClientEndpointId,
    pub target: ClientClipboardImageTarget,
    pub generation: u64,
    pub host: String,
    pub path: PathBuf,
    pub original: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Choice {
    Wait,
    Upload,
    Paste,
}

pub(super) fn choice(events: &[ClientInputEvent]) -> Choice {
    use crate::protocol::{ClientKeyCode, ClientKeyKind, ClientMouseKind};
    let mut upload = false;
    for event in events {
        match event {
            ClientInputEvent::Key {
                code: ClientKeyCode::Char(ch),
                modifiers,
                kind: ClientKeyKind::Press,
                ..
            } if (*ch == 'u' || *ch == 'U')
                && *modifiers == crossterm::event::KeyModifiers::CONTROL.bits() =>
            {
                upload = true;
            }
            ClientInputEvent::Key {
                kind: ClientKeyKind::Press | ClientKeyKind::Repeat,
                ..
            }
            | ClientInputEvent::TextCommit(_)
            | ClientInputEvent::Paste { .. } => return Choice::Paste,
            ClientInputEvent::Mouse { kind, .. } if !matches!(kind, ClientMouseKind::Moved) => {
                return Choice::Paste
            }
            _ => {}
        }
    }
    if upload {
        Choice::Upload
    } else {
        Choice::Wait
    }
}

pub(super) fn paste_message(
    target: &ClientClipboardImageTarget,
    text: String,
) -> Option<crate::protocol::ClientMessage> {
    use crate::protocol::{ClientMessage, ClientPaneInputEvent};
    match target {
        ClientClipboardImageTarget::Pane(pane_id) => Some(ClientMessage::ClientShellPaneInput {
            pane_id: pane_id.clone(),
            events: vec![ClientPaneInputEvent::Paste(text)],
        }),
        ClientClipboardImageTarget::Popup(terminal_id) => {
            Some(ClientMessage::ClientShellPopupInput {
                terminal_id: terminal_id.clone(),
                events: vec![ClientPaneInputEvent::Paste(text)],
            })
        }
        ClientClipboardImageTarget::DirectTerminal => None,
    }
}

pub(super) fn dropped_file(events: &[ClientInputEvent]) -> Option<PathBuf> {
    let [ClientInputEvent::Paste { text }] = events else {
        return None;
    };
    let text = text.trim_end_matches(['\r', '\n']);
    if text.is_empty() || text.contains(['\r', '\n']) {
        return None;
    }
    let text = text
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| text.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(text);
    let path = PathBuf::from(text);
    (path.is_absolute() && !text.starts_with("\\\\")).then_some(path)
}

fn safe_name(path: &Path) -> String {
    let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("file");
    let clean: String = name
        .chars()
        .take(120)
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if clean.is_empty() || clean == "." || clean == ".." {
        "file".into()
    } else {
        clean
    }
}

pub(super) fn upload(
    path: &Path,
    host: &str,
    mut progress: impl FnMut(u64, u64),
) -> Result<String, String> {
    let mut file = std::fs::File::open(path).map_err(|e| format!("Cannot open local file: {e}"))?;
    let before = file.metadata().map_err(|e| e.to_string())?;
    if !before.is_file() || before.len() > MAX_FILE_BYTES {
        return Err("Only files up to 1 GiB are supported".into());
    }
    let name = safe_name(path);
    let script = format!("umask 077; mkdir -p -- \"$HOME/herdr-uploads\" || exit 1; d=$(mktemp -d \"$HOME/herdr-uploads/drop-XXXXXXXX\") || exit 1; f=\"$d/{name}\"; cat > \"$f\" || exit 1; sha256sum -- \"$f\"");
    let mut command = Command::new("ssh");
    command
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "ConnectTimeout=10",
            "-o",
            "ServerAliveInterval=15",
            "-o",
            "ServerAliveCountMax=2",
            "-T",
            host,
            &script,
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Cannot start SSH: {e}"))?;
    let transfer = (|| -> io::Result<(String, u64)> {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| io::Error::other("SSH input unavailable"))?;
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        let mut total = 0u64;
        let mut last_progress = std::time::Instant::now();
        loop {
            let n = file.read(&mut buffer)?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > MAX_FILE_BYTES {
                return Err(io::Error::other("File grew past 1 GiB"));
            }
            stdin.write_all(&buffer[..n])?;
            hash.update(&buffer[..n]);
            if last_progress.elapsed() >= std::time::Duration::from_secs(2) {
                progress(total, before.len());
                last_progress = std::time::Instant::now();
            }
        }
        drop(stdin);
        Ok((format!("{:x}", hash.finalize()), total))
    })();
    let (hash, bytes) = match transfer {
        Ok(value) => value,
        Err(e) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("Upload failed: {e}"));
        }
    };
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("SSH upload failed; a partial file may remain on SID".into());
    }
    let after = file.metadata().map_err(|e| e.to_string())?;
    if bytes != before.len()
        || after.len() != before.len()
        || after.modified().ok() != before.modified().ok()
    {
        return Err("Local file changed during upload; SID copy was not inserted".into());
    }
    let result = std::str::from_utf8(&output.stdout)
        .map_err(|_| "Invalid SID response")?
        .trim_end();
    let (remote_hash, remote_path) = result
        .split_once("  ")
        .ok_or("Invalid SID checksum response")?;
    if remote_hash != hash
        || !remote_path.starts_with('/')
        || remote_path.chars().any(char::is_control)
    {
        return Err("SID checksum or path verification failed".into());
    }
    Ok(remote_path.to_owned())
}

pub(super) fn shell_quote(path: &str) -> String {
    format!("'{}'", path.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn path_upload_requires_explicit_ctrl_u() {
        use crate::protocol::{ClientKeyCode, ClientKeyKind, ClientKeySource};
        let key = |code, modifiers, kind| ClientInputEvent::Key {
            code,
            modifiers,
            kind,
            repeat_count: 1,
            generated_text: None,
            source: ClientKeySource::Synthesized,
        };
        assert_eq!(
            choice(&[key(
                ClientKeyCode::Char('u'),
                crossterm::event::KeyModifiers::CONTROL.bits(),
                ClientKeyKind::Press
            )]),
            Choice::Upload
        );
        assert_eq!(
            choice(&[key(ClientKeyCode::Enter, 0, ClientKeyKind::Press)]),
            Choice::Paste
        );
        assert_eq!(
            choice(&[key(
                ClientKeyCode::Char('u'),
                crossterm::event::KeyModifiers::CONTROL.bits(),
                ClientKeyKind::Release
            )]),
            Choice::Wait
        );
    }
    #[test]
    fn safe_names_cannot_change_remote_command() {
        assert_eq!(safe_name(Path::new("a';$(x).pdf")), "a____x_.pdf");
    }
    #[test]
    fn drop_accepts_quoted_windows_path_and_rejects_text() {
        let event = |s: &str| vec![ClientInputEvent::Paste { text: s.into() }];
        assert_eq!(
            dropped_file(&event("\"C:\\Users\\a b.pdf\"")),
            Some(PathBuf::from("C:\\Users\\a b.pdf"))
        );
        assert_eq!(dropped_file(&event("ordinary text")), None);
        assert_eq!(dropped_file(&event("\\\\server\\file")), None);
    }
    #[test]
    #[ignore = "requires HERDR_TEST_UPLOAD_FILE and HERDR_TEST_UPLOAD_HOST"]
    fn upload_real_file_over_ssh() {
        let path = std::env::var_os("HERDR_TEST_UPLOAD_FILE").expect("test file path");
        let host = std::env::var("HERDR_TEST_UPLOAD_HOST").expect("test host");
        let remote = upload(Path::new(&path), &host, |_, _| {}).expect("verified upload");
        assert!(remote.starts_with('/'));
        assert!(remote.contains("/herdr-uploads/drop-"));
    }
    #[test]
    fn path_is_shell_quoted() {
        assert_eq!(shell_quote("/home/a b/a'b"), "'/home/a b/a'\\''b'");
    }
}
