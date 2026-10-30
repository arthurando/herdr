use super::*;
use crate::api::schema::{
    ArchiveEntryInfo, ArchiveResumeParams, EmptyParams, Method, ResponseResult,
};

#[derive(Default)]
pub(super) struct ArchivePanel {
    open: bool,
    loading: bool,
    message: Option<String>,
    entries: Vec<ArchiveEntryInfo>,
    pub(super) scroll: usize,
}

impl ArchivePanel {
    pub(super) fn is_open(&self) -> bool {
        self.open
    }
}

pub(super) fn render(
    buffer: &mut Buffer,
    area: Rect,
    panel: &ArchivePanel,
    palette: &Palette,
    hits: &mut ShellHitMap,
) {
    if area.is_empty() {
        return;
    }
    hits.archive_body = area;
    let style = Style::default().fg(palette.overlay0);
    super::render::put_text(
        buffer,
        area.x,
        area.y,
        area.width,
        " Archived sessions",
        style.add_modifier(Modifier::BOLD),
    );
    let body = Rect::new(
        area.x,
        area.y.saturating_add(1),
        area.width,
        area.height.saturating_sub(1),
    );
    if body.is_empty() {
        return;
    }
    if panel.loading {
        super::render::put_text(buffer, body.x, body.y, body.width, " Loading...", style);
        return;
    }
    if let Some(message) = &panel.message {
        super::render::put_text(buffer, body.x, body.y, body.width, message, style);
        return;
    }
    if panel.entries.is_empty() {
        super::render::put_text(
            buffer,
            body.x,
            body.y,
            body.width,
            " No archived sessions",
            style,
        );
        return;
    }
    hits.archive_max_scroll = panel.entries.len().saturating_sub(body.height as usize);
    for (line, entry) in panel
        .entries
        .iter()
        .skip(panel.scroll.min(hits.archive_max_scroll))
        .take(body.height as usize)
        .enumerate()
    {
        let rect = Rect::new(body.x, body.y + line as u16, body.width, 1);
        let label = if entry.resumable {
            format!(" {}", entry.name)
        } else {
            format!(" {} (unavailable)", entry.name)
        };
        super::render::put_text(
            buffer,
            rect.x,
            rect.y,
            rect.width,
            &label,
            if entry.resumable {
                Style::default()
                    .fg(palette.text)
                    .add_modifier(Modifier::BOLD)
            } else {
                style
            },
        );
        if entry.resumable {
            hits.archive_rows.push((rect, entry.id.clone()));
        }
    }
}

impl ClientShellState {
    pub(super) fn toggle_archive(&mut self, outcome: &mut ClientShellInput) {
        if self.archive.open {
            self.archive.open = false;
        } else {
            self.archive = ArchivePanel {
                open: true,
                ..Default::default()
            };
            let method = Method::WorkspaceArchiveList(EmptyParams {});
            if !self.endpoint_is_online(&self.active_endpoint_id) {
                self.archive.message = Some(" Server offline".into());
            } else if !self.supports_endpoint_method(&method) {
                self.archive.message = Some(" Server update required".into());
            } else {
                self.archive.loading = self.push_endpoint_method_with_kind(
                    method,
                    PendingEndpointKind::ArchiveList,
                    outcome,
                );
                if !self.archive.loading {
                    self.archive.message = Some(" Archive unavailable".into());
                }
            }
        }
        outcome.repaint = true;
    }

    pub(super) fn complete_archive_list(
        &mut self,
        result: Result<ResponseResult, ClientShellEndpointError>,
    ) {
        self.archive.loading = false;
        match result {
            Ok(ResponseResult::ArchiveList { entries }) => self.archive.entries = entries,
            Ok(_) => self.archive.message = Some(" Archive unavailable".into()),
            Err(_) => self.archive.message = Some(" Archive unavailable".into()),
        }
    }

    pub(super) fn resume_archive(&mut self, id: String, outcome: &mut ClientShellInput) {
        if !self
            .archive
            .entries
            .iter()
            .any(|entry| entry.id == id && entry.resumable)
        {
            return;
        }
        self.push_endpoint_method_with_kind(
            Method::WorkspaceArchiveResume(ArchiveResumeParams { id }),
            PendingEndpointKind::ArchiveResume,
            outcome,
        );
    }
}
