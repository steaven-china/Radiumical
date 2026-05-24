use crate::tui::app::App;
use crate::types::AgentMode;
use crate::tui::{PULSE, SLASH_COMMANDS};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block as RBlock, BorderType, Borders, Paragraph};

// ═══ Draw ═══

pub fn draw(f: &mut Frame, app: &mut App, _out_h: usize) {
    let area = f.area();
    let hint_page_start = app.hint_page * 8;
    let hint_page_end = (hint_page_start + 8).min(app.hints.len());
    let visible_hints: Vec<&(String, String)> = app.hints[hint_page_start..hint_page_end].iter().collect();
    let hint_count = visible_hints.len();
    let input_lines = app.input.split('\n').count().max(1).min(5);
    let input_h = (input_lines + 2) as u16;
    let bottom_h = (input_h as usize + hint_count + 1).min(area.height.saturating_sub(2) as usize) as u16;
    let chunks = Layout::default().direction(Direction::Vertical).constraints([Constraint::Min(1), Constraint::Length(bottom_h)]).split(area);
    draw_output(f, chunks[0], app, chunks[0].height as usize);
    // Help overlay on welcome screen (bottom-right of output area)
    if app.welcome && app.show_help_overlay && chunks[0].height > 12 {
        let mut stack = crate::board::BoardStack::new();
        let lines = draw_help_overlay_lines();
        app.help_board.render_stacked(f, chunks[0], Text::from(lines), &mut stack);
    }
    if app.show_model_picker {
        let mut stack = crate::board::BoardStack::new();
        if app.welcome && app.show_help_overlay && chunks[0].height > 12 {
            // Push help board first so model picker stacks above it
            let _ = stack.push(crate::board::Corner::BottomRight, app.help_board.w, app.help_board.h, chunks[0]);
        }
        let models: Vec<Line> = app.available_models.iter().map(|m| {
            let prefix = if m == &app.model { "* " } else { "  " };
            Line::from(Span::raw(format!("{prefix}{m}")))
        }).collect();
        app.model_board.render_stacked(f, chunks[0], Text::from(models), &mut stack);
    }
    let bottom = Layout::default().direction(Direction::Vertical).constraints([Constraint::Length(input_h)].into_iter().chain(std::iter::repeat(Constraint::Length(1)).take(hint_count)).chain(std::iter::once(Constraint::Length(1))).collect::<Vec<_>>()).split(chunks[1]);
    draw_input(f, bottom[0], app);
    // Render toasts at top-center (stacked vertically)
    let mut toast_y = 0u16;
    for toast in &app.toasts {
        if !toast.is_expired() {
            let w = (toast.message.len() as u16 + 4).min(area.width - 4);
            let x = (area.width - w) / 2;
            let r = Rect { x: area.x + x, y: area.y + toast_y, width: w, height: 3 };
            toast_y += 3;
            let color = match toast.level {
                crate::board::ToastLevel::Info => Color::Cyan,
                crate::board::ToastLevel::Warn => Color::Yellow,
                crate::board::ToastLevel::Error => Color::Red,
            };
            let block = RBlock::default().borders(Borders::ALL).border_type(BorderType::Rounded)
                .border_style(Style::default().fg(color));
            f.render_widget(Paragraph::new(toast.message.as_str()).block(block), r);
        }
    }
    app.toasts.retain(|t| !t.is_expired());
    // Progress bar at top-right
    app.progress.render(f, area);
    // Perf overlay at top-right
    if app.perf_visible { draw_perf_overlay(f, area, app); }
    // Dashboard (// toggle)
    if app.dashboard.visible {
        app.dashboard.render(f, chunks[0]);
    }
    // Session list popup
    if app.session_list_visible { app.session_list.render(f, chunks[0]); }
    // Render confirm dialog
    app.confirm.render(f, area);
    for (i, (n, d)) in visible_hints.iter().take(hint_count).enumerate() {
        let selected = app.hint_selected == Some(hint_page_start + i);
        draw_hint_row(f, bottom[1 + i], n, d, selected);
    }
    draw_status(f, bottom[bottom.len() - 1], app);
}

fn draw_output(f: &mut Frame, area: Rect, app: &App, _vis: usize) {
    use crate::layout::measure_blocks;
    use crate::markdown::MarkdownRenderer;
    use ratatui::widgets::Wrap;
    let total = app.output.len(); if total == 0 { return; }
    let vis = (area.height as usize).saturating_sub(2).min(_vis);
    let avail_w = area.width.saturating_sub(2) as usize; // leave margin for scrollbar

    let blocks = measure_blocks(&app.output);
    let mut md = MarkdownRenderer::new(); md.tick_frame();

    // ── Build flat list of pre-wrapped visual lines ──
    let mut visual_lines: Vec<(usize, Line)> = Vec::new(); // (logical_line_index, line)
    let mut logical_to_visual: Vec<usize> = Vec::new(); // logical_line -> first visual line
    let mut line_offset = 0usize;

    for block in &blocks {
        let block_lines = block.render(area.width, app.thinking_frame, &mut md, app.show_full_reasoning);
        for (li, bline) in block_lines.iter().enumerate() {
            let logical_li = line_offset + li;
            // Pre-wrap each logical line to fit within avail_w
            let wrapped = wrap_line_to_width(bline, avail_w);
            if logical_to_visual.len() <= logical_li {
                logical_to_visual.resize(logical_li + 1, visual_lines.len());
            }
            logical_to_visual[logical_li] = visual_lines.len();
            for wline in wrapped {
                visual_lines.push((logical_li, wline));
            }
        }
        line_offset += block.height;
    }

    let total_visual = visual_lines.len();
    // Clamp vis to actual visual line count
    let vis = vis.min(total_visual.max(1));

    // Compute start visual line based on stick_to_bottom / scroll
    let start_visual = if app.stick_to_bottom {
        total_visual.saturating_sub(vis)
    } else {
        // Convert logical scroll to visual scroll proportionally
        let max_logical = total.saturating_sub(1).max(1) as f32;
        let frac = (app.scroll as f32 / max_logical).clamp(0.0, 1.0);
        ((frac * (total_visual.saturating_sub(vis)) as f32) as usize).min(total_visual.saturating_sub(1))
    };
    let end_visual = (start_visual + vis).min(total_visual);

    // Build the rendered slice
    let mut rendered: Vec<Line> = Vec::with_capacity(vis);
    for i in start_visual..end_visual {
        let (logical_li, line) = &visual_lines[i];
        let mut display_line = line.clone();
        if let Some((sel_start, sel_end)) = app.selection {
            if *logical_li >= sel_start && *logical_li <= sel_end {
                display_line = display_line.style(Style::default().bg(Color::Rgb(60, 60, 70)));
            }
        }
        rendered.push(display_line);
    }
    let content_h = rendered.len();
    let mut filled = rendered;
    filled.resize(filled.len().max(vis), Line::from(""));

    // ── Scrollbar (visual-line aware) ──
    if total_visual > vis {
        let sb_h = area.height.saturating_sub(1);
        let thumb_h = ((vis as f32 / total_visual as f32) * sb_h as f32).max(1.0) as u16;
        let thumb_y = if app.stick_to_bottom {
            sb_h.saturating_sub(thumb_h)
        } else {
            let progress = start_visual as f32 / (total_visual - vis).max(1) as f32;
            (progress * (sb_h - thumb_h) as f32) as u16
        };
        let sb_style = Style::default().fg(Color::Rgb(60, 60, 70));
        for i in 0..sb_h {
            let ch = if i >= thumb_y && i < thumb_y + thumb_h { '█' } else { '│' };
            f.render_widget(Paragraph::new(ch.to_string()).style(sb_style), Rect { x: area.x + area.width - 1, y: area.y + 1 + i, width: 1, height: 1 });
        }
    }

    if app.welcome && content_h < vis && app.scroll <= 0.0 && !filled.is_empty() && filled.iter().any(|l| l.width() > 0) {
        let pad_top = (vis - content_h) / 2;
        let max_w = filled.iter().map(|l| l.width()).max().unwrap_or(0) as u16;
        let pad_left = (area.width.saturating_sub(max_w) / 2) as usize;
        let mut padded: Vec<Line> = Vec::new(); padded.resize(pad_top, Line::from(""));
        for line in filled { let line_w = line.width() as u16; let extra = (max_w.saturating_sub(line_w) / 2) as usize; let mut spans = vec![Span::raw(" ".repeat(pad_left + extra))]; spans.extend(line.spans.into_iter()); padded.push(Line::from(spans)); }
        f.render_widget(Paragraph::new(Text::from(padded)).wrap(Wrap { trim: false }), area);
    } else { f.render_widget(Paragraph::new(Text::from(filled)).wrap(Wrap { trim: false }), area); }
}

/// Clone a borrowed Line into an owned Line<'static>.
fn clone_line_to_static(line: &Line) -> Line<'static> {
    let spans: Vec<Span<'static>> = line.spans.iter().map(|s| {
        Span::styled(s.content.to_string(), s.style)
    }).collect();
    Line::from(spans)
}

/// Split a Line into multiple Lines, each ≤ max_width display columns.
/// Preserves span styling across wrapped segments.
fn wrap_line_to_width(line: &Line, max_width: usize) -> Vec<Line<'static>> {
    if max_width == 0 { return vec![clone_line_to_static(line)]; }
    if line.width() <= max_width { return vec![clone_line_to_static(line)]; }

    let mut result: Vec<Line<'static>> = Vec::new();
    let mut cur_spans: Vec<Span<'static>> = Vec::new();
    let mut cur_w: usize = 0;

    for span in &line.spans {
        let text = span.content.as_ref();
        let style = span.style;

        // Split this span's text into chunks that fit
        let mut remaining = text;
        while !remaining.is_empty() {
            if cur_w >= max_width {
                result.push(Line::from(std::mem::take(&mut cur_spans)));
                cur_w = 0;
            }
            let room = max_width - cur_w;
            // Find longest prefix of `remaining` that fits in `room`
            let (chunk, rest) = split_str_at_width(remaining, room);
            if chunk.is_empty() {
                // Single char wider than room -> start new line
                if cur_w > 0 {
                    result.push(Line::from(std::mem::take(&mut cur_spans)));
                    cur_w = 0;
                }
                let (force_chunk, force_rest) = split_str_at_width(remaining, max_width);
                let force = if force_chunk.is_empty() {
                    // At least one char
                    if let Some(c) = remaining.chars().next() {
                        let c_str = &remaining[..c.len_utf8()];
                        remaining = &remaining[c.len_utf8()..];
                        c_str
                    } else { remaining = ""; "" }
                } else { remaining = force_rest; force_chunk };
                if !force.is_empty() {
                    let w = Line::from(force.to_string()).width();
                    cur_spans.push(Span::styled(force.to_string(), style));
                    cur_w += w;
                }
            } else {
                let w = Line::from(chunk.to_string()).width();
                cur_spans.push(Span::styled(chunk.to_string(), style));
                cur_w += w;
                remaining = rest;
            }
        }
    }

    if !cur_spans.is_empty() {
        result.push(Line::from(cur_spans));
    }

    if result.is_empty() {
        result.push(Line::from(""));
    }
    result
}

/// Split a &str at the longest prefix whose display width ≤ max_width.
/// Returns (prefix, suffix). Guarantees char-boundary safety.
fn split_str_at_width(s: &str, max_width: usize) -> (&str, &str) {
    if s.is_empty() || max_width == 0 { return ("", s); }
    // Binary search for the longest fitting prefix
    let mut lo = 0;
    let mut hi = s.len();
    while lo < hi {
        let mid = (lo + hi + 1) / 2;
        // Find nearest char boundary ≤ mid
        let mut bound = mid;
        while bound > 0 && !s.is_char_boundary(bound) { bound -= 1; }
        if bound == 0 { lo = mid; break; } // mid was in first char
        let candidate = &s[..bound];
        let w = Line::from(candidate.to_string()).width();
        if w <= max_width { lo = bound; } else { hi = bound.saturating_sub(1); }
    }
    let split_at = if lo > 0 { lo } else {
        // Ensure we take at least one char if possible
        s.chars().next().map(|c| c.len_utf8()).unwrap_or(0)
    };
    // Security: ensure char boundary
    let split_at = if s.is_char_boundary(split_at) { split_at } else {
        let mut b = split_at;
        while b > 0 && !s.is_char_boundary(b) { b -= 1; }
        b
    };
    (&s[..split_at], &s[split_at..])
}

fn draw_input(f: &mut Frame, area: Rect, app: &App) {
    use ratatui::widgets::{Block as RBlock, BorderType, Borders, Wrap};
    let lines: Vec<&str> = app.input.split('\n').collect();
    let cursor_line = app.input[..app.cursor].chars().filter(|c| *c == '\n').count().min(lines.len().saturating_sub(1));
    let cursor_col = app.cursor - app.input[..app.cursor].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let mut spans: Vec<Line> = Vec::new();
    for (li, line) in lines.iter().enumerate() {
        let mut row: Vec<Span> = Vec::new();
        row.push(Span::styled(if li == 0 { "> " } else { "  " }, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)));
        if li == cursor_line {
            if cursor_col < line.len() {
                // Cursor within line: highlight the character at cursor
                let before = &line[..cursor_col];
                let ch = line[cursor_col..].chars().next().unwrap_or(' ');
                let after = &line[cursor_col + ch.len_utf8()..];
                row.push(Span::raw(before));
                row.push(Span::styled(ch.to_string(), Style::default().bg(Color::White).fg(Color::Black)));
                row.push(Span::raw(after));
            } else if !line.is_empty() {
                // Cursor at end of non-empty line: highlight last char as block cursor
                let last_char_start = line.char_indices().next_back().map(|(i, _)| i).unwrap_or(0);
                let before = &line[..last_char_start];
                let last_ch = &line[last_char_start..];
                row.push(Span::raw(before));
                row.push(Span::styled(last_ch.to_string(), Style::default().bg(Color::White).fg(Color::Black)));
            } else {
                // Cursor on empty line: show highlighted space as cursor placeholder
                row.push(Span::raw(""));
                row.push(Span::styled(" ".to_string(), Style::default().bg(Color::White).fg(Color::Black)));
            }
        } else { row.push(Span::raw(*line)); }
        spans.push(Line::from(row));
    }
    let block = RBlock::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(Color::DarkGray));
    f.render_widget(Paragraph::new(Text::from(spans)).block(block).wrap(Wrap { trim: false }), area);
}



fn draw_perf_overlay(f: &mut Frame, area: Rect, _app: &App) {
    let report = crate::perf::report();
    let w = (report.len() + 4).min(area.width as usize - 4) as u16;
    let r = Rect { x: area.x + area.width - w - 2, y: area.y + 1, width: w, height: 1 };
    f.render_widget(Paragraph::new(report).style(Style::default().fg(Color::Rgb(100, 100, 110)).bg(Color::Rgb(15, 15, 20))), r);
}

fn draw_hint_row(f: &mut Frame, area: Rect, name: &str, desc: &str, selected: bool) {
    let bg = if selected { Style::default().bg(Color::Rgb(50, 50, 60)) } else { Style::default() };
    let line = Line::from(vec![
        Span::styled(format!("  {name}"), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD).add_modifier(if selected { Modifier::UNDERLINED } else { Modifier::empty() })),
        Span::styled(format!(" — {desc}"), Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(line).style(bg), area);
}

fn draw_help_overlay_lines() -> Vec<Line<'static>> {
    let max_w = SLASH_COMMANDS.iter().map(|(n,_)| n.len()).max().unwrap_or(10);
    SLASH_COMMANDS.iter().map(|(n,d)| {
        Line::from(vec![Span::styled(format!("{n:<w$}", w = max_w), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)), Span::raw(format!("  {d}"))])
    }).collect()
}

fn draw_status(f: &mut Frame, area: Rect, app: &App) {
    use ratatui::layout::Alignment;
    let chunks = Layout::default().direction(Direction::Horizontal).constraints([Constraint::Percentage(70), Constraint::Percentage(30)]).split(area);
    let style = Style::default().fg(Color::Rgb(130, 130, 130));
    let left = if app.thinking { let bar = PULSE[app.thinking_frame % PULSE.len()]; format!(" {} thinking {}s", bar, app.thinking_elapsed) } else { "Ready".into() };
    f.render_widget(Paragraph::new(left).style(style), chunks[0]);
    let mode = match app.mode { AgentMode::Auto => "Auto", AgentMode::Plan => "Plan", AgentMode::Exec => "Exec" };
    f.render_widget(Paragraph::new(format!("{} | {} ", app.model, mode)).style(style).alignment(Alignment::Right), chunks[1]);
}

