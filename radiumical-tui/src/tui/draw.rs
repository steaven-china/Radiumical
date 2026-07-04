use crate::tui::app::App;
use crate::tui::PULSE;
use radiumical_core::types::AgentMode;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block as RBlock, BorderType, Borders, Paragraph};
use ratatui::Frame;

// ═══ Draw ═══

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let hint_page_start = app.input.hint_page * 8;
    let hint_page_end = (hint_page_start + 8).min(app.input.hints.len());
    let visible_hints: Vec<(String, String)> =
        app.input.hints[hint_page_start..hint_page_end].to_vec();
    let hint_count = visible_hints.len();
    let input_lines = app.input.text.split('\n').count().clamp(1, 5);
    let input_h = (input_lines + 2) as u16;
    let status_h = 1u16;
    let bottom_h = (input_h as usize + hint_count + status_h as usize)
        .min(area.height.saturating_sub(1) as usize) as u16;
    let output_h = area.height.saturating_sub(bottom_h).max(1);
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(output_h), Constraint::Length(bottom_h)])
        .split(area);
    draw_output(f, chunks[0], app, chunks[0].height as usize);

    // ── Panel-driven floating overlays (no overlap) ──
    // Sync panel state from legacy flags.
    sync_panels(app);
    let slots = app.panels.layout(chunks[0]);
    for slot in &slots {
        let title = slot.id.title();
        let border = match slot.id {
            crate::panel::PanelId::Confirm => Color::Yellow,
            crate::panel::PanelId::Perf => Color::Rgb(100, 100, 110),
            _ => Color::Cyan,
        };
        let bg = match slot.id {
            crate::panel::PanelId::Perf => Color::Rgb(15, 15, 20),
            _ => Color::Rgb(20, 20, 25),
        };

        let _inner = Rect {
            x: slot.rect.x + 1,
            y: slot.rect.y + 1,
            width: slot.rect.width.saturating_sub(2),
            height: slot.rect.height.saturating_sub(2),
        };

        match slot.id {
            crate::panel::PanelId::Confirm => {
                app.confirm.render_at(f, slot.rect);
            }
            crate::panel::PanelId::Perf => {
                draw_perf_overlay_at(f, slot.rect);
            }
            crate::panel::PanelId::Dashboard => {
                app.dashboard.render_at(f, slot.rect);
            }
            crate::panel::PanelId::ProviderPicker => {
                app.provider_picker.render_at(f, slot.rect);
            }
            crate::panel::PanelId::Settings => {
                app.settings_board.render_at(f, slot.rect);
            }
            crate::panel::PanelId::SubAgents => {
                crate::panel::PanelManager::render_panel_frame(f, slot, title, border, bg);
                crate::panels::subagents::render(f, slot);
            }
            crate::panel::PanelId::Mcp => {
                crate::panel::PanelManager::render_panel_frame(f, slot, title, border, bg);
                crate::panels::mcp_status::render(
                    f,
                    slot,
                    &app.mcp_servers,
                    app.overlays.mcp_selected,
                );
            }
            crate::panel::PanelId::Outline => {
                crate::panel::PanelManager::render_panel_frame(f, slot, title, border, bg);
            }
            crate::panel::PanelId::Diagnostics => {
                crate::panel::PanelManager::render_panel_frame(f, slot, title, border, bg);
            }
            crate::panel::PanelId::Memory => {
                crate::panel::PanelManager::render_panel_frame(f, slot, title, border, bg);
            }
            crate::panel::PanelId::Plan => {
                crate::panel::PanelManager::render_panel_frame(f, slot, title, border, bg);
                crate::panels::plan::render_plan_panel(
                    f,
                    slot,
                    &app.overlays.plan_title,
                    &app.overlays.plan_tasks,
                );
            }
            crate::panel::PanelId::Agents => {
                crate::panel::PanelManager::render_panel_frame(f, slot, title, border, bg);
                crate::panels::agents::render_agents_panel(
                    f,
                    slot,
                    &app.overlays.agents_list,
                    &app.agent_role,
                );
            }
            _ => {
                crate::panel::PanelManager::render_panel_frame(f, slot, title, border, bg);
            }
        }
    }

    // ── Help overlay (bottom-right, not tiled) ──
    if app.overlays.help {
        let help_w = 40u16.min(chunks[0].width.saturating_sub(2));
        let help_h = 20u16.min(chunks[0].height.saturating_sub(2));
        let help_r = Rect {
            x: chunks[0].x + chunks[0].width.saturating_sub(help_w + 1),
            y: chunks[0].y + chunks[0].height.saturating_sub(help_h + 1),
            width: help_w,
            height: help_h,
        };
        let help_lines = draw_help_overlay_lines();
        let help_slot = crate::panel::PanelSlot {
            id: crate::panel::PanelId::Help,
            rect: help_r,
        };
        crate::panel::PanelManager::render_panel_frame(
            f,
            &help_slot,
            " Help ",
            Color::DarkGray,
            Color::Rgb(20, 20, 25),
        );
        let inner = Rect {
            x: help_r.x + 1,
            y: help_r.y + 1,
            width: help_r.width.saturating_sub(2),
            height: help_r.height.saturating_sub(2),
        };
        f.render_widget(
            Paragraph::new(Text::from(help_lines)).wrap(ratatui::widgets::Wrap { trim: false }),
            inner,
        );
    }

    // ── Modal overlays (always on top of everything) ──
    if app.session_tui.visible {
        app.session_tui
            .render(f, chunks[0], &app.model, app.mode.clone());
    }
    app.choice_panel.render(f, area);

    // Bottom: input, hints, status
    let bottom = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [Constraint::Length(input_h)]
                .into_iter()
                .chain(std::iter::repeat_n(Constraint::Length(1), hint_count))
                .chain(std::iter::once(Constraint::Length(1)))
                .collect::<Vec<_>>(),
        )
        .split(chunks[1]);
    draw_input(f, bottom[0], app);

    // Toasts at top-center, offset below any top-occupied panels
    let top_offset = app
        .panels
        .top_occupied_bottom(chunks[0])
        .saturating_sub(chunks[0].y);
    let mut toast_y = top_offset;
    for toast in &app.toasts {
        if !toast.is_expired() {
            let w = (toast.message.len() as u16 + 4).min(area.width - 4);
            let x = (area.width - w) / 2;
            let r = Rect {
                x: area.x + x,
                y: area.y + toast_y,
                width: w,
                height: 3,
            };
            toast_y += 3;
            let color = match toast.level {
                crate::board::ToastLevel::Info => Color::Cyan,
                crate::board::ToastLevel::Warn => Color::Yellow,
                crate::board::ToastLevel::Error => Color::Red,
            };
            let block = RBlock::default()
                .borders(Borders::ALL)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(color));
            f.render_widget(Paragraph::new(toast.message.as_str()).block(block), r);
        }
    }
    app.toasts.retain(|t| !t.is_expired());

    app.progress.render(f, area);

    for (i, (n, d)) in visible_hints.iter().take(hint_count).enumerate() {
        let selected = app.input.hint_selected == Some(hint_page_start + i);
        draw_hint_row(f, bottom[1 + i], n, d, selected);
    }
    draw_status(f, bottom[bottom.len() - 1], app);
}

/// Sync PanelManager from legacy boolean flags.
fn sync_panels(app: &mut App) {
    use crate::panel::PanelId;
    let flags = [
        (PanelId::Dashboard, app.dashboard.visible),
        (PanelId::ProviderPicker, app.overlays.model_picker),
        (PanelId::Settings, app.overlays.settings),
        (PanelId::Confirm, app.confirm.visible),
        (PanelId::Perf, app.overlays.perf),
        (PanelId::Outline, app.overlays.outline),
        (PanelId::Diagnostics, app.overlays.diagnostics),
        (PanelId::Memory, app.overlays.memory),
        (PanelId::SubAgents, app.overlays.subagents),
        (PanelId::Mcp, app.overlays.mcp),
        (PanelId::Plan, app.overlays.plan),
        (PanelId::Agents, app.overlays.agents),
    ];
    for (id, visible) in flags {
        if visible && !app.panels.is_open(id) {
            app.panels.open(id);
        } else if !visible && app.panels.is_open(id) {
            app.panels.close(id);
        }
    }
}

fn draw_output(f: &mut Frame, area: Rect, app: &mut App, _vis: usize) {
    use crate::layout::measure_blocks;
    use crate::tui::app::mouse::tool_call_key;
    use ratatui::widgets::{Clear, Wrap};
    if app.output.is_empty() {
        return;
    }
    let vis = _vis.min(area.height as usize).max(1);
    app.viewport.visible_lines = vis;
    // Use previous frame's rendered_total for scrollbar decision (updated below)
    let needs_scrollbar = app.viewport.rendered_total > vis;
    let text_area = Rect {
        x: area.x,
        y: area.y,
        width: if needs_scrollbar {
            area.width.saturating_sub(1)
        } else {
            area.width
        },
        height: area.height,
    };
    app.viewport.width = text_area.width as usize;
    f.render_widget(Clear, area);

    app.blocks = measure_blocks(
        &app.output,
        text_area.width,
        app.thinking.show_full_reasoning,
    );
    for block in &mut app.blocks {
        if let crate::layout::BlockKind::ToolCall {
            name,
            args,
            result,
            expanded,
            result_scroll,
        } = &block.kind
        {
            let key = tool_call_key(block);
            let want = app.tool_expanded.get(&key).copied().unwrap_or(false);
            let scroll = app.tool_result_scroll.get(&key).copied().unwrap_or(0);
            if want != *expanded || scroll != *result_scroll {
                // content width inside the box borders
                let content_w = (text_area.width as usize).saturating_sub(4 + 1).max(1);
                let wrapped_count =
                    crate::layout::wrapped_tool_result_lines(result, content_w).len();
                const MAX_RESULT_VIS: usize = 10;
                let visible_count = wrapped_count.min(MAX_RESULT_VIS);
                block.kind = crate::layout::BlockKind::ToolCall {
                    name: name.clone(),
                    args: args.clone(),
                    result: result.clone(),
                    expanded: want,
                    result_scroll: scroll,
                };
                // collapsed: top(1)+args(1)+bottom(1)+hint(1) = 4
                // expanded:  top(1)+args(1)+sep(1)+results(N)+bottom(1)+hint(1) = 5 + N
                block.height = if want && visible_count > 0 {
                    5 + visible_count
                } else {
                    4
                };
            }
        }
    }
    let total: usize = app.blocks.iter().map(|b| b.height).sum();
    app.viewport.rendered_total = total;

    let start = app.scroll_start(total, vis);
    let end = (start + vis).min(total);

    app.markdown.tick_frame();
    let mut rendered_blocks: Vec<Vec<Line>> = Vec::with_capacity(app.blocks.len());
    for block in &app.blocks {
        if matches!(block.kind, crate::layout::BlockKind::Logo) {
            rendered_blocks.push(block.render(
                text_area.width,
                app.thinking.frame,
                &mut app.markdown,
                app.thinking.show_full_reasoning,
            ));
            continue;
        }
        let key = block_render_key(block, text_area.width, app.thinking.show_full_reasoning);
        if let Some(lines) = app.render_cache.get(&key) {
            rendered_blocks.push(lines.clone());
        } else {
            let lines = block.render(
                text_area.width,
                app.thinking.frame,
                &mut app.markdown,
                app.thinking.show_full_reasoning,
            );
            // LRU eviction: limit cache to 512 entries.
            const MAX_CACHE: usize = 512;
            if app.render_cache.len() >= MAX_CACHE {
                if let Some(oldest) = app.render_cache_order.pop_front() {
                    app.render_cache.remove(&oldest);
                }
            }
            app.render_cache.insert(key, lines.clone());
            app.render_cache_order.push_back(key);
            rendered_blocks.push(lines);
        }
    }

    let mut rendered: Vec<Line> = Vec::with_capacity(vis);
    let mut line_offset = 0usize;
    for (bi, block) in app.blocks.iter().enumerate() {
        let block_end = line_offset + block.height;
        if block_end > start && line_offset < end {
            let skip = start.saturating_sub(line_offset);
            let take = vis.saturating_sub(rendered.len());
            let all_lines = &rendered_blocks[bi];
            let block_start = skip.min(all_lines.len());
            let block_end_idx = (block_start + take).min(all_lines.len());
            let hovered = app.hovered_block == Some(bi);
            for bline in all_lines[block_start..block_end_idx].iter() {
                let mut line = bline.clone();
                if hovered {
                    line = line.style(Style::default().bg(Color::Rgb(45, 45, 55)));
                }
                rendered.push(line);
            }
        }
        line_offset = block_end;
        if rendered.len() >= vis {
            break;
        }
    }
    let mut filled = rendered;
    filled.resize(vis, Line::from(""));

    if total > vis && vis > 0 {
        let sb_h = area.height.saturating_sub(1) as usize;
        let thumb_h =
            ((vis as f32 / total.max(vis) as f32).min(1.0) * sb_h as f32).max(1.0) as usize;
        let thumb_y = if app.viewport.stick_to_bottom {
            sb_h.saturating_sub(thumb_h)
        } else {
            let progress = (app.viewport.scroll / (total - vis).max(1) as f32).clamp(0.0, 1.0);
            ((progress * sb_h.saturating_sub(thumb_h) as f32) as usize).min(sb_h.saturating_sub(1))
        };
        let mut bar = String::with_capacity(sb_h * 4);
        for i in 0..sb_h {
            bar.push(if i >= thumb_y && i < thumb_y + thumb_h {
                '█'
            } else {
                '│'
            });
            bar.push('\n');
        }
        f.render_widget(
            Paragraph::new(bar).style(Style::default().fg(Color::Rgb(60, 60, 70))),
            Rect {
                x: area.x + area.width - 1,
                y: area.y + 1,
                width: 1,
                height: sb_h as u16,
            },
        );
    }

    let content_h = filled.iter().filter(|l| l.width() > 0).count();
    if app.welcome && content_h < vis && app.viewport.scroll <= 0.0 && content_h > 0 {
        let pad_top = (vis - content_h) / 2;
        let max_w = filled.iter().map(|l| l.width()).max().unwrap_or(0) as u16;
        let pad_left = (text_area.width.saturating_sub(max_w) / 2) as usize;
        let mut padded: Vec<Line> = Vec::new();
        padded.resize(pad_top, Line::from(""));
        for line in filled {
            let line_w = line.width() as u16;
            let extra = (max_w.saturating_sub(line_w) / 2) as usize;
            let mut spans = vec![Span::raw(" ".repeat(pad_left + extra))];
            spans.extend(line.spans.into_iter());
            padded.push(Line::from(spans));
        }
        f.render_widget(
            Paragraph::new(Text::from(padded))
                .wrap(Wrap { trim: false })
                .style(Style::default().bg(Color::Reset)),
            text_area,
        );
    } else {
        f.render_widget(
            Paragraph::new(Text::from(filled))
                .wrap(Wrap { trim: false })
                .style(Style::default().bg(Color::Reset)),
            text_area,
        );
    }
}

fn draw_input(f: &mut Frame, area: Rect, app: &App) {
    use ratatui::widgets::{Block as RBlock, BorderType, Borders, Wrap};
    let lines: Vec<&str> = app.input.text.split('\n').collect();
    let cursor_line = app.input.text[..app.input.cursor]
        .chars()
        .filter(|c| *c == '\n')
        .count()
        .min(lines.len().saturating_sub(1));
    let cursor_col = app.input.cursor
        - app.input.text[..app.input.cursor]
            .rfind('\n')
            .map(|p| p + 1)
            .unwrap_or(0);
    let mut spans: Vec<Line> = Vec::new();
    for (li, line) in lines.iter().enumerate() {
        let mut row: Vec<Span> = Vec::new();
        row.push(Span::styled(
            if li == 0 { "> " } else { "  " },
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ));
        if li == cursor_line {
            let before = &line[..cursor_col.min(line.len())];
            let ch = if cursor_col < line.len() {
                line[cursor_col..].chars().next().unwrap_or(' ')
            } else {
                ' '
            };
            let after = if cursor_col < line.len() {
                &line[cursor_col + ch.len_utf8()..]
            } else {
                ""
            };
            row.push(Span::raw(before));
            row.push(Span::styled(
                ch.to_string(),
                Style::default().bg(Color::White).fg(Color::Black),
            ));
            row.push(Span::raw(after));
        } else {
            row.push(Span::raw(*line));
        }
        spans.push(Line::from(row));
    }
    let block = RBlock::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(Color::DarkGray));
    f.render_widget(
        Paragraph::new(Text::from(spans))
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_perf_overlay_at(f: &mut Frame, r: Rect) {
    let report = radiumical_core::perf::report();
    f.render_widget(
        Paragraph::new(report).style(
            Style::default()
                .fg(Color::Rgb(100, 100, 110))
                .bg(Color::Rgb(15, 15, 20)),
        ),
        r,
    );
}

fn draw_hint_row(f: &mut Frame, area: Rect, name: &str, desc: &str, selected: bool) {
    let bg = if selected {
        Style::default().bg(Color::Rgb(50, 50, 60))
    } else {
        Style::default()
    };
    let line = Line::from(vec![
        Span::styled(
            format!("  {name}"),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
                .add_modifier(if selected {
                    Modifier::UNDERLINED
                } else {
                    Modifier::empty()
                }),
        ),
        Span::styled(format!(" — {desc}"), Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(line).style(bg), area);
}

fn draw_help_overlay_lines() -> Vec<Line<'static>> {
    vec![
        Line::from(vec![
            Span::styled(
                " //",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("  Dashboard"),
        ]),
        Line::from(vec![
            Span::styled(
                " /help",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" All commands"),
        ]),
        Line::from(vec![
            Span::styled(
                " /provider",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Switch model"),
        ]),
        Line::from(vec![
            Span::styled(
                " /sessions",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Session mgr"),
        ]),
        Line::from(vec![
            Span::styled(
                " /retry",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Retry last"),
        ]),
        Line::from(vec![
            Span::styled(
                " /status",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Session info"),
        ]),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                " Ctrl+C",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Cancel/Quit"),
        ]),
        Line::from(vec![
            Span::styled(
                " Esc",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Close overlay"),
        ]),
        Line::from(vec![
            Span::styled(
                " ↑↓",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" History/hints"),
        ]),
        Line::from(vec![
            Span::styled(
                " Tab",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Autocomplete"),
        ]),
        Line::from(vec![
            Span::styled(
                " PgUp/Dn",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Scroll"),
        ]),
        Line::from(vec![
            Span::styled(
                " Ctrl+L",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Clear screen"),
        ]),
        Line::from(vec![
            Span::styled(
                " Ctrl+A/E",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" Line start/end"),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            " Press any key to close",
            Style::default().fg(Color::Rgb(100, 100, 110)),
        )),
    ]
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    use ratatui::layout::Alignment;
    use unicode_width::UnicodeWidthStr;

    let style = Style::default().fg(Color::Rgb(130, 130, 130));
    let dim_style = Style::default().fg(Color::Rgb(100, 100, 110));
    let filter_style = Style::default().fg(Color::Rgb(180, 180, 100));
    let left = if let Some(ref prefix) = app.input.history_filter_prefix {
        let display = if prefix.len() > 20 {
            format!("{}...", &prefix[..20])
        } else {
            prefix.clone()
        };
        Line::from(Span::styled(
            format!(" [↑ history: {display}]"),
            filter_style,
        ))
    } else if app.thinking.active {
        let bar = PULSE[app.thinking.frame % PULSE.len()];
        Line::from(vec![
            Span::styled(
                format!(" {} thinking {}s", bar, app.thinking.elapsed),
                style,
            ),
            Span::styled(" (Esc/Ctrl+C to cancel)", dim_style),
        ])
    } else if let Some(ref title) = app.session_title {
        let mode = match app.mode {
            AgentMode::Auto => "Auto",
            AgentMode::Plan => "Plan",
            AgentMode::Exec => "Exec",
        };
        let right_text = format!("{} | {}", app.model, mode);
        let right_len = right_text.width() + 2;
        let right_w = right_len.min(area.width.saturating_sub(8) as usize).max(8) as u16;
        let avail = area.width.saturating_sub(right_w).saturating_sub(2) as usize;
        let title_w = title.width();
        let display = if title_w > avail {
            let target = avail.saturating_sub(1);
            let mut truncated = String::new();
            let mut w = 0;
            for ch in title.chars() {
                let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                if w + cw > target {
                    break;
                }
                truncated.push(ch);
                w += cw;
            }
            truncated.push('\u{2026}');
            truncated
        } else {
            title.clone()
        };
        Line::from(Span::styled(format!(" {display}"), style))
    } else {
        let tip_style = Style::default().fg(Color::Rgb(160, 160, 100));
        if app.tip_state.enabled {
            Line::from(vec![
                Span::styled(" Ready", style),
                Span::styled(format!("  {} ", app.tip_state.text()), tip_style),
            ])
        } else {
            Line::from(vec![
                Span::styled(" Ready", style),
                Span::styled("  // dashboard  /help commands  Ctrl+C cancel", dim_style),
            ])
        }
    };
    let mode = match app.mode {
        AgentMode::Auto => "Auto",
        AgentMode::Plan => "Plan",
        AgentMode::Exec => "Exec",
    };
    let right_text = format!("{} | {}", app.model, mode);
    // Reserve exactly the width the right side needs (with a little padding).
    let right_len = right_text.width() + 2;
    let right_w = right_len.min(area.width.saturating_sub(8) as usize).max(8) as u16;
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Min(8), Constraint::Length(right_w)])
        .split(area);

    f.render_widget(Paragraph::new(left), chunks[0]);
    f.render_widget(
        Paragraph::new(right_text)
            .style(style)
            .alignment(Alignment::Right),
        chunks[1],
    );
}

fn block_render_key(block: &crate::layout::Block, width: u16, show_full: bool) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    block.kind.hash(&mut hasher);
    block.source_lines.hash(&mut hasher);
    width.hash(&mut hasher);
    show_full.hash(&mut hasher);
    hasher.finish()
}
