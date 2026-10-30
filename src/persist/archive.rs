//! Durable, per-server snapshots of explicitly closed workspaces.
use super::SessionSnapshot;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Serialize, Deserialize)]
pub(crate) struct Entry {
    pub name: String,
    pub snapshot: SessionSnapshot,
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 80 && id.bytes().all(|c| c.is_ascii_digit() || c == b'-')
}

pub(crate) fn directory() -> PathBuf {
    crate::session::data_dir().join("archive")
}

pub(crate) fn save(dir: &Path, entry: &Entry) -> io::Result<String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    std::fs::create_dir_all(dir)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_nanos();
    let id = format!(
        "{now}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    );
    let path = dir.join(format!("{id}.json"));
    let temporary = dir.join(format!("{id}.tmp"));
    let bytes = serde_json::to_vec(entry).map_err(io::Error::other)?;
    let mut file = crate::platform::create_private_state_file(&temporary)?;
    if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
        drop(file);
        let _ = std::fs::remove_file(temporary);
        return Err(error);
    }
    drop(file);
    crate::platform::replace_file(&temporary, &path)?;
    crate::platform::sync_parent_directory(dir)?;
    Ok(id)
}

/// Only exact, supported native agent session identities are safe to resume.
pub(crate) fn resume_keys(snapshot: &SessionSnapshot) -> Option<Vec<String>> {
    let mut keys = Vec::new();
    if snapshot.workspaces.len() != 1 || snapshot.workspaces[0].worktree_space.is_some() {
        return None;
    }
    for pane in snapshot.workspaces[0]
        .tabs
        .iter()
        .flat_map(|tab| tab.panes.values())
    {
        let saved = pane.agent_session.as_ref()?;
        let session = crate::agent_resume::session_ref_from_snapshot(
            &saved.source,
            &saved.agent,
            saved.kind,
            &saved.value,
        )?;
        let plan = crate::agent_resume::plan(&saved.source, &saved.agent, &session.session_ref)?;
        if keys.contains(&plan.dedupe_key) {
            return None;
        }
        keys.push(plan.dedupe_key);
    }
    if keys.is_empty() {
        None
    } else {
        Some(keys)
    }
}

pub(crate) fn conflicts(
    snapshot: &SessionSnapshot,
    keys: &[String],
    workspace_id: Option<&str>,
) -> bool {
    snapshot.workspaces.iter().any(|ws| {
        (workspace_id.is_some() && ws.id.as_deref() == workspace_id)
            || ws
                .tabs
                .iter()
                .flat_map(|tab| tab.panes.values())
                .any(|pane| {
                    pane.agent_session
                        .as_ref()
                        .and_then(|saved| {
                            let session = crate::agent_resume::session_ref_from_snapshot(
                                &saved.source,
                                &saved.agent,
                                saved.kind,
                                &saved.value,
                            )?;
                            crate::agent_resume::plan(
                                &saved.source,
                                &saved.agent,
                                &session.session_ref,
                            )
                        })
                        .is_some_and(|plan| keys.contains(&plan.dedupe_key))
                })
    })
}

pub(crate) fn load(dir: &Path, id: &str) -> io::Result<Entry> {
    if !valid_id(id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid archive id",
        ));
    }
    let bytes = std::fs::read(dir.join(format!("{id}.json")))?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

pub(crate) fn list(dir: &Path) -> io::Result<Vec<(String, Entry)>> {
    let files = match std::fs::read_dir(dir) {
        Ok(files) => files,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut entries = Vec::new();
    for file in files {
        let file = file?;
        let name = file.file_name();
        let Some(name) = name.to_str().and_then(|s| s.strip_suffix(".json")) else {
            continue;
        };
        if valid_id(name) {
            entries.push((name.to_string(), load(dir, name)?));
        }
    }
    entries.sort_by(|a, b| b.0.cmp(&a.0));
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(agent: Option<serde_json::Value>) -> SessionSnapshot {
        serde_json::from_value(serde_json::json!({
            "version": 3, "active": 0, "selected": 0,
            "workspaces": [{"id":"workspace-1", "custom_name":"CRM cleanup",
                "identity_cwd":".", "tabs":[{"layout":{"Pane": 1},
                    "panes":{"1":{"cwd":".", "agent_session":agent}}, "zoomed":false}]}]
        }))
        .unwrap()
    }

    #[test]
    fn archive_round_trip_preserves_name_and_exact_agent_identity() {
        let dir = std::env::temp_dir().join(format!(
            "herdr-archive-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let agent = serde_json::json!({"source":"herdr:codex","agent":"codex","kind":"id","value":"test-session"});
        let original = Entry {
            name: "CRM cleanup".into(),
            snapshot: snapshot(Some(agent)),
        };
        let id = save(&dir, &original).unwrap();
        let found = list(&dir).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].0, id);
        assert_eq!(found[0].1.name, "CRM cleanup");
        assert_eq!(
            found[0].1.snapshot.workspaces[0].custom_name.as_deref(),
            Some("CRM cleanup")
        );
        assert!(resume_keys(&found[0].1.snapshot).is_some());
        assert!(load(&dir, "../session").is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn plain_shell_is_not_claimed_as_resumable() {
        assert!(resume_keys(&snapshot(None)).is_none());
    }

    #[test]
    fn active_agent_or_workspace_blocks_repeated_resume() {
        let agent = serde_json::json!({"source":"herdr:codex","agent":"codex","kind":"id","value":"test-session"});
        let archived = snapshot(Some(agent));
        let keys = resume_keys(&archived).unwrap();
        assert!(conflicts(&archived, &keys, None));
        assert!(conflicts(&snapshot(None), &[], Some("workspace-1")));
        assert!(!conflicts(&snapshot(None), &keys, Some("different")));
    }
}
