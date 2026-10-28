use super::*;

#[test]
fn local_tool_shortcuts_accept_normalized_windows_shift_keys() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 20).unwrap();
    state.mode = ClientShellMode::Prefix;
    state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        KeyCode::Char('u'),
        KeyModifiers::SHIFT,
    ))]);
    assert!(matches!(
        state.overlay,
        Some(ClientShellOverlay::LocalHistory(_))
    ));
    state.overlay = None;
    state.mode = ClientShellMode::Prefix;
    state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        KeyCode::Char('i'),
        KeyModifiers::SHIFT,
    ))]);
    assert!(matches!(
        state.overlay,
        Some(ClientShellOverlay::LocalCompose(_))
    ));
}

#[test]
fn local_reader_keeps_snapshot_while_live_output_advances() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state.compose(106, 20).unwrap();
    state.open_local_history();
    let before = state.compose(106, 20).unwrap();
    let mut newer = surface();
    newer.surface_revision += 1;
    newer.frame.cells[0].symbol = "X".into();
    state.set_pane_surface(newer);
    let reading = state.compose(106, 20).unwrap();
    // Local text does not change when a remote frame arrives underneath it.
    assert_eq!(before.frame.cells, reading.frame.cells);
    state.route_overlay_key(
        &crate::input::TerminalKey::new(KeyCode::Esc, KeyModifiers::NONE),
        &mut ClientShellInput::default(),
    );
    assert_eq!(
        state.pane_surface.as_ref().unwrap().frame.cells[0].symbol,
        "X"
    );
}

#[test]
fn local_compose_owns_text_and_keys_even_with_remote_popup() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface_with_popup());
    state.open_local_compose();
    let output = state.handle_raw_events(vec![
        RawInputEvent::Paste("中文 draft".into()),
        RawInputEvent::Key(crate::input::TerminalKey::new(
            KeyCode::Char('x'),
            KeyModifiers::NONE,
        )),
    ]);
    assert!(output.requests.is_empty() && output.actions.is_empty());
    let Some(ClientShellOverlay::LocalCompose(draft)) = state.overlay.as_ref() else {
        panic!("draft missing")
    };
    assert_eq!(draft.input.as_str(), "中文 draftx");
}

#[test]
fn local_tools_use_stable_advertised_transport_and_retain_sent_draft() {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    let mut pane_surface = surface();
    let last_row = 1200 + u32::from(pane_surface.panes[0].inner_rect.height.saturating_sub(1));
    pane_surface.panes[0].scroll = Some(crate::protocol::PaneSurfaceScrollMetrics {
        offset_from_bottom: 400,
        max_offset_from_bottom: 1200,
        viewport_rows: 18,
    });
    state.set_pane_surface(pane_surface);
    state.set_endpoint_status(&ClientEndpointId::Local, ClientEndpointStatus::Online);
    state.set_endpoint_methods(Some(vec!["pane.selection.read".into()]));
    state.open_local_history();
    let mut output = ClientShellInput::default();
    state.route_local_history_key(
        &crate::input::TerminalKey::new(KeyCode::Char('r'), KeyModifiers::NONE),
        &mut output,
    );
    assert!(
        matches!(&output.actions[..], [ClientShellAction::Endpoint { request, .. }]
        if matches!(&request.method, crate::api::schema::Method::PaneSelectionRead(params)
            if params.content_revision.is_some() && params.cursor.row == last_row && params.anchor.row == last_row - 999))
    );
    let ClientShellAction::Endpoint { request, .. } = &output.actions[0] else {
        unreachable!()
    };
    state.handle_endpoint_result(
        "boot-1",
        &request.id,
        Err(ClientShellEndpointError {
            code: Some("stale_content".into()),
            message: "pane content changed".into(),
        }),
    );
    assert!(matches!(
        state.overlay,
        Some(ClientShellOverlay::LocalHistory(_))
    ));
    assert!(state.pending_requests.is_empty());
    let rendered = state.compose(106, 20).unwrap();
    let text: String = rendered
        .frame
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect();
    assert!(text.contains("cached text retained"));
    state.open_local_compose();
    state.handle_raw_events(vec![RawInputEvent::Paste("literal draft".into())]);
    let send = crate::input::TerminalKey::new(KeyCode::Enter, KeyModifiers::CONTROL);
    let output = state.handle_raw_events(vec![RawInputEvent::Key(send.clone())]);
    assert!(output.actions.is_empty());
    assert!(
        matches!(&output.requests[..], [ClientMessage::ClientShellPaneInput { events, .. }]
        if matches!(&events[..], [ClientPaneInputEvent::Paste(text)] if text == "literal draft"))
    );
    let Some(ClientShellOverlay::LocalCompose(draft)) = state.overlay.as_ref() else {
        panic!("draft missing")
    };
    assert_eq!(draft.input.as_str(), "literal draft");
    assert!(draft.status.contains("Paste queued"));
    let output = state.handle_raw_events(vec![RawInputEvent::Key(send)]);
    assert!(output.requests.is_empty() && output.actions.is_empty());
}

fn enrolled_remote_state() -> ClientShellState {
    let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
    state.set_snapshot(Box::new(snapshot()));
    state.set_pane_surface(surface());
    state.active_endpoint_id = ClientEndpointId::Ssh(
        crate::client::endpoint::ProfileId::parse("0123456789abcdef0123456789abcdef").unwrap(),
    );
    state.draft_defaults = serde_json::from_value(serde_json::json!({"version":1,"panes":[{
        "profile_id":"0123456789abcdef0123456789abcdef", "boot_id":state.snapshot.as_ref().unwrap().boot_id,
        "pane_id":state.focused_pane_id().unwrap(),"enabled":true
    }]})).unwrap();
    state
}

#[test]
fn enrolled_remote_printable_key_and_paste_stay_local_without_request() {
    let mut state = enrolled_remote_state();
    let output = state.handle_raw_events(vec![
        RawInputEvent::Key(crate::input::TerminalKey::new(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
        )),
        RawInputEvent::Paste("bc".into()),
    ]);
    assert!(output.requests.is_empty() && output.actions.is_empty());
    let Some(ClientShellOverlay::LocalCompose(draft)) = state.overlay.as_ref() else {
        panic!("default draft missing")
    };
    assert_eq!(draft.input.as_str(), "abc");
}

#[test]
fn default_draft_fences_boot_and_direct_toggle_preserves_draft() {
    let mut state = enrolled_remote_state();
    state
        .snapshot
        .as_mut()
        .unwrap()
        .boot_id
        .push_str("-restarted");
    assert!(!state.default_draft_enabled());
    let mut state = enrolled_remote_state();
    state.handle_raw_events(vec![RawInputEvent::Paste("retained".into())]);
    state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        KeyCode::Char('d'),
        KeyModifiers::CONTROL | KeyModifiers::SHIFT,
    ))]);
    assert!(!state.default_draft_enabled());
    assert!(state.overlay.is_none());
    assert_eq!(state.local_compose_drafts[0].input.as_str(), "retained");
    state.set_default_draft(true);
    assert!(state.default_draft_enabled());
}

#[test]
fn default_draft_does_not_capture_control_shortcuts_or_local_panes() {
    let mut state = enrolled_remote_state();
    state.handle_raw_events(vec![RawInputEvent::Key(crate::input::TerminalKey::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    ))]);
    assert!(state.overlay.is_none());
    state.active_endpoint_id = ClientEndpointId::Local;
    assert!(!state.default_draft_enabled());
}

#[test]
fn default_draft_consumes_release_and_preserves_generated_text_once() {
    let mut state = enrolled_remote_state();
    let mut key = crate::input::TerminalKey::new(KeyCode::Char('a'), KeyModifiers::NONE);
    key.generated_text = Some("x".into());
    let release = key
        .clone()
        .with_kind(crossterm::event::KeyEventKind::Release);
    let output =
        state.handle_raw_events(vec![RawInputEvent::Key(key), RawInputEvent::Key(release)]);
    assert!(output.requests.is_empty() && output.actions.is_empty());
    let Some(ClientShellOverlay::LocalCompose(draft)) = state.overlay.as_ref() else {
        panic!("draft missing")
    };
    assert_eq!(draft.input.as_str(), "x");
}

#[test]
fn default_draft_repeat_stays_local_and_chrome_shows_mode() {
    let mut state = enrolled_remote_state();
    let key = crate::input::TerminalKey::new(KeyCode::Char('a'), KeyModifiers::NONE);
    let output = state.handle_raw_events(vec![
        RawInputEvent::Key(key.clone()),
        RawInputEvent::Key(key.with_kind(crossterm::event::KeyEventKind::Repeat)),
    ]);
    assert!(output.requests.is_empty() && output.actions.is_empty());
    let Some(ClientShellOverlay::LocalCompose(draft)) = state.overlay.as_ref() else {
        panic!("draft missing")
    };
    assert_eq!(draft.input.as_str(), "aa");
    state.overlay = None;
    let frame = state.compose(106, 30).unwrap();
    let text: String = frame
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect();
    assert!(text.contains(" draft "));
    state.set_default_draft(false);
    let frame = state.compose(106, 30).unwrap();
    let text: String = frame
        .cells
        .iter()
        .map(|cell| cell.symbol.as_str())
        .collect();
    assert!(text.contains(" direct "));
    state.toggle_global_menu();
    assert!(
        state.compose(24, 12).is_some(),
        "narrow menu remains bounded"
    );
}
