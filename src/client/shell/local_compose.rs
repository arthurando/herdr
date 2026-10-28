use super::{text_editor, ClientEndpointId, Palette, TextEditor};
use crate::{input::TerminalKey, protocol::CursorState};
use crossterm::event::{KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::Style,
    widgets::{Block, Borders, Widget},
};

const MAX_DRAFT_BYTES: usize = 128 * 1024;

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
    confirm_multiline: bool,
    attempted: bool,
    confirm_retry: bool,
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
            confirm_multiline: false,
            attempted: false,
            confirm_retry: false,
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
            if self.attempted && !self.confirm_retry {
                self.confirm_retry = true;
                self.status =
                    "Verify remote text first. Ctrl+Enter again confirms another paste.".into();
                return ComposeEffect::None;
            }
            if self.input.contains('\n') && !self.confirm_multiline {
                self.confirm_multiline = true;
                self.status = format!(
                    "Line breaks may submit commands to {:?}/{}. Ctrl+Enter again confirms.",
                    self.endpoint_id, self.pane_id
                );
                return ComposeEffect::None;
            }
            self.confirm_multiline = false;
            self.confirm_retry = false;
            self.attempted = true;
            self.pending = true;
            self.status = "Paste queued locally; draft retained.".into();
            return ComposeEffect::Send(self.input.as_str().to_owned());
        }
        self.confirm_multiline = false;
        self.confirm_retry = false;
        let before = self.input.clone();
        self.input.handle_key(key);
        if self.input.len() > MAX_DRAFT_BYTES {
            self.input = before;
            self.status = "Draft limit is 128 KiB. Edit rejected; draft retained.".into();
        }
        ComposeEffect::None
    }

    pub(super) fn insert_paste(&mut self, text: &str) {
        if !self.pending {
            self.confirm_multiline = false;
            self.confirm_retry = false;
            if text.len() > MAX_DRAFT_BYTES.saturating_sub(self.input.len()) {
                self.status = "Draft limit is 128 KiB. Paste rejected; draft retained.".into();
                return;
            }
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
                self.status =
                    "Paste queued; verify remote text. Draft retained. No Enter appended.".into();
            }
            Err(error) => {
                // Transport queues do not acknowledge remote application delivery.
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
            "Local draft. No Enter appended. Line breaks may submit commands.",
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
            ComposeEffect::None
        );
        assert!(!c.pending);
        assert!(c.status.contains("Line breaks may submit commands"));
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
    fn queue_completion_retains_draft_and_retry_requires_confirmation() {
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
        assert_eq!(c.input.as_str(), "draft");
        assert!(!c.pending);
        assert_eq!(c.route_key(&send), ComposeEffect::None);
        assert!(c.status.contains("Verify remote text"));
        assert_eq!(
            c.route_key(&send.clone().with_kind(KeyEventKind::Repeat)),
            ComposeEffect::None
        );
        assert_eq!(c.route_key(&send), ComposeEffect::Send("draft".into()));
    }
    #[test]
    fn multiline_confirmation_is_revoked_by_edit_and_repeat_cannot_confirm() {
        let mut c = compose();
        c.insert_paste("a\nb");
        let send = TerminalKey::new(KeyCode::Enter, KeyModifiers::CONTROL);
        assert_eq!(c.route_key(&send), ComposeEffect::None);
        assert_eq!(
            c.route_key(&send.clone().with_kind(KeyEventKind::Repeat)),
            ComposeEffect::None
        );
        c.insert_paste("c");
        assert_eq!(c.route_key(&send), ComposeEffect::None);
        assert_eq!(c.route_key(&send), ComposeEffect::Send("a\nbc".into()));
    }

    #[test]
    fn draft_limit_rejects_large_paste_and_unicode_without_corruption() {
        let mut c = compose();
        c.insert_paste(&"x".repeat(MAX_DRAFT_BYTES - 2));
        c.insert_paste("\u{4e2d}");
        assert_eq!(c.input.len(), MAX_DRAFT_BYTES - 2);
        key(&mut c, KeyCode::Char('\u{4e2d}'), KeyModifiers::NONE);
        assert_eq!(c.input.len(), MAX_DRAFT_BYTES - 2);
        c.insert_paste("ab");
        assert_eq!(c.input.len(), MAX_DRAFT_BYTES);
        key(&mut c, KeyCode::Enter, KeyModifiers::NONE);
        assert_eq!(c.input.len(), MAX_DRAFT_BYTES);
        assert!(c.status.contains("128 KiB"));
        key(&mut c, KeyCode::Backspace, KeyModifiers::NONE);
        key(&mut c, KeyCode::Backspace, KeyModifiers::NONE);
        key(&mut c, KeyCode::Backspace, KeyModifiers::NONE);
        c.insert_paste("\u{4e2d}");
        assert_eq!(c.input.len(), MAX_DRAFT_BYTES);
        assert!(c.input.ends_with("\u{4e2d}"));
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
