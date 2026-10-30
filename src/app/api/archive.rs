use super::responses::{encode_error, encode_success};
use crate::api::schema::{ArchiveEntryInfo, ArchiveResumeParams, ResponseResult};
use crate::app::App;

impl App {
    pub(super) fn archive_panes(
        &self,
        ws_idx: usize,
        pane_ids: &[crate::layout::PaneId],
    ) -> std::io::Result<()> {
        if !self.policy.persist_session {
            return Ok(());
        }
        self.archive_panes_to(ws_idx, pane_ids, &crate::persist::archive::directory())
    }

    fn archive_panes_to(
        &self,
        ws_idx: usize,
        pane_ids: &[crate::layout::PaneId],
        directory: &std::path::Path,
    ) -> std::io::Result<()> {
        let ws = self
            .state
            .workspaces
            .get(ws_idx)
            .ok_or_else(|| std::io::Error::other("workspace not found"))?;
        for &pane_id in pane_ids {
            let pane_info = self
                .pane_info(ws_idx, pane_id)
                .ok_or_else(|| std::io::Error::other("pane not found"))?;
            let name = pane_info
                .label
                .filter(|label| !label.is_empty())
                .unwrap_or_else(|| self.workspace_info(ws_idx).label);
            let mut snapshot = crate::persist::capture(
                std::slice::from_ref(ws),
                &self.state.terminals,
                &self.terminal_runtimes,
                Some(0),
                0,
            );
            let workspace = &mut snapshot.workspaces[0];
            let pane = workspace
                .tabs
                .iter_mut()
                .find_map(|tab| tab.panes.remove(&pane_id.raw()))
                .ok_or_else(|| std::io::Error::other("pane missing from captured workspace"))?;
            workspace.id = None;
            workspace.custom_name = Some(name.clone());
            workspace.identity_cwd = pane.cwd.clone();
            workspace.worktree_space = None;
            workspace.public_pane_numbers.clear();
            workspace.next_public_pane_number = 0;
            workspace.public_tab_numbers.clear();
            workspace.next_public_tab_number = 0;
            workspace.active_tab = 0;
            workspace.tabs = vec![crate::persist::TabSnapshot {
                custom_name: None,
                layout: crate::persist::LayoutSnapshot::Pane(pane_id.raw()),
                panes: std::collections::HashMap::from([(pane_id.raw(), pane)]),
                zoomed: false,
                focused: Some(pane_id.raw()),
                root_pane: Some(pane_id.raw()),
            }];
            crate::persist::archive::save(
                directory,
                &crate::persist::archive::Entry { name, snapshot },
            )?;
        }
        Ok(())
    }

    pub(super) fn archive_workspaces(&self, indices: &[usize]) -> std::io::Result<()> {
        if !self.policy.persist_session {
            return Ok(());
        }
        self.archive_workspaces_to(indices, &crate::persist::archive::directory())
    }

    fn archive_workspaces_to(
        &self,
        indices: &[usize],
        directory: &std::path::Path,
    ) -> std::io::Result<()> {
        for &index in indices {
            let Some(workspace) = self.state.workspaces.get(index) else {
                continue;
            };
            let name = self.workspace_info(index).label;
            let mut single = crate::persist::capture(
                std::slice::from_ref(workspace),
                &self.state.terminals,
                &self.terminal_runtimes,
                Some(0),
                0,
            );
            single.workspaces[0].custom_name = Some(name.clone());
            crate::persist::archive::save(
                directory,
                &crate::persist::archive::Entry {
                    name,
                    snapshot: single,
                },
            )?;
        }
        Ok(())
    }

    pub(super) fn handle_archive_list(&self, id: String) -> String {
        match crate::persist::archive::list(&crate::persist::archive::directory()) {
            Ok(entries) => encode_success(
                id,
                ResponseResult::ArchiveList {
                    entries: entries
                        .into_iter()
                        .map(|(id, entry)| ArchiveEntryInfo {
                            id,
                            name: entry.name,
                            resumable: crate::persist::archive::resume_keys(&entry.snapshot)
                                .is_some(),
                        })
                        .collect(),
                },
            ),
            Err(e) => encode_error(id, "archive_read_failed", e.to_string()),
        }
    }

    pub(super) fn handle_archive_resume(
        &mut self,
        id: String,
        params: ArchiveResumeParams,
    ) -> String {
        let entry = match crate::persist::archive::load(
            &crate::persist::archive::directory(),
            &params.id,
        ) {
            Ok(entry) => entry,
            Err(e) => return encode_error(id, "archive_read_failed", e.to_string()),
        };
        self.resume_archive_entry(id, entry)
    }

    fn resume_archive_entry(
        &mut self,
        id: String,
        mut entry: crate::persist::archive::Entry,
    ) -> String {
        let Some(keys) = crate::persist::archive::resume_keys(&entry.snapshot) else {
            return encode_error(
                id,
                "archive_not_resumable",
                "This archive cannot safely resume every process.",
            );
        };
        let active = crate::persist::capture(
            &self.state.workspaces,
            &self.state.terminals,
            &self.terminal_runtimes,
            self.state.active,
            self.state.selected,
        );
        if crate::persist::archive::conflicts(
            &active,
            &keys,
            entry.snapshot.workspaces[0].id.as_deref(),
        ) {
            return encode_error(
                id,
                "archive_already_active",
                "The workspace or an agent session is already active.",
            );
        }
        // Restore otherwise prefers reported commands such as `--continue`, which
        // do not necessarily identify the archived conversation.
        for pane in entry.snapshot.workspaces[0]
            .tabs
            .iter_mut()
            .flat_map(|tab| tab.panes.values_mut())
        {
            pane.agent_resume = None;
        }
        let (workspaces, terminals, runtimes) = crate::persist::restore(
            &entry.snapshot,
            None,
            24,
            80,
            self.state.pane_scrollback_limit_bytes,
            &self.state.default_shell,
            self.state.shell_mode,
            true,
            self.event_tx.clone(),
            self.render_notify.clone(),
            self.render_dirty.clone(),
        );
        if workspaces.len() != 1 || terminals.values().any(|t| t.restore_error.is_some()) {
            return encode_error(
                id,
                "archive_resume_failed",
                "Could not start every saved pane; the archive remains available.",
            );
        }
        let index = self.state.workspaces.len();
        self.state.workspaces.extend(workspaces);
        self.state.terminals.extend(terminals);
        for (terminal_id, runtime) in runtimes {
            self.terminal_runtimes.insert(terminal_id, runtime);
        }
        self.state.switch_workspace(index);
        self.state.mark_session_dirty();
        self.save_session_now();
        self.sync_pending_agent_resume_deadline(std::time::Instant::now());
        self.emit_workspace_open_events(index);
        for tab in 1..self.state.workspaces[index].tabs.len() {
            self.emit_tab_created_events(index, tab);
        }
        match self.workspace_created_result(index) {
            Some(result) => encode_success(id, result),
            None => encode_error(
                id,
                "archive_resume_failed",
                "The workspace did not produce a root pane.",
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::schema::{ErrorResponse, SuccessResponse};
    use crate::workspace::Workspace;

    #[tokio::test]
    async fn closed_named_agent_workspace_can_resume_once() {
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &crate::config::Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            rx,
            crate::api::EventHub::default(),
        );
        app.state.workspaces = vec![Workspace::test_new("CRM cleanup")];
        app.state.workspaces[0].set_custom_name("CRM cleanup".into());
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.ensure_test_terminals();
        let terminal = app.state.terminals.values_mut().next().unwrap();
        terminal.set_persisted_agent_session(crate::agent_resume::PersistedAgentSession {
            source: "herdr:codex".into(),
            agent: "codex".into(),
            session_ref: crate::agent_resume::AgentSessionRef::id("test-session").unwrap(),
        });
        let dir = std::env::temp_dir().join(format!(
            "herdr-archive-app-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        app.archive_workspaces_to(&[0], &dir).unwrap();
        let (archive_id, entry) = crate::persist::archive::list(&dir).unwrap().pop().unwrap();
        app.state.close_selected_workspace();
        let response = app.resume_archive_entry("resume".into(), entry);
        let response: SuccessResponse = serde_json::from_str(&response).unwrap();
        assert!(matches!(
            response.result,
            ResponseResult::WorkspaceCreated { .. }
        ));
        assert_eq!(
            app.state.workspaces[0].custom_name.as_deref(),
            Some("CRM cleanup")
        );
        assert!(app.has_pending_agent_resumes());
        let entry = crate::persist::archive::load(&dir, &archive_id).unwrap();
        let second: ErrorResponse =
            serde_json::from_str(&app.resume_archive_entry("again".into(), entry)).unwrap();
        assert_eq!(second.error.code, "archive_already_active");
        assert!(crate::persist::archive::load(&dir, &archive_id).is_ok());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn closed_pane_resumes_without_closing_its_workspace() {
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &crate::config::Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            rx,
            crate::api::EventHub::default(),
        );
        app.state.workspaces = vec![Workspace::test_new("CRM cleanup")];
        app.state.workspaces[0].set_custom_name("CRM cleanup".into());
        let closed = app.state.workspaces[0].test_split(ratatui::layout::Direction::Horizontal);
        app.state.active = Some(0);
        app.state.selected = 0;
        app.state.ensure_test_terminals();
        let terminal_id = app.state.terminal_id_for_pane(0, closed).unwrap();
        app.state
            .terminals
            .get_mut(&terminal_id)
            .unwrap()
            .set_persisted_agent_session(crate::agent_resume::PersistedAgentSession {
                source: "herdr:codex".into(),
                agent: "codex".into(),
                session_ref: crate::agent_resume::AgentSessionRef::id("pane-session").unwrap(),
            });
        let dir = std::env::temp_dir().join(format!(
            "herdr-archive-pane-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        app.archive_panes_to(0, &[closed], &dir).unwrap();
        let (_, entry) = crate::persist::archive::list(&dir).unwrap().pop().unwrap();
        assert_eq!(entry.name, "CRM cleanup");
        assert_eq!(entry.snapshot.workspaces[0].tabs[0].panes.len(), 1);
        let public_id = app.public_pane_id(0, closed).unwrap();
        app.close_pane(
            "close".into(),
            &crate::api::schema::PaneTarget { pane_id: public_id },
        )
        .unwrap();
        assert_eq!(app.state.workspaces.len(), 1);
        let result: SuccessResponse =
            serde_json::from_str(&app.resume_archive_entry("resume".into(), entry)).unwrap();
        assert!(matches!(
            result.result,
            ResponseResult::WorkspaceCreated { .. }
        ));
        assert_eq!(app.state.workspaces.len(), 2);
        assert_eq!(
            app.state.workspaces[1].custom_name.as_deref(),
            Some("CRM cleanup")
        );
        assert!(app.has_pending_agent_resumes());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn archive_multiple_panes_keeps_each_identity_separate() {
        let (_tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let mut app = App::new(
            &crate::config::Config::default(),
            crate::app::AppPolicy::TEST,
            None,
            rx,
            crate::api::EventHub::default(),
        );
        app.state.workspaces = vec![Workspace::test_new("group")];
        let first = app.state.workspaces[0].tabs[0].root_pane;
        let second = app.state.workspaces[0].test_split(ratatui::layout::Direction::Horizontal);
        app.state.ensure_test_terminals();
        for (pane, name) in [(first, "first"), (second, "second")] {
            let id = app.state.terminal_id_for_pane(0, pane).unwrap();
            app.state
                .terminals
                .get_mut(&id)
                .unwrap()
                .set_persisted_agent_session(crate::agent_resume::PersistedAgentSession {
                    source: "herdr:codex".into(),
                    agent: "codex".into(),
                    session_ref: crate::agent_resume::AgentSessionRef::id(name).unwrap(),
                });
        }
        let dir = std::env::temp_dir().join(format!(
            "herdr-archive-panes-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        app.archive_panes_to(0, &[first, second], &dir).unwrap();
        let entries = crate::persist::archive::list(&dir).unwrap();
        assert_eq!(entries.len(), 2);
        let mut values: Vec<_> = entries
            .into_iter()
            .map(|(_, entry)| {
                let tab = &entry.snapshot.workspaces[0].tabs[0];
                assert_eq!(tab.panes.len(), 1);
                tab.panes
                    .values()
                    .next()
                    .unwrap()
                    .agent_session
                    .as_ref()
                    .unwrap()
                    .value
                    .clone()
            })
            .collect();
        values.sort();
        assert_eq!(values, ["first", "second"]);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
