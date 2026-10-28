use super::*;

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
