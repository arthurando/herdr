use super::{text_editor, ClientEndpointId, Palette, TextEditor};
use crate::{input::TerminalKey, protocol::CursorState};
use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Widget},
};

#[derive(Debug, PartialEq, Eq)]
pub(super) enum ComposeEffect {
    None,
    Close,
    Send(String),
}

#[derive(Debug)]
pub(super) struct ClientLocalCompose {
    pub(super) endpoint_id: ClientEndpointId,
    pub(super) boot_id: String,
    pub(super) pane_id: String,
    pub(super) input: TextEditor,
    pub(super) pending: bool,
    pub(super) status: String,
}

impl ClientLocalCompose {
    pub(super) fn new(endpoint_id: ClientEndpointId, boot_id: String, pane_id: String) -> Self {
        Self {
            endpoint_id,
            boot_id,
            pane_id,
            input: TextEditor::multiline(),
            pending: false,
            status: String::new(),
        }
    }

    pub(super) fn route_key(&mut self, key: &TerminalKey) -> ComposeEffect {
        if self.pending || key.kind == KeyEventKind::Release {
            return ComposeEffect::None;
        }
        if key.code == KeyCode::Esc {
            return ComposeEffect::Close;
        }
        if key.code == KeyCode::Enter && key.modifiers == KeyModifiers::CONTROL {
            if key.kind == KeyEventKind::Repeat || self.input.is_empty() {
                return ComposeEffect::None;
            }
            self.pending = true;
            self.status = "Send in progress; draft retained until acknowledgement.".into();
            return ComposeEffect::Send(self.input.as_str().to_owned());
        }
        self.input.handle_key(key);
        ComposeEffect::None
    }

    pub(super) fn insert_paste(&mut self, text: &str) {
        if !self.pending {
            self.input.insert(text);
        }
    }

    pub(super) fn complete(&mut self, result: Result<(), String>) {
        if !self.pending {
            return;
        }
        self.pending = false;
        match result {
            Ok(()) => {
                self.input.clear();
                self.status = "Text sent. Remote Enter was not sent.".into();
            }
            Err(error) => {
                // An acknowledgement can be lost after delivery. Do not promise retry safety.
                let safe_error = TextEditor::from(error.as_str());
                self.status = format!(
                    "Send failed or uncertain; check remote text before retry. {safe_error}"
                );
            }
        }
    }

    pub(super) fn render(&self, buffer: &mut Buffer, palette: &Palette) -> Option<CursorState> {
        let screen = buffer.area;
        let width = screen.width.saturating_sub(2).min(100);
        let height = screen.height.saturating_sub(2).min(24);
        if width < 8 || height < 7 {
            return None;
        }
        let area = Rect::new(
            screen.x + (screen.width - width) / 2,
            screen.y + (screen.height - height) / 2,
            width,
            height,
        );
        let style = Style::default().fg(palette.text).bg(palette.panel_bg);
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                buffer[(x, y)].set_symbol(" ").set_style(style);
            }
        }
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Local compose ")
            .border_style(Style::default().fg(palette.accent))
            .style(style);
        let inner = block.inner(area);
        block.render(area, buffer);
        buffer.set_stringn(
            inner.x,
            inner.y,
            "Enter: new line | Ctrl+Enter: send text | Esc: close",
            usize::from(inner.width),
            style,
        );
        buffer.set_stringn(
            inner.x,
            inner.bottom() - 2,
            "Draft stays local until Send. No remote Enter. Not for passwords.",
            usize::from(inner.width),
            style,
        );
        buffer.set_stringn(
            inner.x,
            inner.bottom() - 1,
            &self.status,
            usize::from(inner.width),
            style,
        );
        let cursor = text_editor::render(
            buffer,
            Rect::new(inner.x, inner.y + 1, inner.width, inner.height - 3),
            &self.input,
            Style::default().fg(palette.text).bg(palette.surface0),
        );
        if self.pending {
            None
        } else {
            cursor
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn compose() -> ClientLocalCompose {
        ClientLocalCompose::new(ClientEndpointId::Local, "boot".into(), "pane".into())
    }
    fn key(c: &mut ClientLocalCompose, code: KeyCode, modifiers: KeyModifiers) -> ComposeEffect {
        c.route_key(&TerminalKey::new(code, modifiers))
    }
    #[test]
    fn unicode_paste_and_multiline_remain_local_until_explicit_send() {
        let mut c = compose();
        c.insert_paste("first\r\n中e\u{301}");
        assert_eq!(
            key(&mut c, KeyCode::Backspace, KeyModifiers::NONE),
            ComposeEffect::None
        );
        assert_eq!(
            key(&mut c, KeyCode::Enter, KeyModifiers::NONE),
            ComposeEffect::None
        );
        c.insert_paste("last\x1b\x00");
        assert_eq!(c.input.as_str(), "first\n中\nlast");
        assert_eq!(
            key(&mut c, KeyCode::Enter, KeyModifiers::CONTROL),
            ComposeEffect::Send("first\n中\nlast".into())
        );
    }
    #[test]
    fn pending_blocks_duplicate_send_close_and_edits_then_failure_keeps_draft() {
        let mut c = compose();
        c.insert_paste("draft");
        assert_eq!(
            key(&mut c, KeyCode::Enter, KeyModifiers::CONTROL),
            ComposeEffect::Send("draft".into())
        );
        for code in [KeyCode::Enter, KeyCode::Esc, KeyCode::Backspace] {
            assert_eq!(
                key(&mut c, code, KeyModifiers::CONTROL),
                ComposeEffect::None
            );
        }
        c.insert_paste("ignored");
        c.complete(Err("timeout".into()));
        assert_eq!(c.input.as_str(), "draft");
        assert!(!c.pending);
        assert!(c.status.contains("check remote text"));
        assert_eq!(
            key(&mut c, KeyCode::Esc, KeyModifiers::NONE),
            ComposeEffect::Close
        );
    }
    #[test]
    fn success_clears_only_after_ack_and_repeat_never_sends() {
        let mut c = compose();
        c.insert_paste("draft");
        let send = TerminalKey::new(KeyCode::Enter, KeyModifiers::CONTROL);
        assert_eq!(
            c.route_key(&send.clone().with_kind(KeyEventKind::Repeat)),
            ComposeEffect::None
        );
        assert_eq!(c.input.as_str(), "draft");
        c.route_key(&send);
        assert_eq!(c.input.as_str(), "draft");
        c.complete(Ok(()));
        assert!(c.input.is_empty());
        assert!(!c.pending);
        c.insert_paste("next");
        c.complete(Ok(()));
        assert_eq!(c.input.as_str(), "next");
    }
    #[test]
    fn multiline_cursor_edits_previous_line_without_flattening() {
        let mut c = compose();
        c.insert_paste("one\ntwo");
        key(&mut c, KeyCode::Up, KeyModifiers::NONE);
        key(&mut c, KeyCode::Home, KeyModifiers::NONE);
        c.insert_paste("X");
        key(&mut c, KeyCode::Down, KeyModifiers::NONE);
        key(&mut c, KeyCode::End, KeyModifiers::NONE);
        c.insert_paste("Y");
        assert_eq!(c.input.as_str(), "Xone\ntwoY");
    }
}
