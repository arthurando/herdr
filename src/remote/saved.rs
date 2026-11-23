use std::io;
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::{Duration, Instant};

use super::attach::{
    find_cached_remote_herdr, find_installed_remote_herdr, RemoteHerdr, RemoteSsh, SshStdioBridge,
};

pub(crate) struct SavedSshBridge {
    _bridge: SshStdioBridge,
}

pub(crate) struct SavedSshStream {
    pub(crate) stream: crate::ipc::LocalStream,
    pub(crate) bridge: SavedSshBridge,
}

pub(crate) fn connect_saved_ssh(
    profile_id: &str,
    target: &str,
    session: &str,
    cancelled: Arc<AtomicBool>,
) -> io::Result<SavedSshStream> {
    let mut ssh = validated_saved_ssh(profile_id, target, session)?;
    ssh.cancelled = Arc::clone(&cancelled);
    ssh.prepare_shared_transport()?;
    let metadata_cache =
        crate::client::endpoint::SshMetadataCache::new(profile_id, target, session)?;
    let remote_herdr = resolve_saved_remote_herdr(&ssh, &metadata_cache)?;
    let metadata = remote_herdr.machine_metadata();
    if cancelled.load(Ordering::Acquire) {
        return Err(super::process::cancelled());
    }
    let path = saved_bridge_path(profile_id);
    let bridge = SshStdioBridge::start(
        target.to_owned(),
        remote_herdr,
        path.clone(),
        session.to_owned(),
        ssh.options(),
        true,
        Arc::clone(&cancelled),
    )?;
    let stream = crate::ipc::connect_local_stream_cancellable(
        &path,
        &cancelled,
        Instant::now() + Duration::from_secs(5),
    )?;
    if let Some(metadata) = metadata {
        metadata_cache.store(&metadata);
    }
    Ok(SavedSshStream {
        stream,
        bridge: SavedSshBridge { _bridge: bridge },
    })
}

fn resolve_saved_remote_herdr(
    ssh: &RemoteSsh,
    metadata_cache: &crate::client::endpoint::SshMetadataCache,
) -> io::Result<RemoteHerdr> {
    resolve_saved_remote_herdr_with(
        metadata_cache,
        |metadata| find_cached_remote_herdr(ssh, metadata),
        || find_installed_remote_herdr(ssh),
    )
}

fn resolve_saved_remote_herdr_with(
    metadata_cache: &crate::client::endpoint::SshMetadataCache,
    validate: impl FnOnce(
        &crate::client::endpoint::SshMachineMetadata,
    ) -> io::Result<Option<RemoteHerdr>>,
    discover: impl FnOnce() -> io::Result<RemoteHerdr>,
) -> io::Result<RemoteHerdr> {
    Ok(match metadata_cache.load() {
        Some(metadata) => match validate(&metadata)? {
            Some(remote) => remote,
            None => {
                metadata_cache.invalidate();
                discover()?
            }
        },
        None => discover()?,
    })
}

pub(crate) struct SavedSshApiBridge {
    path: PathBuf,
    bridge: SshStdioBridge,
    metadata_cache: crate::client::endpoint::SshMetadataCache,
    ssh: RemoteSsh,
    pub(crate) can_retry: bool,
    pub(crate) used_cached_metadata: bool,
}

impl SavedSshApiBridge {
    pub(crate) fn start(
        profile_id: &str,
        target: &str,
        session: &str,
        use_cached_metadata: bool,
        allow_retry: bool,
    ) -> io::Result<Self> {
        let ssh = validated_saved_ssh(profile_id, target, session)?;
        let mut can_retry = allow_retry;
        let metadata_cache =
            crate::client::endpoint::SshMetadataCache::new(profile_id, target, session)?;
        let cached = use_cached_metadata.then(|| metadata_cache.load()).flatten();
        let used_cached_metadata = cached.is_some();
        let metadata = match cached {
            Some(metadata) => metadata,
            None => {
                let discover = || super::attach::discover_remote_api_metadata(&ssh, session);
                let metadata = match discover() {
                    Err(_) if can_retry && ssh.prepare_shared_transport().unwrap_or(false) => {
                        can_retry = false;
                        discover()?
                    }
                    result => result?,
                };
                metadata_cache.store(&metadata);
                metadata
            }
        };
        let command = super::attach::cached_remote_api_command(&metadata, session);
        let path = crate::platform::remote_bridge_endpoint_path(
            &format!("herdr-api-ssh-{}-{profile_id}.sock", std::process::id()),
            &format!(
                "herdr-api-{}-{}.sock",
                std::process::id(),
                &profile_id[..16]
            ),
        );
        let bridge = SshStdioBridge::start_command(
            target.to_owned(),
            command,
            path.clone(),
            ssh.options(),
            true,
            Arc::new(AtomicBool::new(false)),
        )?;
        Ok(Self {
            path,
            bridge,
            metadata_cache,
            ssh,
            can_retry,
            used_cached_metadata,
        })
    }

    pub(crate) fn socket_path(&self) -> &std::path::Path {
        &self.path
    }

    pub(crate) fn reported_failure(&self) -> Option<io::Error> {
        self.bridge.reported_failure()
    }

    pub(crate) fn invalidate_metadata(&self) {
        self.metadata_cache.invalidate();
    }

    pub(crate) fn recover_transport(&self) -> bool {
        self.can_retry && self.ssh.prepare_shared_transport().unwrap_or(false)
    }

    pub(crate) fn stale_metadata_failure(error: &io::Error) -> bool {
        error
            .to_string()
            .contains(super::attach::STALE_API_METADATA)
    }
}

pub(crate) fn saved_ssh_bootstrap_command(target: &str, session: &str) -> String {
    format!(
        "herdr --remote {} --session {}",
        super::shell_quote(target),
        super::shell_quote(session)
    )
}

pub(crate) fn saved_ssh_failure_needs_attention(error: &io::Error) -> bool {
    if matches!(
        error.kind(),
        io::ErrorKind::InvalidInput
            | io::ErrorKind::InvalidData
            | io::ErrorKind::NotFound
            | io::ErrorKind::PermissionDenied
            | io::ErrorKind::Unsupported
    ) {
        return true;
    }
    let message = error.to_string().to_ascii_lowercase();
    [
        "permission denied",
        "host key verification failed",
        "remote host identification has changed",
        "no matching host key",
        "unsupported remote platform",
        "not ready",
        "install or update",
        "protocol",
        "handshake",
    ]
    .iter()
    .any(|needle| message.contains(needle))
}

fn saved_bridge_path(profile_id: &str) -> PathBuf {
    let pid = std::process::id();
    let readable = format!("herdr-ssh-{pid}-{profile_id}.sock");
    let short = format!("herdr-s-{pid}-{}.sock", &profile_id[..16]);
    crate::platform::remote_bridge_endpoint_path(&readable, &short)
}

fn validated_saved_ssh(profile_id: &str, target: &str, session: &str) -> io::Result<RemoteSsh> {
    validate_profile_path_id(profile_id)?;
    crate::session::validate_name(session)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    Ok(RemoteSsh::new_noninteractive(target.to_owned()))
}

fn validate_profile_path_id(profile_id: &str) -> io::Result<()> {
    if profile_id.len() == 32
        && profile_id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid SSH endpoint profile id",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cached_resolution_handles_hit_and_stale_metadata() {
        const CHILD_CASE: &str = "HERDR_SAVED_CACHE_TEST_CASE";
        if let Ok(case) = std::env::var(CHILD_CASE) {
            let cached = crate::client::endpoint::SshMachineMetadata {
                os: "windows".into(),
                executable: r"C:\cached\herdr.exe".into(),
            };
            let fresh = crate::client::endpoint::SshMachineMetadata {
                os: "windows".into(),
                executable: r"C:\fresh\herdr.exe".into(),
            };
            let cache = crate::client::endpoint::SshMetadataCache::new(
                "0123456789abcdef0123456789abcdef",
                "test-host",
                "default",
            )
            .unwrap();
            cache.store(&cached);
            let mut validations = 0;
            let mut discoveries = 0;
            let remote = resolve_saved_remote_herdr_with(
                &cache,
                |metadata| {
                    validations += 1;
                    assert_eq!(metadata, &cached);
                    Ok((case == "hit")
                        .then(|| RemoteHerdr::from_machine_metadata(metadata).unwrap()))
                },
                || {
                    discoveries += 1;
                    Ok(RemoteHerdr::from_machine_metadata(&fresh).unwrap())
                },
            )
            .unwrap();
            assert_eq!(validations, 1);
            match case.as_str() {
                "hit" => {
                    assert_eq!(discoveries, 0);
                    assert_eq!(remote.machine_metadata(), Some(cached.clone()));
                    assert_eq!(cache.load(), Some(cached));
                }
                "stale" => {
                    assert_eq!(discoveries, 1);
                    assert_eq!(remote.machine_metadata(), Some(fresh));
                    assert_eq!(cache.load(), None);
                }
                _ => panic!("unexpected cache test case"),
            }
            return;
        }

        let root = std::env::temp_dir().join(format!(
            "herdr-saved-cache-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        for case in ["hit", "stale"] {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .arg("cached_resolution_handles_hit_and_stale_metadata")
                .arg("--nocapture")
                .env(CHILD_CASE, case)
                .env("XDG_STATE_HOME", root.join(case))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{case}: {}",
                String::from_utf8_lossy(&output.stdout)
            );
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn bridge_paths_use_profile_identity_not_target_or_session() {
        let first = saved_bridge_path("0123456789abcdef0123456789abcdef");
        let second = saved_bridge_path("fedcba9876543210fedcba9876543210");
        assert_ne!(first, second);
        assert!(!first.to_string_lossy().contains("example.com"));
        assert!(!first.to_string_lossy().contains("default"));
    }

    #[test]
    fn bootstrap_command_preserves_the_explicit_remote_session() {
        assert_eq!(
            saved_ssh_bootstrap_command("build host", "agent work"),
            "herdr --remote 'build host' --session 'agent work'"
        );
    }

    #[test]
    fn prompt_and_compatibility_failures_require_attention() {
        for message in [
            "Permission denied (publickey)",
            "Host key verification failed",
            "matching Herdr is not ready; install or update",
            "handshake rejected",
        ] {
            assert!(saved_ssh_failure_needs_attention(&io::Error::other(
                message
            )));
        }
        assert!(!saved_ssh_failure_needs_attention(&io::Error::new(
            io::ErrorKind::TimedOut,
            "network timed out"
        )));
    }
}
