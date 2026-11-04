use super::*;

/// A client-only preview. Snapshot identity prevents Enter from using a reused workspace ID.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct WorkspaceNavigationTarget {
    pub(super) endpoint_id: ClientEndpointId,
    pub(super) workspace_id: String,
    boot_id: String,
    generation: Option<u64>,
}

/// Display-only continuity while a direct focus request awaits its authoritative snapshot.
pub(super) struct PendingWorkspaceHighlight {
    pub(super) target: WorkspaceNavigationTarget,
    pub(super) request_id: String,
    pub(super) expires_at: Option<std::time::Instant>,
}

/// Input remains bound to the selection that received it, including its boot and generation.
pub(super) struct PendingWorkspaceInput {
    pub(super) target: WorkspaceNavigationTarget,
    pub(super) tab_id: Option<String>,
    pub(super) pane_id: Option<String>,
    pub(super) events: Vec<crate::protocol::ClientPaneInputEvent>,
    pub(super) uncertain: bool,
    pub(super) armed: bool,
}

pub(super) struct PendingDirectFocus {
    pub(super) target: WorkspaceNavigationTarget,
    pub(super) tab_id: Option<String>,
    pub(super) pane_id: Option<String>,
    pub(super) request_id: String,
}

impl WorkspaceNavigationTarget {
    pub(super) fn matches(&self, endpoint_id: &ClientEndpointId, workspace_id: &str) -> bool {
        &self.endpoint_id == endpoint_id && self.workspace_id == workspace_id
    }
}

impl ClientShellState {
    fn pending_input_rows(&self) -> Vec<String> {
        if self.pending_workspace_inputs.is_empty() {
            return vec!["No pending input".into()];
        }
        self.pending_workspace_inputs.iter().map(|entry| format!(
            "{} / {} / {}: {} events - {}",
            self.endpoint_label(&entry.target.endpoint_id), entry.target.workspace_id,
            entry.pane_id.as_deref().or(entry.tab_id.as_deref()).unwrap_or("workspace"),
            entry.events.len(), if entry.uncertain { "delivery uncertain; check remote" } else { "unsent" }
        )).collect()
    }

    pub(super) fn open_pending_workspace_input(&mut self) {
        self.overlay = Some(ClientShellOverlay::PendingInput(ClientPendingInputOverlay {
            rows: self.pending_input_rows(), selected: 0, confirm_delete: false,
        }));
    }

    pub(super) fn route_pending_workspace_input_key(&mut self, key: &crate::input::TerminalKey, outcome: &mut ClientShellInput) -> bool {
        let Some(ClientShellOverlay::PendingInput(view)) = self.overlay.as_ref() else { return false; };
        if key.kind != crossterm::event::KeyEventKind::Press { return true; }
        let selected = view.selected;
        let confirm = view.confirm_delete;
        match key.code {
            KeyCode::Esc => self.overlay = None,
            KeyCode::Up | KeyCode::Down => {
                let delta = if key.code == KeyCode::Up { -1 } else { 1 };
                if let Some(ClientShellOverlay::PendingInput(view)) = self.overlay.as_mut() {
                    view.selected = (view.selected as isize + delta).clamp(0, view.rows.len().saturating_sub(1) as isize) as usize;
                    view.confirm_delete = false;
                }
            }
            KeyCode::Char('d') | KeyCode::Char('D') if selected < self.pending_workspace_inputs.len() => {
                if confirm {
                    self.pending_workspace_inputs.remove(selected);
                    self.open_pending_workspace_input();
                } else if let Some(ClientShellOverlay::PendingInput(view)) = self.overlay.as_mut() {
                    view.confirm_delete = true;
                }
            }
            KeyCode::Enter if selected < self.pending_workspace_inputs.len() => {
                let entry = &self.pending_workspace_inputs[selected];
                if entry.uncertain {
                    self.receive_endpoint_unavailable("Delivery is uncertain. Check the remote pane; do not retry blindly.".into());
                } else {
                    let target = entry.target.clone();
                    let tab_id = entry.tab_id.clone();
                    let pane_id = entry.pane_id.clone();
                    if !self.navigation_target_valid(&target) {
                        self.receive_endpoint_unavailable("Original workspace identity changed. Input remains unsent.".into());
                        outcome.repaint = true;
                        return true;
                    }
                    self.overlay = None;
                    let focus = if let Some(pane) = pane_id.clone() { ClientEndpointFocusTarget::Pane(pane) }
                        else if let Some(tab) = tab_id.clone() { ClientEndpointFocusTarget::Tab(tab) }
                        else { ClientEndpointFocusTarget::Workspace(target.workspace_id.clone()) };
                    if self.focus_or_activate(target.endpoint_id.clone(), focus, outcome) {
                        if let Some(entry) = self.pending_workspace_inputs.iter_mut().find(|entry| entry.target == target && entry.tab_id == tab_id && entry.pane_id == pane_id && !entry.uncertain) {
                            entry.armed = true;
                        }
                    }
                }
            }
            _ => {}
        }
        outcome.repaint = true;
        true
    }

    pub(crate) fn take_ready_workspace_input(&mut self) -> Option<crate::protocol::ClientMessage> {
        let snapshot = self.snapshot.as_deref()?;
        let workspace_id = snapshot.focused_workspace_id.as_deref()?;
        let pane_id = snapshot.focused_pane_id.as_deref()?;
        let surface = self.pane_surface.as_ref()?;
        if surface.boot_id != snapshot.boot_id || surface.projection_revision != snapshot.revision
            || !surface.panes.iter().any(|pane| pane.focused && pane.pane_id == pane_id)
            || !snapshot.panes.iter().any(|pane| pane.pane_id == pane_id && pane.workspace_id == workspace_id)
        {
            return None;
        }
        if self.pending_workspace_highlight.as_ref().is_some_and(|pending|
            !pending.target.matches(&self.active_endpoint_id, workspace_id))
        {
            return None;
        }
        if self.pending_direct_focus.as_ref().is_some_and(|pending|
            !pending.target.matches(&self.active_endpoint_id, workspace_id)
                || pending.target.boot_id != snapshot.boot_id
                || pending.target.generation != self.active_snapshot_generation
                || pending.tab_id.as_deref().is_some_and(|id| snapshot.focused_tab_id.as_deref() != Some(id))
                || pending.pane_id.as_deref().is_some_and(|id| pane_id != id))
        {
            return None;
        }
        self.pending_direct_focus = None;
        let index = self.pending_workspace_inputs.iter().position(|entry|
            !entry.uncertain && entry.armed && !entry.events.is_empty()
                && entry.target.matches(&self.active_endpoint_id, workspace_id)
                && entry.target.boot_id == snapshot.boot_id
                && entry.target.generation == self.active_snapshot_generation
                && entry.tab_id.as_deref().is_none_or(|id| snapshot.focused_tab_id.as_deref() == Some(id))
                && entry.pane_id.as_deref().is_none_or(|id| pane_id == id)
        )?;
        let entry = &mut self.pending_workspace_inputs[index];
        entry.uncertain = true;
        Some(crate::protocol::ClientMessage::ClientShellPaneInput {
            pane_id: pane_id.to_owned(), events: entry.events.clone(),
        })
    }

    pub(super) fn buffer_workspace_input(
        &mut self,
        events: Vec<crate::protocol::ClientPaneInputEvent>,
    ) -> Result<usize, &'static str> {
        let (target, tab_id, pane_id) = self.pending_direct_focus.as_ref().map(|pending| (pending.target.clone(), pending.tab_id.clone(), pending.pane_id.clone()))
            .or_else(|| self.pending_workspace_highlight.as_ref().map(|pending| (pending.target.clone(), None, None)))
            .or_else(|| self.pending_workspace_inputs.iter().find(|entry| entry.armed && !entry.uncertain).map(|entry| (entry.target.clone(), entry.tab_id.clone(), entry.pane_id.clone())))
            .ok_or("No workspace selection is pending")?;
        let existing = self.pending_workspace_inputs.iter().position(|entry| entry.target == target && entry.tab_id == tab_id && entry.pane_id == pane_id);
        if existing.is_none() && self.pending_workspace_inputs.len() >= 8 {
            return Err("Pending input limit reached; no input was queued");
        }
        let index = match existing {
            Some(index) => index,
            None => {
                self.pending_workspace_inputs.push(PendingWorkspaceInput {
                    target, tab_id, pane_id, events: Vec::new(), uncertain: false, armed: true,
                });
                self.pending_workspace_inputs.len() - 1
            }
        };
        let entry = &mut self.pending_workspace_inputs[index];
        if entry.uncertain {
            return Err("Previous delivery is uncertain; no input was queued");
        }
        let size = |events: &[crate::protocol::ClientPaneInputEvent]| -> usize {
            events.iter().map(|event| match event {
                crate::protocol::ClientPaneInputEvent::TextCommit(text)
                | crate::protocol::ClientPaneInputEvent::Paste(text) => text.len(),
                crate::protocol::ClientPaneInputEvent::Key { generated_text: Some(text), .. } => 64 + text.len(),
                _ => 64,
            }).sum()
        };
        if entry.events.len().saturating_add(events.len()) > 2048
            || size(&entry.events).saturating_add(size(&events)) > 131_072
        {
            return Err("Pending input limit reached; no input was queued");
        }
        entry.events.extend(events);
        Ok(entry.events.len())
    }

    pub(crate) fn tick_workspace_highlight(&mut self, now: std::time::Instant) -> bool {
        if self
            .pending_workspace_highlight
            .as_ref()
            .and_then(|pending| pending.expires_at)
            .is_some_and(|expires_at| now >= expires_at)
        {
            if let Some(target) = self.pending_workspace_highlight.as_ref().map(|pending| pending.target.clone()) {
                for entry in &mut self.pending_workspace_inputs {
                    if entry.target == target { entry.armed = false; }
                }
            }
            self.pending_workspace_highlight = None;
            return true;
        }
        false
    }

    pub(super) fn reconcile_pending_workspace_highlight(&mut self) {
        if self
            .pending_workspace_highlight
            .as_ref()
            .is_some_and(|pending| {
                pending.target.endpoint_id != self.active_endpoint_id
                    || !self.navigation_target_valid(&pending.target)
                    || self.snapshot.as_deref().is_some_and(|snapshot| {
                        snapshot.focused_workspace_id.as_deref()
                            == Some(pending.target.workspace_id.as_str())
                    })
            })
        {
            self.pending_workspace_highlight = None;
        }
    }

    pub(super) fn navigation_target(
        &self,
        endpoint_id: &ClientEndpointId,
        workspace_id: &str,
    ) -> Option<WorkspaceNavigationTarget> {
        let endpoint = self
            .endpoints
            .iter()
            .find(|entry| &entry.endpoint_id == endpoint_id)?;
        let snapshot = endpoint.snapshot.as_deref()?;
        Some(WorkspaceNavigationTarget {
            endpoint_id: endpoint_id.clone(),
            workspace_id: workspace_id.to_owned(),
            boot_id: snapshot.boot_id.clone(),
            generation: endpoint.snapshot_generation,
        })
    }

    pub(super) fn focused_navigation_target(&self) -> Option<WorkspaceNavigationTarget> {
        let workspace_id = self.snapshot.as_deref()?.focused_workspace_id.as_deref()?;
        self.navigation_target(&self.active_endpoint_id, workspace_id)
    }

    pub(super) fn navigation_target_valid(&self, target: &WorkspaceNavigationTarget) -> bool {
        self.endpoints.iter().any(|endpoint| {
            endpoint.endpoint_id == target.endpoint_id
                && endpoint.status == ClientEndpointStatus::Online
                && endpoint.snapshot_generation == target.generation
                && endpoint.snapshot.as_deref().is_some_and(|snapshot| {
                    snapshot.boot_id == target.boot_id
                        && snapshot
                            .workspaces
                            .iter()
                            .any(|workspace| workspace.workspace_id == target.workspace_id)
                })
        })
    }

    pub(super) fn workspace_preview_action_blocked(&self) -> bool {
        self.navigate_workspace_id.as_ref().is_some_and(|target| {
            target.endpoint_id != self.active_endpoint_id || !self.navigation_target_valid(target)
        })
    }

    pub(super) fn move_navigate_workspace(&mut self, delta: isize) {
        let mobile = self.mobile_layout_active();
        let surface_available = self.snapshot.is_some() && self.pane_surface.is_some();
        let empty_collapsed_groups = HashSet::new();
        let mut targets = Vec::new();
        for endpoint in &self.endpoints {
            if endpoint.status != ClientEndpointStatus::Online {
                continue;
            }
            let Some(snapshot) = endpoint.snapshot.as_deref() else {
                continue;
            };
            let entries = if self.sidebar_collapsed && !mobile && surface_available {
                snapshot
                    .workspaces
                    .iter()
                    .enumerate()
                    .map(|(index, _)| WorkspaceEntry {
                        index,
                        indented: false,
                        last_child: false,
                    })
                    .collect()
            } else {
                let collapsed_groups = if mobile && surface_available {
                    &empty_collapsed_groups
                } else {
                    self.collapsed_groups_for_endpoint(&endpoint.endpoint_id)
                        .unwrap_or(&empty_collapsed_groups)
                };
                render::workspace_entries(snapshot, collapsed_groups)
            };
            for entry in entries {
                targets.push(WorkspaceNavigationTarget {
                    endpoint_id: endpoint.endpoint_id.clone(),
                    workspace_id: snapshot.workspaces[entry.index].workspace_id.clone(),
                    boot_id: snapshot.boot_id.clone(),
                    generation: endpoint.snapshot_generation,
                });
            }
        }
        if targets.is_empty() {
            return;
        }
        let current = self
            .navigate_workspace_id
            .as_ref()
            .and_then(|selected| targets.iter().position(|target| target == selected));
        let next = match current {
            Some(current) if mobile => {
                (current as isize + delta).clamp(0, targets.len() as isize - 1) as usize
            }
            Some(current) => (current as isize + delta).rem_euclid(targets.len() as isize) as usize,
            None if delta < 0 => targets.len() - 1,
            None => 0,
        };
        let target = targets.swap_remove(next);
        self.collapsed_endpoints.remove(&target.endpoint_id);
        if self.endpoints.len() == 1 && !mobile {
            self.reveal_workspace(&target.workspace_id);
        }
        self.navigate_workspace_id = Some(target);
        self.reveal_mobile_workspace = mobile;
        self.reveal_navigation_workspace =
            !mobile || self.snapshot.is_none() || self.pane_surface.is_none();
    }

    pub(super) fn accept_navigate_workspace(&mut self, outcome: &mut ClientShellInput) {
        let Some(target) = self.navigate_workspace_id.clone() else {
            self.mode = self.copy_or_terminal_mode();
            outcome.repaint = true;
            return;
        };
        if !self.navigation_target_valid(&target) {
            self.receive_endpoint_unavailable(
                "Workspace is no longer available; select a connected workspace".into(),
            );
            outcome.repaint = true;
            return;
        }
        let action_index = outcome.actions.len();
        if self.focus_or_activate(
            target.endpoint_id.clone(),
            ClientEndpointFocusTarget::Workspace(target.workspace_id.clone()),
            outcome,
        ) {
            // Endpoint handoffs already retain their coherent source frame in the runtime.
            // Only retain a highlight if a focus request was actually enqueued.
            if let Some(ClientShellAction::Endpoint { request, .. }) =
                outcome.actions.get(action_index)
            {
                self.pending_workspace_highlight = Some(PendingWorkspaceHighlight {
                    target,
                    request_id: request.id.clone(),
                    expires_at: None,
                });
                self.reconcile_pending_workspace_highlight();
            }
            self.mode = ClientShellMode::Terminal;
            self.navigate_workspace_id = None;
        }
        outcome.repaint = true;
    }
}
