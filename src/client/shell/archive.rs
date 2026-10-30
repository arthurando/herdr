use super::*;
use crate::api::schema::{
    ArchiveEntryInfo, ArchiveResumeParams, EmptyParams, Method, ResponseResult,
};

#[derive(Default)]
pub(super) struct ArchivePanel {
    open: bool,
    loading: bool,
    endpoint_id: Option<ClientEndpointId>,
    boot_id: Option<String>,
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
                endpoint_id: Some(self.active_endpoint_id.clone()),
                boot_id: self
                    .snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.boot_id.clone()),
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
        if self.archive.endpoint_id.as_ref() != Some(&self.active_endpoint_id)
            || self.archive.boot_id.as_deref()
                != self
                    .snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.boot_id.as_str())
            || !self
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn archive_renders_names_and_only_resumable_rows_are_clickable() {
        let config = ClientShellConfig::from_config(&Config::default());
        let panel = ArchivePanel {
            open: true,
            entries: vec![
                ArchiveEntryInfo {
                    id: "one".into(),
                    name: "CRM cleanup".into(),
                    resumable: true,
                },
                ArchiveEntryInfo {
                    id: "two".into(),
                    name: "Old session".into(),
                    resumable: false,
                },
            ],
            ..Default::default()
        };
        let rect = Rect::new(0, 0, 30, 4);
        let mut buffer = Buffer::empty(rect);
        let mut hits = ShellHitMap::default();
        render(&mut buffer, rect, &panel, &config.palette, &mut hits);
        assert!(
            buffer
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
                .contains("CRM cleanup")
        );
        assert_eq!(hits.archive_rows.len(), 1);
        assert_eq!(hits.archive_rows[0].1, "one");
    }

    #[test]
    fn archive_checks_capability_and_requests_resume_by_id() {
        let mut state = ClientShellState::new(ClientShellConfig::from_config(&Config::default()));
        state.set_snapshot(Box::new(super::super::tests::snapshot()));
        state.set_endpoint_methods(Some(vec!["workspace.list".into()]));
        let mut outcome = ClientShellInput::default();
        state.toggle_archive(&mut outcome);
        assert!(outcome.actions.is_empty());
        assert_eq!(
            state.archive.message.as_deref(),
            Some(" Server update required")
        );
        state.toggle_archive(&mut outcome);
        state.set_endpoint_methods(Some(vec![
            "workspace.archive.list".into(),
            "workspace.archive.resume".into(),
        ]));
        state.toggle_archive(&mut outcome);
        let [ClientShellAction::Endpoint { request, .. }] = &outcome.actions[..] else {
            panic!("expected archive list")
        };
        assert!(matches!(request.method, Method::WorkspaceArchiveList(_)));
        let id = request.id.clone();
        state.handle_endpoint_result(
            "boot-1",
            &id,
            Ok(ResponseResult::ArchiveList {
                entries: vec![ArchiveEntryInfo {
                    id: "archive-1".into(),
                    name: "CRM cleanup".into(),
                    resumable: true,
                }],
            }),
        );
        let mut resume = ClientShellInput::default();
        state.resume_archive("archive-1".into(), &mut resume);
        let [ClientShellAction::Endpoint { request, .. }] = &resume.actions[..] else {
            panic!("expected archive resume")
        };
        assert!(
            matches!(&request.method, Method::WorkspaceArchiveResume(params) if params.id == "archive-1")
        );
        state.active_endpoint_id = ClientEndpointId::Ssh("different".into());
        let mut stale = ClientShellInput::default();
        state.resume_archive("archive-1".into(), &mut stale);
        assert!(stale.actions.is_empty());
    }
}
