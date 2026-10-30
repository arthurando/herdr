use super::render::{display_width, put_right_text, put_text, ShellRenderState};
use super::*;

fn collapsed_groups_for_endpoint<'a>(
    state: &'a ShellRenderState<'_>,
    endpoint_id: &ClientEndpointId,
) -> Option<&'a HashSet<String>> {
    if endpoint_id.is_local() {
        Some(state.collapsed_groups)
    } else {
        state.remote_collapsed_groups.get(endpoint_id)
    }
}

pub(super) fn render_collapsed(
    buffer: &mut Buffer,
    area: Rect,
    config: &ClientShellConfig,
    state: &mut ShellRenderState<'_>,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    super::render::render_sidebar_background(buffer, area, palette);
    let (workspace_area, divider_y, detail_area) = super::sidebar::collapsed_sidebar_sections(area);
    let mut total_rows = 0usize;
    let mut selected_row = None;
    let reveal = std::mem::take(state.reveal_navigation_workspace);
    for endpoint in state.endpoints {
        total_rows += 1;
        if state.collapsed_endpoints.contains(&endpoint.endpoint_id) {
            continue;
        }
        if let Some(snapshot) = endpoint.snapshot.as_deref() {
            if reveal {
                if let Some(target) = state
                    .selected_workspace_id
                    .filter(|target| target.endpoint_id == endpoint.endpoint_id)
                {
                    selected_row = snapshot
                        .workspaces
                        .iter()
                        .position(|workspace| workspace.workspace_id == target.workspace_id)
                        .map(|index| total_rows + index);
                }
            }
            total_rows += snapshot.workspaces.len();
        }
    }
    let height = usize::from(workspace_area.height);
    let max_scroll = total_rows.saturating_sub(height);
    *state.workspace_scroll = (*state.workspace_scroll).min(max_scroll);
    if let Some(row) = selected_row {
        if row < *state.workspace_scroll {
            *state.workspace_scroll = row;
        } else if row >= state.workspace_scroll.saturating_add(height) {
            *state.workspace_scroll = row.saturating_add(1).saturating_sub(height).min(max_scroll);
        }
    }
    hits.workspace_max_scroll = max_scroll;
    let mut skip = *state.workspace_scroll;
    let mut y = workspace_area.y;
    for (index, endpoint) in state.endpoints.iter().enumerate() {
        if y >= workspace_area.bottom() {
            break;
        }
        let rect = Rect::new(workspace_area.x, y, workspace_area.width, 1);
        let active = &endpoint.endpoint_id == state.active_endpoint_id;
        let collapsed = state.collapsed_endpoints.contains(&endpoint.endpoint_id);
        if skip > 0 {
            skip -= 1;
        } else {
            if active && collapsed {
                buffer.set_style(rect, Style::default().bg(palette.active_row_bg));
            }
            let label = if endpoint.endpoint_id.is_local() {
                "L".to_owned()
            } else {
                (index + 1).to_string()
            };
            let marker = if collapsed { "▸" } else { "▾" };
            put_text(
                buffer,
                rect.x,
                rect.y,
                rect.width.saturating_sub(1),
                &format!("{marker}{label}"),
                Style::default().fg(if endpoint.status == ClientEndpointStatus::Online {
                    palette.text
                } else {
                    palette.overlay0
                }),
            );
            let mut status_badge = Rect::default();
            if !endpoint.endpoint_id.is_local() {
                let (glyph, _, color) = endpoint_status_presentation(endpoint.status, palette);
                let width = display_width(glyph).min(rect.width);
                status_badge = Rect::new(rect.right().saturating_sub(width), rect.y, width, 1);
                put_right_text(
                    buffer,
                    rect,
                    rect.y,
                    glyph,
                    state.machine_diagnostics.badge_style(
                        endpoint,
                        palette,
                        Style::default().fg(color),
                    ),
                );
            }
            hits.machines.push(MachineHit {
                rect,
                status_badge,
                collapse_toggle: Rect::new(rect.x, rect.y, u16::from(rect.width > 1), 1),
                endpoint_id: endpoint.endpoint_id.clone(),
            });
            y = y.saturating_add(1);
        }
        if collapsed {
            continue;
        }
        let Some(snapshot) = endpoint.snapshot.as_deref() else {
            continue;
        };
        for workspace in &snapshot.workspaces {
            if skip > 0 {
                skip -= 1;
                continue;
            }
            if y >= workspace_area.bottom() {
                break;
            }
            let rect = Rect::new(workspace_area.x, y, workspace_area.width, 1);
            let focused = active && workspace.focused;
            let selected = state.selected_workspace_id.is_some_and(|target| {
                target.matches(&endpoint.endpoint_id, &workspace.workspace_id)
            });
            let selection_background = if palette.selection_bg == ratatui::style::Color::Reset {
                palette.active_row_bg
            } else {
                palette.selection_bg
            };
            if selected {
                buffer.set_style(rect, Style::default().bg(selection_background));
            } else if focused {
                buffer.set_style(
                    rect,
                    Style::default().bg(super::sidebar::workspace_active_background(
                        palette,
                        state.selected_workspace_id.is_some(),
                    )),
                );
            }
            let stale = endpoint.status != ClientEndpointStatus::Online;
            let number = format!(" {}", workspace.number);
            let number_width = super::render::display_width(&number).min(rect.width);
            let dim = if stale {
                Modifier::DIM
            } else {
                Modifier::empty()
            };
            put_text(
                buffer,
                rect.x,
                rect.y,
                number_width,
                &number,
                Style::default()
                    .fg(if focused && !stale {
                        palette.text
                    } else {
                        palette.overlay0
                    })
                    .add_modifier(dim),
            );
            put_text(
                buffer,
                rect.x.saturating_add(number_width),
                rect.y,
                rect.width.saturating_sub(number_width),
                status_icon(workspace.agent_status, config.status_indicators),
                Style::default()
                    .fg(if stale {
                        palette.overlay0
                    } else {
                        status_color(workspace.agent_status, palette)
                    })
                    .add_modifier(dim),
            );
            hits.workspaces.push(WorkspaceHit {
                rect,
                endpoint_id: endpoint.endpoint_id.clone(),
                workspace_id: workspace.workspace_id.clone(),
                indented: false,
                group_toggle: None,
            });
            y = y.saturating_add(1);
        }
    }
    if let Some(divider_y) = divider_y {
        put_text(
            buffer,
            workspace_area.x,
            divider_y,
            workspace_area.width,
            &"─".repeat(workspace_area.width as usize),
            Style::default().fg(palette.surface_dim),
        );
    }
    super::endpoint_agents::render_collapsed(
        buffer,
        detail_area,
        state.endpoints,
        state.active_endpoint_id,
        config,
        hits,
    );
    hits.sidebar_toggle = if area.is_empty() || workspace_area.width == 0 {
        Rect::default()
    } else {
        Rect::new(
            workspace_area.x + workspace_area.width / 2,
            area.bottom().saturating_sub(1),
            1,
            1,
        )
    };
    put_text(
        buffer,
        hits.sidebar_toggle.x,
        hits.sidebar_toggle.y,
        hits.sidebar_toggle.width,
        "»",
        Style::default().fg(palette.overlay0),
    );
}

pub(super) fn render_expanded(
    buffer: &mut Buffer,
    area: Rect,
    active_snapshot: Option<&ClientShellSnapshot>,
    config: &ClientShellConfig,
    state: &mut ShellRenderState<'_>,
    hits: &mut ShellHitMap,
) {
    let palette = &config.palette;
    super::render::render_sidebar_background(buffer, area, palette);
    hits.sidebar_divider = if area.is_empty() {
        Rect::default()
    } else {
        Rect::new(area.right().saturating_sub(1), area.y, 1, area.height)
    };
    let custom_view = active_snapshot.and_then(|snapshot| snapshot.agent_view_label.as_deref());
    let (workspace_area, detail_area) = if custom_view.is_some() {
        hits.sidebar_section_divider =
            crate::ui::sidebar_section_divider_rect(area, state.sidebar_section_split);
        crate::ui::expanded_sidebar_sections(area, state.sidebar_section_split)
    } else {
        hits.sidebar_section_divider = Rect::default();
        (
            Rect::new(area.x, area.y, area.width.saturating_sub(1), area.height),
            Rect::default(),
        )
    };
    put_text(
        buffer,
        workspace_area.x,
        workspace_area.y,
        workspace_area.width,
        " machines",
        Style::default()
            .fg(palette.overlay0)
            .add_modifier(Modifier::BOLD),
    );
    if custom_view.is_none() && config.mouse_capture {
        let label = match config.agent_panel_sort {
            crate::config::AgentPanelSortConfig::Spaces => "grouped",
            crate::config::AgentPanelSortConfig::Priority => "priority",
        };
        let width = display_width(label).min(workspace_area.width.saturating_sub(11));
        let rect = Rect::new(
            workspace_area.right().saturating_sub(width + 1),
            workspace_area.y,
            width,
            1,
        );
        hits.agent_sort_toggle = rect;
        put_text(
            buffer,
            rect.x,
            rect.y,
            rect.width,
            label,
            Style::default()
                .fg(palette.overlay0)
                .add_modifier(Modifier::BOLD),
        );
    }

    let empty_collapsed_groups = HashSet::new();
    let agents =
        super::endpoint_agents::agent_rows(state.endpoints, state.active_endpoint_id, config);
    let mut agents_by_workspace = std::collections::HashMap::new();
    for (index, agent) in agents.iter().enumerate() {
        agents_by_workspace
            .entry((&agent.endpoint_id, agent.workspace_id.as_str()))
            .or_insert_with(Vec::new)
            .push(index);
    }
    let custom_agent_rows = config.agents != crate::config::AgentsSidebarConfig::default();

    enum Row {
        Endpoint(usize),
        Workspace {
            endpoint: usize,
            entry: WorkspaceEntry,
        },
        Agent(usize),
    }
    let mut rows = Vec::new();
    for (endpoint_index, endpoint) in state.endpoints.iter().enumerate() {
        rows.push(Row::Endpoint(endpoint_index));
        if let Some(snapshot) = endpoint.snapshot.as_deref() {
            let collapsed_groups = collapsed_groups_for_endpoint(state, &endpoint.endpoint_id)
                .unwrap_or(&empty_collapsed_groups);
            let entries = super::sidebar::workspace_entries(snapshot, collapsed_groups);
            let mut groups: Vec<Vec<WorkspaceEntry>> = Vec::new();
            for entry in entries {
                if !entry.indented || groups.is_empty() {
                    groups.push(Vec::new());
                }
                groups.last_mut().unwrap().push(entry);
            }
            if custom_view.is_none()
                && config.agent_panel_sort == crate::config::AgentPanelSortConfig::Priority
            {
                groups.sort_by_key(|group| {
                    group
                        .iter()
                        .filter_map(|entry| {
                            let workspace = &snapshot.workspaces[entry.index];
                            agents_by_workspace
                                .get(&(&endpoint.endpoint_id, workspace.workspace_id.as_str()))
                                .and_then(|items| items.first())
                                .copied()
                        })
                        .min()
                        .unwrap_or(usize::MAX)
                });
            }
            for entry in groups.into_iter().flatten() {
                let workspace_id = &snapshot.workspaces[entry.index].workspace_id;
                let matching =
                    agents_by_workspace.get(&(&endpoint.endpoint_id, workspace_id.as_str()));
                rows.push(Row::Workspace {
                    endpoint: endpoint_index,
                    entry,
                });
                if let Some(matching) =
                    matching.filter(|matching| custom_view.is_none() && matching.len() > 1)
                {
                    rows.extend(matching.iter().copied().map(Row::Agent));
                }
            }
        }
    }
    let archive_height = if state.archive.is_open() {
        workspace_area
            .height
            .saturating_sub(WORKSPACE_HEADER_ROWS + 2)
            .min(8)
    } else {
        0
    };
    let body = Rect::new(
        workspace_area.x,
        workspace_area.y.saturating_add(WORKSPACE_HEADER_ROWS),
        workspace_area.width,
        workspace_area
            .height
            .saturating_sub(WORKSPACE_HEADER_ROWS + 1 + archive_height),
    );
    hits.workspace_body = body;
    let row_heights = rows
        .iter()
        .map(|row| match row {
            Row::Endpoint(_) => 1,
            Row::Agent(index) => agents[*index]
                .agent
                .rows
                .len()
                .max(1)
                .min(u16::MAX as usize) as u16,
            Row::Workspace { endpoint, entry } => {
                let endpoint = &state.endpoints[*endpoint];
                let collapsed_groups = collapsed_groups_for_endpoint(state, &endpoint.endpoint_id)
                    .unwrap_or(&empty_collapsed_groups);
                endpoint
                    .snapshot
                    .as_deref()
                    .and_then(|snapshot| {
                        let workspace = snapshot.workspaces.get(entry.index)?;
                        if custom_view.is_none() && custom_agent_rows {
                            if let Some(items) = agents_by_workspace
                                .get(&(&endpoint.endpoint_id, workspace.workspace_id.as_str()))
                                .filter(|items| items.len() == 1)
                            {
                                return Some(
                                    agents[items[0]]
                                        .agent
                                        .rows
                                        .len()
                                        .max(1)
                                        .min(u16::MAX as usize)
                                        as u16,
                                );
                            }
                        }
                        Some(
                            super::sidebar::workspace_rows(
                                workspace,
                                super::sidebar::displayed_workspace_status(
                                    snapshot,
                                    workspace,
                                    collapsed_groups,
                                ),
                                entry.indented,
                                &config.spaces,
                            )
                            .len()
                            .max(1)
                            .min(u16::MAX as usize) as u16,
                        )
                    })
                    .unwrap_or(1)
            }
        })
        .collect::<Vec<_>>();
    let gaps = rows
        .iter()
        .enumerate()
        .map(|(index, row)| match (row, rows.get(index + 1)) {
            (
                Row::Workspace { endpoint, .. },
                Some(Row::Workspace {
                    endpoint: next_endpoint,
                    entry,
                }),
            ) if endpoint == next_endpoint => u16::from(!entry.indented) * config.spaces.row_gap,
            _ => 0,
        })
        .collect::<Vec<_>>();
    let reveal_navigation = !body.is_empty() && std::mem::take(state.reveal_navigation_workspace);
    let reveal_focus = !body.is_empty() && std::mem::take(state.reveal_focused_workspace);
    if reveal_navigation || reveal_focus {
        let selected_row = rows.iter().position(|row| match row {
            Row::Workspace { endpoint, entry } => {
                let endpoint = &state.endpoints[*endpoint];
                endpoint
                    .snapshot
                    .as_deref()
                    .and_then(|snapshot| snapshot.workspaces.get(entry.index))
                    .is_some_and(|workspace| {
                        if reveal_navigation {
                            state.selected_workspace_id.is_some_and(|target| {
                                target.matches(&endpoint.endpoint_id, &workspace.workspace_id)
                            })
                        } else {
                            &endpoint.endpoint_id == state.active_endpoint_id
                                && active_snapshot.is_some_and(|snapshot| {
                                    snapshot.focused_workspace_id.as_deref()
                                        == Some(workspace.workspace_id.as_str())
                                })
                        }
                    })
            }
            Row::Endpoint(_) | Row::Agent(_) => false,
        });
        if let Some(selected_row) = selected_row {
            *state.workspace_scroll = super::scroll::list_scroll_start_to_reveal(
                &row_heights,
                &gaps,
                body.height,
                *state.workspace_scroll,
                selected_row,
            );
        }
    }
    let metrics = super::scroll::list_scroll_metrics(
        &row_heights,
        &gaps,
        body.height,
        *state.workspace_scroll,
    );
    hits.workspace_max_scroll = metrics.max_offset_from_bottom;
    hits.workspace_scroll_metrics = Some(metrics);
    *state.workspace_scroll = metrics
        .max_offset_from_bottom
        .saturating_sub(metrics.offset_from_bottom);
    let show_scrollbar = metrics.max_offset_from_bottom > 0 && body.width > 1;
    let content_width = body.width.saturating_sub(u16::from(show_scrollbar));
    let mut y = body.y;
    for (row_index, row) in rows.iter().enumerate().skip(*state.workspace_scroll) {
        match row {
            Row::Endpoint(index) => {
                if y >= body.bottom() {
                    break;
                }
                let endpoint = &state.endpoints[*index];
                let rect = Rect::new(body.x, y, content_width, 1);
                let status_badge = render_endpoint_row(
                    buffer,
                    rect,
                    "",
                    endpoint,
                    false,
                    state.machine_diagnostics,
                    palette,
                );
                hits.machines.push(MachineHit {
                    rect,
                    status_badge,
                    collapse_toggle: Rect::default(),
                    endpoint_id: endpoint.endpoint_id.clone(),
                });
                y = y
                    .saturating_add(1)
                    .saturating_add(gaps.get(row_index).copied().unwrap_or(0));
            }
            Row::Agent(index) => {
                let agent = &agents[*index];
                let height =
                    (agent.agent.rows.len().max(1).min(u16::MAX as usize) as u16).min(body.height);
                if y.saturating_add(height) > body.bottom() {
                    break;
                }
                let rect = Rect::new(
                    body.x.saturating_add(1),
                    y,
                    content_width.saturating_sub(1),
                    height,
                );
                super::agent_sidebar::render_agent_row(buffer, rect, &agent.agent, config);
                if agent.stale {
                    buffer.set_style(
                        rect,
                        Style::default()
                            .fg(palette.overlay0)
                            .add_modifier(Modifier::DIM),
                    );
                }
                hits.endpoint_agents.push((
                    rect,
                    agent.endpoint_id.clone(),
                    agent.agent.pane_id.clone(),
                ));
                if &agent.endpoint_id == state.active_endpoint_id {
                    hits.agents.push((rect, agent.agent.pane_id.clone()));
                }
                y = y
                    .saturating_add(height)
                    .saturating_add(gaps.get(row_index).copied().unwrap_or(0));
            }
            Row::Workspace { endpoint, entry } => {
                let endpoint = &state.endpoints[*endpoint];
                let Some(snapshot) = endpoint.snapshot.as_deref() else {
                    continue;
                };
                let Some(workspace) = snapshot.workspaces.get(entry.index) else {
                    continue;
                };
                let collapsed_groups = collapsed_groups_for_endpoint(state, &endpoint.endpoint_id)
                    .unwrap_or(&empty_collapsed_groups);
                let status = super::sidebar::displayed_workspace_status(
                    snapshot,
                    workspace,
                    collapsed_groups,
                );
                let tokens = super::sidebar::workspace_rows(
                    workspace,
                    status,
                    entry.indented,
                    &config.spaces,
                );
                let single_agent = if custom_view.is_none() && custom_agent_rows {
                    agents_by_workspace
                        .get(&(&endpoint.endpoint_id, workspace.workspace_id.as_str()))
                        .filter(|items| items.len() == 1)
                        .map(|items| &agents[items[0]])
                } else {
                    None
                };
                let height = (single_agent
                    .map_or(tokens.len(), |agent| agent.agent.rows.len())
                    .max(1)
                    .min(u16::MAX as usize) as u16)
                    .min(body.height);
                if y.saturating_add(height) > body.bottom() {
                    break;
                }
                let rect = Rect::new(body.x, y, content_width, height);
                let nested = Rect::new(
                    rect.x.saturating_add(2),
                    rect.y,
                    rect.width.saturating_sub(2),
                    rect.height,
                );
                let endpoint_active = &endpoint.endpoint_id == state.active_endpoint_id;
                let selected = state.selected_workspace_id.is_some_and(|target| {
                    target.matches(&endpoint.endpoint_id, &workspace.workspace_id)
                });
                let dragged = endpoint_active
                    && state.dragged_workspace_id == Some(workspace.workspace_id.as_str());
                if selected {
                    let background = if palette.selection_bg == ratatui::style::Color::Reset {
                        palette.active_row_bg
                    } else {
                        palette.selection_bg
                    };
                    buffer.set_style(rect, Style::default().bg(background));
                } else if dragged {
                    buffer.set_style(rect, Style::default().bg(palette.surface1));
                } else if endpoint_active && workspace.focused {
                    buffer.set_style(
                        rect,
                        Style::default().bg(super::sidebar::workspace_active_background(
                            palette,
                            state.selected_workspace_id.is_some(),
                        )),
                    );
                }
                if let Some(agent) = single_agent {
                    super::agent_sidebar::render_agent_row(buffer, nested, &agent.agent, config);
                } else {
                    super::sidebar::render_workspace_rows(
                        buffer,
                        nested,
                        status,
                        config.status_indicators,
                        entry,
                        tokens,
                        endpoint_active && workspace.focused,
                        selected,
                        state.selected_workspace_id.is_some(),
                        dragged,
                        palette,
                        Some(row_index),
                    );
                }
                if endpoint.status != ClientEndpointStatus::Online {
                    buffer.set_style(
                        rect,
                        Style::default()
                            .fg(palette.overlay0)
                            .add_modifier(Modifier::DIM),
                    );
                }
                let group_toggle = super::sidebar::render_parent_group_toggle(
                    buffer,
                    rect,
                    snapshot,
                    entry.index,
                    collapsed_groups,
                    palette,
                );
                hits.workspaces.push(WorkspaceHit {
                    rect,
                    endpoint_id: endpoint.endpoint_id.clone(),
                    workspace_id: workspace.workspace_id.clone(),
                    indented: entry.indented,
                    group_toggle,
                });
                if custom_view.is_none() {
                    if let Some(matching) = agents_by_workspace
                        .get(&(&endpoint.endpoint_id, workspace.workspace_id.as_str()))
                        .filter(|matching| matching.len() == 1)
                    {
                        let agent = &agents[matching[0]];
                        hits.endpoint_agents.push((
                            nested,
                            endpoint.endpoint_id.clone(),
                            agent.agent.pane_id.clone(),
                        ));
                        if endpoint_active {
                            hits.agents.push((nested, agent.agent.pane_id.clone()));
                        }
                    }
                }
                y = y
                    .saturating_add(height)
                    .saturating_add(gaps.get(row_index).copied().unwrap_or(0));
            }
        }
    }
    if show_scrollbar {
        let track = Rect::new(body.right().saturating_sub(1), body.y, 1, body.height);
        hits.workspace_scrollbar = track;
        super::scroll::render_list_scrollbar(buffer, track, metrics, palette);
    }
    if let Some(row) = state
        .workspace_drop_indicator_row
        .filter(|row| *row >= body.y && *row < body.bottom())
    {
        put_text(
            buffer,
            body.x,
            row,
            body.width,
            &"─".repeat(body.width as usize),
            Style::default().fg(palette.accent),
        );
    }

    let footer_y = workspace_area.bottom().saturating_sub(1);
    if archive_height > 0 {
        let archive_area = Rect::new(
            workspace_area.x,
            footer_y.saturating_sub(archive_height),
            workspace_area.width,
            archive_height,
        );
        super::archive::render(buffer, archive_area, state.archive, palette, hits);
    }
    if config.mouse_capture {
        let label = " [+] New";
        let attention = active_snapshot.is_some_and(super::global_menu::global_menu_attention);
        let width = if attention { 8 } else { 6 }.min(workspace_area.width);
        let archive_width = 8.min(workspace_area.width.saturating_sub(width));
        let new_width = workspace_area.width.saturating_sub(width + archive_width);
        hits.new_workspace = Rect::new(
            workspace_area.x,
            footer_y,
            display_width(label).min(new_width),
            u16::from(workspace_area.height > 0),
        );
        put_text(
            buffer,
            workspace_area.x,
            footer_y,
            new_width,
            label,
            Style::default().fg(palette.overlay0),
        );
        hits.archive_toggle = Rect::new(
            workspace_area.x.saturating_add(new_width),
            footer_y,
            archive_width,
            1,
        );
        put_text(
            buffer,
            hits.archive_toggle.x,
            footer_y,
            archive_width,
            " Archive",
            Style::default().fg(if state.archive.is_open() {
                palette.accent
            } else {
                palette.overlay0
            }),
        );
        hits.global_launcher = Rect::new(
            workspace_area.right().saturating_sub(width),
            footer_y,
            width,
            1,
        );
        put_right_text(
            buffer,
            workspace_area,
            footer_y,
            if attention { "● menu" } else { "menu" },
            Style::default().fg(if attention {
                palette.accent
            } else {
                palette.overlay0
            }),
        );
    }
    if custom_view.is_some() {
        super::endpoint_agents::render_expanded(
            buffer,
            detail_area,
            custom_view,
            state.endpoints,
            state.active_endpoint_id,
            config,
            state.agent_scroll,
            hits,
        );
    }
    hits.sidebar_toggle = Rect::new(
        area.right().saturating_sub(2),
        area.y,
        u16::from(area.width > 1),
        u16::from(area.height > 0),
    );
    put_text(
        buffer,
        hits.sidebar_toggle.x,
        hits.sidebar_toggle.y,
        hits.sidebar_toggle.width,
        "«",
        Style::default().fg(palette.overlay0),
    );
}

fn render_endpoint_row(
    buffer: &mut Buffer,
    rect: Rect,
    marker: &str,
    endpoint: &ClientShellEndpoint,
    highlighted: bool,
    auth: &super::machine_diagnostics::MachineDiagnostics,
    palette: &Palette,
) -> Rect {
    buffer.set_style(
        rect,
        Style::default().bg(if highlighted {
            palette.active_row_bg
        } else {
            palette.panel_bg
        }),
    );
    let (glyph, state, color) = endpoint_status_presentation(endpoint.status, palette);
    let state = if endpoint.status == ClientEndpointStatus::Online {
        ""
    } else {
        state
    };
    let signal = if auth.required_for(endpoint) {
        "! auth".to_owned()
    } else if endpoint.status == ClientEndpointStatus::Attention {
        "! error".to_owned()
    } else if endpoint.endpoint_id.is_local() {
        String::new()
    } else if state.is_empty() {
        glyph.to_owned()
    } else {
        format!("{glyph} {state}")
    };
    let signal_width = display_width(&signal).min(rect.width);
    put_text(
        buffer,
        rect.x,
        rect.y,
        rect.width.saturating_sub(signal_width.saturating_add(1)),
        &format!(" {marker} {}", endpoint.label),
        Style::default()
            .fg(
                if matches!(endpoint.status, ClientEndpointStatus::Disabled) {
                    palette.overlay0
                } else {
                    palette.text
                },
            )
            .add_modifier(Modifier::BOLD),
    );
    put_right_text(
        buffer,
        rect,
        rect.y,
        &signal,
        auth.badge_style(endpoint, palette, Style::default().fg(color)),
    );
    Rect::new(
        rect.right().saturating_sub(signal_width),
        rect.y,
        signal_width,
        1,
    )
}
