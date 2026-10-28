use super::local_compose::{ClientLocalCompose, ComposeEffect};
use super::*;

impl ClientShellState {
    pub(super) fn local_text_tool_active(&self) -> bool {
        matches!(
            self.overlay,
            Some(ClientShellOverlay::LocalCompose(_) | ClientShellOverlay::LocalHistory(_))
        )
    }
    pub(super) fn open_local_compose(&mut self) {
        let Some(pane_id) = self.focused_pane_id() else {
            return;
        };
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let boot_id = snapshot.boot_id.clone();
        let draft = self
            .local_compose_drafts
            .iter()
            .position(|draft| {
                draft.endpoint_id == self.active_endpoint_id
                    && draft.boot_id == boot_id
                    && draft.pane_id == pane_id
            })
            .map(|index| self.local_compose_drafts.remove(index));
        if draft.is_none() && self.local_compose_drafts.len() >= 8 {
            self.push_endpoint_notice(
                ClientEndpointNoticeKind::Unsupported,
                "local-compose-limit",
                "Local draft limit",
                "Send or clear an existing local draft before opening another pane draft.",
            );
            return;
        }
        self.overlay = Some(ClientShellOverlay::LocalCompose(draft.unwrap_or_else(
            || ClientLocalCompose::new(self.active_endpoint_id.clone(), boot_id, pane_id),
        )));
        self.mode = ClientShellMode::Terminal;
    }

    pub(super) fn route_local_compose_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) -> bool {
        let Some(ClientShellOverlay::LocalCompose(draft)) = self.overlay.as_mut() else {
            return false;
        };
        let effect = draft.route_key(key);
        outcome.repaint = true;
        match effect {
            ComposeEffect::None => {}
            ComposeEffect::Close => {
                if let Some(ClientShellOverlay::LocalCompose(draft)) = self.overlay.take() {
                    if !draft.input.is_empty() {
                        self.local_compose_drafts.push(draft);
                    }
                }
            }
            ComposeEffect::Send(text) => {
                let endpoint = draft.endpoint_id.clone();
                let boot = draft.boot_id.clone();
                let pane = draft.pane_id.clone();
                let target_matches = self.active_endpoint_id == endpoint
                    && self.snapshot.as_ref().is_some_and(|snapshot| {
                        snapshot.boot_id == boot && snapshot.panes.iter().any(|p| p.pane_id == pane)
                    });
                if !target_matches {
                    if let Some(ClientShellOverlay::LocalCompose(draft)) = self.overlay.as_mut() {
                        draft.complete(Err("Original target changed. Nothing was sent.".into()));
                    }
                    return true;
                }
                if self.endpoint_is_online(&endpoint) {
                    super::push_target_event(
                        ClientInputTarget::Pane(pane),
                        ClientPaneInputEvent::Paste(text),
                        outcome,
                    );
                    if let Some(ClientShellOverlay::LocalCompose(draft)) = self.overlay.as_mut() {
                        draft.complete(Ok(()));
                    }
                } else if let Some(ClientShellOverlay::LocalCompose(draft)) = self.overlay.as_mut()
                {
                    draft.complete(Err("Target unavailable. Nothing was queued.".into()));
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyModifiers};
    #[test]
    fn compose_does_not_send_live_keys_and_fences_changed_target() {
        let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
        state.overlay = Some(ClientShellOverlay::LocalCompose(ClientLocalCompose::new(
            ClientEndpointId::Local,
            "old-boot".into(),
            "old-pane".into(),
        )));
        let mut output = ClientShellInput::default();
        state.route_overlay_key(
            &crate::input::TerminalKey::new(KeyCode::Char('a'), KeyModifiers::NONE),
            &mut output,
        );
        assert!(output.actions.is_empty() && output.requests.is_empty());
        state.route_overlay_key(
            &crate::input::TerminalKey::new(KeyCode::Enter, KeyModifiers::CONTROL),
            &mut output,
        );
        assert!(output.actions.is_empty() && output.requests.is_empty());
        let Some(ClientShellOverlay::LocalCompose(draft)) = state.overlay.as_ref() else {
            panic!("draft lost")
        };
        assert_eq!(draft.input.as_str(), "a");
        assert!(!draft.pending);
    }
    #[test]
    fn closing_compose_retains_local_draft() {
        let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
        let mut draft =
            ClientLocalCompose::new(ClientEndpointId::Local, "boot".into(), "pane".into());
        draft.insert_paste("unsent text");
        state.overlay = Some(ClientShellOverlay::LocalCompose(draft));
        state.route_overlay_key(
            &crate::input::TerminalKey::new(KeyCode::Esc, KeyModifiers::NONE),
            &mut ClientShellInput::default(),
        );
        assert_eq!(state.local_compose_drafts[0].input.as_str(), "unsent text");
        assert!(state.overlay.is_none());
    }
}
