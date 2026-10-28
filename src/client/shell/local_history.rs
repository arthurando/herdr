//! Client-owned text snapshots. Never infer history by concatenating screen redraws.
use super::*;
use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::widgets::{Block, Borders, Widget};
use std::sync::Arc;

const MAX_BYTES: usize = 1024 * 1024;
const MAX_SNAPSHOTS: usize = 8;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct HistoryKey {
    endpoint: ClientEndpointId,
    boot: String,
    pane: String,
}

#[derive(Debug)]
struct TextSnapshot {
    lines: Vec<String>,
    source: &'static str,
    revision: u64,
    truncated: bool,
}

impl TextSnapshot {
    fn new(text: &str, source: &'static str, revision: u64, truncated: bool) -> Self {
        // Bound retained UTF-8 text; account for line storage as well as payload.
        let mut budget = MAX_BYTES;
        let mut lines = Vec::new();
        let mut clipped = truncated;
        for line in text.lines().rev() {
            let overhead = std::mem::size_of::<String>() + 1;
            if budget <= overhead {
                clipped = true;
                break;
            }
            let clean: String = line
                .chars()
                .map(|c| if c == '\t' { ' ' } else { c })
                .filter(|c| !c.is_control())
                .collect();
            let mut end = clean.len().min(budget - overhead);
            while !clean.is_char_boundary(end) {
                end -= 1;
            }
            clipped |= end < clean.len();
            budget -= end + overhead;
            lines.push(clean[..end].to_owned());
            if end < clean.len() {
                break;
            }
        }
        lines.reverse();
        if lines.is_empty() {
            lines.push(String::new());
        }
        Self {
            lines,
            source,
            revision,
            truncated: clipped,
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct LocalHistoryCache(VecDeque<(HistoryKey, Arc<TextSnapshot>)>);

impl LocalHistoryCache {
    fn insert(&mut self, key: HistoryKey, value: Arc<TextSnapshot>) {
        self.0.retain(|(existing, _)| existing != &key);
        self.0.push_back((key, value));
        while self.0.len() > MAX_SNAPSHOTS {
            self.0.pop_front();
        }
    }
    fn get(&self, key: &HistoryKey) -> Option<Arc<TextSnapshot>> {
        self.0
            .iter()
            .find(|(existing, _)| existing == key)
            .map(|(_, value)| value.clone())
    }
}

#[derive(Debug)]
pub(super) struct ClientLocalHistory {
    key: HistoryKey,
    snapshot: Arc<TextSnapshot>,
    scroll: usize,
    cursor: usize,
    anchor: Option<usize>,
    pending: bool,
    status: String,
}

impl ClientLocalHistory {
    fn new(key: HistoryKey, snapshot: Arc<TextSnapshot>) -> Self {
        Self {
            key,
            cursor: snapshot.lines.len().saturating_sub(1),
            snapshot,
            scroll: 0,
            anchor: None,
            pending: false,
            status: String::new(),
        }
    }
    fn move_cursor(&mut self, delta: isize, height: usize) {
        self.cursor = self
            .cursor
            .saturating_add_signed(delta)
            .min(self.snapshot.lines.len() - 1);
        if self.cursor < self.scroll {
            self.scroll = self.cursor;
        }
        if self.cursor >= self.scroll + height {
            self.scroll = self.cursor.saturating_sub(height - 1);
        }
    }
    fn scroll_by(&mut self, delta: isize, height: usize) {
        let old_scroll = self.scroll;
        self.scroll = self
            .scroll
            .saturating_add_signed(delta)
            .min(self.snapshot.lines.len().saturating_sub(height));
        let moved = self.scroll as isize - old_scroll as isize;
        self.cursor = self
            .cursor
            .saturating_add_signed(moved)
            .min(self.snapshot.lines.len().saturating_sub(1));
    }
    fn selected_text(&self) -> String {
        let anchor = self.anchor.unwrap_or(self.cursor);
        self.snapshot.lines[anchor.min(self.cursor)..=anchor.max(self.cursor)].join("\n")
    }
}

pub(super) fn history_geometry(area: Rect) -> Rect {
    Rect::new(
        area.x.saturating_add(2),
        area.y.saturating_add(2),
        area.width.saturating_sub(4),
        area.height.saturating_sub(4),
    )
}

pub(super) fn render_history(b: &mut Buffer, view: &ClientLocalHistory, palette: &Palette) {
    let outer = Rect::new(
        b.area.x + 1,
        b.area.y + 1,
        b.area.width.saturating_sub(2),
        b.area.height.saturating_sub(2),
    );
    b.set_style(
        outer,
        Style::default().fg(palette.text).bg(palette.panel_bg),
    );
    for y in outer.y..outer.bottom() {
        for x in outer.x..outer.right() {
            b[(x, y)].set_symbol(" ");
        }
    }
    Block::default()
        .borders(Borders::ALL)
        .title(" Local text snapshot - remote output continues ")
        .render(outer, b);
    let body = history_geometry(b.area);
    if body.height < 3 {
        return;
    }
    let height = usize::from(body.height - 2);
    let scroll = view
        .scroll
        .min(view.snapshot.lines.len().saturating_sub(height));
    let anchor = view.anchor.unwrap_or(view.cursor);
    for (row, text) in view
        .snapshot
        .lines
        .iter()
        .skip(scroll)
        .take(height)
        .enumerate()
    {
        let index = scroll + row;
        let selected = (anchor.min(view.cursor)..=anchor.max(view.cursor)).contains(&index);
        let style = if selected {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        b.set_stringn(
            body.x,
            body.y + row as u16,
            text,
            usize::from(body.width),
            style,
        );
    }
    let status = format!(
        "{} | rev {} | {} lines{} {}",
        view.snapshot.source,
        view.snapshot.revision,
        view.snapshot.lines.len(),
        if view.snapshot.truncated {
            " (limited)"
        } else {
            ""
        },
        if view.pending {
            "Loading remote history..."
        } else {
            &view.status
        }
    );
    b.set_stringn(
        body.x,
        body.bottom() - 2,
        status,
        usize::from(body.width),
        Style::default(),
    );
    b.set_stringn(
        body.x,
        body.bottom() - 1,
        "Wheel/PgUp/PgDn scroll | V select lines | Y copy | R fetch terminal rows | Esc live",
        usize::from(body.width),
        Style::default(),
    );
}

impl ClientShellState {
    pub(super) fn open_local_history(&mut self) {
        let Some(pane) = self.focused_pane_id() else {
            return;
        };
        let Some(snapshot) = self.snapshot.as_ref() else {
            return;
        };
        let key = HistoryKey {
            endpoint: self.active_endpoint_id.clone(),
            boot: snapshot.boot_id.clone(),
            pane: pane.clone(),
        };
        let cached = self.local_history_cache.get(&key);
        let text = cached.or_else(|| {
            let surface = self.pane_surface.as_ref()?;
            let pane = surface.panes.iter().find(|p| p.pane_id == pane)?;
            let rect = pane.inner_rect;
            let mut text = String::new();
            for y in rect.y..rect.y.saturating_add(rect.height).min(surface.frame.height) {
                for x in rect.x..rect.x.saturating_add(rect.width).min(surface.frame.width) {
                    let cell = surface
                        .frame
                        .cells
                        .get(usize::from(y) * usize::from(surface.frame.width) + usize::from(x))?;
                    if !cell.skip {
                        text.push_str(&cell.symbol);
                    }
                }
                text.push('\n');
            }
            Some(Arc::new(TextSnapshot::new(
                &text,
                "Received viewport",
                pane.content_revision,
                false,
            )))
        });
        if let Some(text) = text {
            self.local_history_cache.insert(key.clone(), text.clone());
            let mut view = ClientLocalHistory::new(key, text);
            let height = self
                .last_composed_size
                .map_or(20, |(_, h)| usize::from(h.saturating_sub(6)).max(1));
            view.scroll = view.snapshot.lines.len().saturating_sub(height);
            self.overlay = Some(ClientShellOverlay::LocalHistory(view));
            self.mode = ClientShellMode::Terminal;
        }
    }

    pub(super) fn route_local_history_key(
        &mut self,
        key: &crate::input::TerminalKey,
        outcome: &mut ClientShellInput,
    ) -> bool {
        let Some(ClientShellOverlay::LocalHistory(view)) = self.overlay.as_mut() else {
            return false;
        };
        if key.kind == crossterm::event::KeyEventKind::Release
            || (key.kind == crossterm::event::KeyEventKind::Repeat
                && matches!(key.code, KeyCode::Char('r' | 'R' | 'v' | 'V' | 'y' | 'Y')))
        {
            return true;
        }
        outcome.repaint = true;
        let height = self
            .last_composed_size
            .map_or(20, |(_, h)| usize::from(h.saturating_sub(6)).max(1));
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.overlay = None;
            }
            KeyCode::Char('r' | 'R') if !view.pending => {
                let history_key = view.key.clone();
                if self.active_endpoint_id != history_key.endpoint
                    || self.snapshot.as_ref().is_none_or(|snapshot| {
                        snapshot.boot_id != history_key.boot
                            || !snapshot
                                .panes
                                .iter()
                                .any(|pane| pane.pane_id == history_key.pane)
                    })
                {
                    view.status = "Original target changed; cached text retained".into();
                    return true;
                }
                let Some(pane) = self.pane_surface.as_ref().and_then(|surface| {
                    surface
                        .panes
                        .iter()
                        .find(|pane| pane.pane_id == history_key.pane)
                }) else {
                    view.status = "Terminal geometry unavailable; cached text retained".into();
                    return true;
                };
                // These are terminal-owned rows, not an alternate-screen application's history.
                let last_row = pane
                    .scroll
                    .map_or(0, |scroll| scroll.max_offset_from_bottom)
                    .saturating_add(u64::from(pane.inner_rect.height.saturating_sub(1)))
                    .min(u64::from(u32::MAX)) as u32;
                let first_row = last_row.saturating_sub(999);
                let revision = pane.content_revision;
                let end_col = pane.inner_rect.width.saturating_sub(1);
                view.pending = true;
                view.status.clear();
                let sent = self.push_endpoint_method_with_kind(
                    crate::api::schema::Method::PaneSelectionRead(
                        crate::api::schema::PaneSelectionReadParams {
                            pane_id: history_key.pane.clone(),
                            anchor: crate::api::schema::PaneTextPoint {
                                row: first_row,
                                col: 0,
                            },
                            cursor: crate::api::schema::PaneTextPoint {
                                row: last_row,
                                col: end_col,
                            },
                            content_revision: Some(revision),
                        },
                    ),
                    PendingEndpointKind::LocalHistory {
                        key: history_key,
                        revision,
                        truncated: first_row > 0,
                    },
                    outcome,
                );
                if !sent {
                    if let Some(ClientShellOverlay::LocalHistory(view)) = self.overlay.as_mut() {
                        view.pending = false;
                        view.status =
                            "Remote history unavailable; local text remains available".into();
                    }
                }
            }
            KeyCode::Char('y' | 'Y') => outcome.actions.push(ClientShellAction::ClipboardWrite(
                view.selected_text().into_bytes(),
            )),
            KeyCode::Char('v' | 'V') => {
                view.anchor = if view.anchor.is_some() {
                    None
                } else {
                    Some(view.cursor)
                };
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if key.modifiers.contains(KeyModifiers::SHIFT) && view.anchor.is_none() {
                    view.anchor = Some(view.cursor)
                };
                view.move_cursor(-1, height);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if key.modifiers.contains(KeyModifiers::SHIFT) && view.anchor.is_none() {
                    view.anchor = Some(view.cursor)
                };
                view.move_cursor(1, height);
            }
            KeyCode::PageUp => view.scroll_by(-(height as isize), height),
            KeyCode::PageDown => view.scroll_by(height as isize, height),
            KeyCode::Home => view.move_cursor(-(view.snapshot.lines.len() as isize), height),
            KeyCode::End => view.move_cursor(view.snapshot.lines.len() as isize, height),
            _ => {}
        }
        true
    }

    pub(super) fn route_local_history_mouse(
        &mut self,
        mouse: MouseEvent,
        outcome: &mut ClientShellInput,
    ) -> bool {
        let Some(ClientShellOverlay::LocalHistory(view)) = self.overlay.as_mut() else {
            return false;
        };
        let (w, h) = self.last_composed_size.unwrap_or((80, 24));
        let body = history_geometry(Rect::new(0, 0, w, h));
        let height = usize::from(body.height.saturating_sub(2)).max(1);
        match mouse.kind {
            MouseEventKind::ScrollUp => view.scroll_by(-3, height),
            MouseEventKind::ScrollDown => view.scroll_by(3, height),
            MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left)
                if mouse.row >= body.y && mouse.row < body.y + height as u16 =>
            {
                let row = (view.scroll + usize::from(mouse.row - body.y))
                    .min(view.snapshot.lines.len() - 1);
                if matches!(mouse.kind, MouseEventKind::Down(_)) {
                    view.anchor = Some(row);
                }
                view.cursor = row;
            }
            _ => {}
        }
        outcome.repaint = true;
        true
    }

    pub(super) fn complete_local_history(
        &mut self,
        key: HistoryKey,
        revision: u64,
        truncated: bool,
        result: Result<crate::api::schema::ResponseResult, ClientShellEndpointError>,
    ) {
        let text = match result {
            Ok(crate::api::schema::ResponseResult::PaneSelection { pane_id, text })
                if pane_id == key.pane =>
            {
                let text = Arc::new(TextSnapshot::new(
                    &text,
                    "Fetched terminal rows",
                    revision,
                    truncated,
                ));
                self.local_history_cache.insert(key.clone(), text.clone());
                Some(text)
            }
            _ => None,
        };
        if let Some(ClientShellOverlay::LocalHistory(view)) = self.overlay.as_mut() {
            if view.key != key {
                return;
            }
            view.pending = false;
            if let Some(text) = text {
                let height = self
                    .last_composed_size
                    .map_or(20, |(_, h)| usize::from(h.saturating_sub(6)).max(1));
                view.snapshot = text;
                view.cursor = view.snapshot.lines.len() - 1;
                view.scroll = view.snapshot.lines.len().saturating_sub(height);
                view.anchor = None;
                view.status = "Cached locally; R refreshes, Esc returns live".into();
            } else {
                view.status = "Remote fetch failed; cached text retained".into();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn page_and_wheel_scroll_move_the_viewport_on_the_first_event() {
        let key = HistoryKey {
            endpoint: ClientEndpointId::Local,
            boot: "boot".into(),
            pane: "pane".into(),
        };
        let text = (0..100)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join("\n");
        let mut view =
            ClientLocalHistory::new(key, Arc::new(TextSnapshot::new(&text, "test", 1, false)));
        view.scroll = 80;
        view.scroll_by(-3, 20);
        assert_eq!((view.scroll, view.cursor), (77, 96));
        view.scroll_by(-20, 20);
        assert_eq!((view.scroll, view.cursor), (57, 76));
        view.scroll_by(200, 20);
        assert_eq!((view.scroll, view.cursor), (80, 99));
    }
    #[test]
    fn snapshots_are_bounded_and_do_not_fabricate_history() {
        let text = "界".repeat(MAX_BYTES);
        let snapshot = TextSnapshot::new(&text, "test", 1, false);
        assert!(snapshot.truncated);
        assert!(
            snapshot
                .lines
                .iter()
                .map(|s| s.len() + std::mem::size_of::<String>() + 1)
                .sum::<usize>()
                <= MAX_BYTES
        );
        assert_eq!(
            TextSnapshot::new("first\nsecond", "test", 1, false).lines,
            vec!["first", "second"]
        );
        assert_eq!(
            TextSnapshot::new("a\tb", "test", 1, false).lines,
            vec!["a b"]
        );
    }
    #[test]
    fn selection_uses_snapshot_lines() {
        let key = HistoryKey {
            endpoint: ClientEndpointId::Local,
            boot: "boot".into(),
            pane: "pane".into(),
        };
        let mut view = ClientLocalHistory::new(
            key,
            Arc::new(TextSnapshot::new("a\n界\nz", "test", 1, false)),
        );
        view.anchor = Some(0);
        view.cursor = 1;
        assert_eq!(view.selected_text(), "a\n界");
        view.move_cursor(1, 2);
        assert_eq!(view.scroll, 1);
    }
    #[test]
    fn cached_scroll_and_selection_never_request_remote_work() {
        let key = HistoryKey {
            endpoint: ClientEndpointId::Local,
            boot: "boot".into(),
            pane: "pane".into(),
        };
        let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
        state.last_composed_size = Some((80, 24));
        // No endpoint or server exists in this test. A cache must remain usable offline.
        state.overlay = Some(ClientShellOverlay::LocalHistory(ClientLocalHistory::new(
            key,
            Arc::new(TextSnapshot::new(
                &(0..100)
                    .map(|n| format!("line {n}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
                "test",
                1,
                false,
            )),
        )));
        let mut output = ClientShellInput::default();
        state.handle_mouse(
            MouseEvent {
                kind: MouseEventKind::ScrollUp,
                column: 5,
                row: 5,
                modifiers: KeyModifiers::NONE,
            },
            &mut output,
        );
        state.route_overlay_key(
            &crate::input::TerminalKey::new(KeyCode::PageUp, KeyModifiers::NONE),
            &mut output,
        );
        state.route_overlay_key(
            &crate::input::TerminalKey::new(KeyCode::Char('v'), KeyModifiers::NONE),
            &mut output,
        );
        state.route_overlay_key(
            &crate::input::TerminalKey::new(KeyCode::Up, KeyModifiers::NONE),
            &mut output,
        );
        assert!(output.repaint);
        assert!(output.requests.is_empty());
        assert!(output.actions.is_empty());
        state.route_overlay_key(
            &crate::input::TerminalKey::new(KeyCode::Char('y'), KeyModifiers::NONE),
            &mut output,
        );
        assert!(
            matches!(&output.actions[..],[ClientShellAction::ClipboardWrite(bytes)] if String::from_utf8_lossy(bytes).contains("line "))
        );
    }
    #[test]
    fn cache_is_bounded_and_endpoint_boots_do_not_share_text() {
        let mut cache = LocalHistoryCache::default();
        for n in 0..12 {
            cache.insert(
                HistoryKey {
                    endpoint: ClientEndpointId::Local,
                    boot: n.to_string(),
                    pane: "pane".into(),
                },
                Arc::new(TextSnapshot::new("private", "test", 1, false)),
            );
        }
        assert_eq!(cache.0.len(), MAX_SNAPSHOTS);
        assert!(cache
            .get(&HistoryKey {
                endpoint: ClientEndpointId::Local,
                boot: "0".into(),
                pane: "pane".into()
            })
            .is_none());
    }
    #[test]
    fn released_or_repeated_toggle_keys_do_not_change_local_selection() {
        let key = HistoryKey {
            endpoint: ClientEndpointId::Local,
            boot: "boot".into(),
            pane: "pane".into(),
        };
        let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
        state.overlay = Some(ClientShellOverlay::LocalHistory(ClientLocalHistory::new(
            key,
            Arc::new(TextSnapshot::new("text", "test", 1, false)),
        )));
        let press = crate::input::TerminalKey::new(KeyCode::Char('v'), KeyModifiers::NONE);
        let mut output = ClientShellInput::default();
        state.route_local_history_key(&press, &mut output);
        state.route_local_history_key(
            &press
                .clone()
                .with_kind(crossterm::event::KeyEventKind::Release),
            &mut output,
        );
        state.route_local_history_key(
            &press.with_kind(crossterm::event::KeyEventKind::Repeat),
            &mut output,
        );
        let Some(ClientShellOverlay::LocalHistory(view)) = state.overlay.as_ref() else {
            panic!("reader missing")
        };
        assert_eq!(view.anchor, Some(0));
        assert!(output.actions.is_empty() && output.requests.is_empty());
    }
    #[test]
    fn history_refresh_rejects_same_pane_id_from_a_different_server_boot() {
        let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
        state.set_snapshot(Box::new(super::super::tests::snapshot()));
        let key = HistoryKey {
            endpoint: ClientEndpointId::Local,
            boot: "retired-boot".into(),
            pane: "pane_1".into(),
        };
        state.overlay = Some(ClientShellOverlay::LocalHistory(ClientLocalHistory::new(
            key,
            Arc::new(TextSnapshot::new("old text", "test", 1, false)),
        )));
        let mut output = ClientShellInput::default();
        state.route_local_history_key(
            &crate::input::TerminalKey::new(KeyCode::Char('r'), KeyModifiers::NONE),
            &mut output,
        );
        assert!(output.actions.is_empty() && output.requests.is_empty());
        let Some(ClientShellOverlay::LocalHistory(view)) = state.overlay.as_ref() else {
            panic!("reader missing")
        };
        assert!(!view.pending);
        assert!(view.status.contains("Original target changed"));
        assert_eq!(view.snapshot.lines, vec!["old text"]);
    }
}
