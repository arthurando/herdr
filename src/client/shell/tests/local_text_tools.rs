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

#[test]
fn stale_enabled_draft_preferences_cannot_capture_remote_keys_or_paste() {
    let path =
        std::env::temp_dir().join(format!("herdr-direct-recovery-{}.json", std::process::id()));
    let sidecar = path.with_extension("drafts.json");
    let snapshot = snapshot();
    let profile = "0123456789abcdef0123456789abcdef";
    let pane_id = snapshot.focused_pane_id.clone().unwrap();
    std::fs::write(
        &sidecar,
        serde_json::to_vec(&serde_json::json!({"version":1,"panes":[{
            "profile_id":profile,"boot_id":snapshot.boot_id,"pane_id":pane_id,"enabled":true
        }]}))
        .unwrap(),
    )
    .unwrap();
    let config = ClientShellConfig::from_config(&Config::default()).with_preferences_path(path);
    let mut state = ClientShellState::new(config);
    state.set_snapshot(Box::new(snapshot));
    state.set_pane_surface(surface());
    state.active_endpoint_id =
        ClientEndpointId::Ssh(crate::client::endpoint::ProfileId::parse(profile).unwrap());
    let key_result = state.handle_raw_events(vec![RawInputEvent::Key(
        crate::input::TerminalKey::new(KeyCode::Char('x'), KeyModifiers::NONE),
    )]);
    assert!(state.overlay.is_none());
    assert!(
        matches!(&key_result.requests[..], [ClientMessage::ClientShellPaneInput { pane_id: target, events }]
        if target == &pane_id && matches!(&events[..], [ClientPaneInputEvent::Key { code: crate::protocol::ClientKeyCode::Char('x'), kind: crate::protocol::ClientKeyKind::Press, modifiers: 0, .. }]))
    );
    let paste_result = state.handle_raw_events(vec![RawInputEvent::Paste("direct paste".into())]);
    assert!(state.overlay.is_none());
    assert!(paste_result.requests.iter().any(|m| matches!(m, ClientMessage::ClientShellPaneInput { events, .. } if events.iter().any(|event| matches!(event, ClientPaneInputEvent::Paste(text) if text == "direct paste")))));
    let ime_result = state.handle_raw_events(vec![RawInputEvent::Text(
        crate::input::TextCommit::new("IME text"),
    )]);
    assert!(state.overlay.is_none());
    assert!(ime_result.requests.iter().any(|m| matches!(m, ClientMessage::ClientShellPaneInput { events, .. } if events.iter().any(|event| matches!(event, ClientPaneInputEvent::TextCommit(text) if text == "IME text")))));
    std::fs::remove_file(sidecar).unwrap();
}
