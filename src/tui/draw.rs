use crate::tui::app::App;
use crate::types::AgentMode;
use crate::tui::{PULSE, SLASH_COMMANDS};
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::Paragraph;

// ═══ Draw ═══

pub fn draw(f: &mut Frame, app: &App, out_h: usize) {
    let area = f.area();
    let hint_page_start = app.hint_page * 8;
    let hint_page_end = (hint_page_start + 8).min(app.hints.len());
    let visible_hints: Vec<&(String, String)> = app.hints[hint_page_start..hint_page_end].iter().collect();
    let hint_count = visible_hints.len();
    let input_lines = app.input.split('\n').count().max(1).min(5);
    let input_h = (input_lines + 2) as u16;
    let bottom_h = (input_h as usize + hint_count + 1).min(area.height.saturating_sub(2) as usize) as u16;
    let chunks = Layout::default().direction(Direction::Vertical).constraints([Constraint::Min(1), Constraint::Length(bottom_h)]).split(area);
    draw_output(f, chunks[0], app, out_h.min(chunks[0].height as usize));
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
    for (i, (n, d)) in visible_hints.iter().take(hint_count).enumerate() {
        let selected = app.hint_selected == Some(hint_page_start + i);
        draw_hint_row(f, bottom[1 + i], n, d, selected);
    }
    draw_status(f, bottom[bottom.len() - 1], app);
}

fn draw_output(f: &mut Frame, area: Rect, app: &App, vis: usize) {
    use crate::layout::measure_blocks;
    use crate::markdown::MarkdownRenderer;
    use ratatui::widgets::Wrap;
    let total = app.output.len(); if total == 0 { return; }
    let vis = vis.min(area.height as usize);
    let start = if app.stick_to_bottom { total.saturating_sub(vis) } else { (app.scroll as usize).min(total.saturating_sub(1)) };
    let end = (start + vis).min(total);
    let blocks = measure_blocks(&app.output);
    let mut md = MarkdownRenderer::new(); md.tick_frame();
    let mut rendered: Vec<Line> = Vec::with_capacity(vis);
    let mut line_offset = 0usize;
    for block in &blocks {
        let block_end = line_offset + block.height;
        if block_end > start && line_offset < end {
            let block_lines = block.render(area.width, app.thinking_frame, &mut md, app.show_full_reasoning);
            for (li, bline) in block_lines.iter().enumerate() {
                let global_li = line_offset + li;
                if app.inside_window(global_li, vis) {
                    let mut line = bline.clone();
                    if let Some((sel_start, sel_end)) = app.selection { if global_li >= sel_start && global_li <= sel_end { line = line.style(Style::default().bg(Color::Rgb(60, 60, 70))); } }
                    rendered.push(line);
                }
            }
        }
        line_offset = block_end;
    }
    let content_h = rendered.len();
    let mut filled = rendered;
    filled.resize(filled.len().max(vis), Line::from(""));

    // Scrollbar on right edge
    if total > vis {
        let sb_h = area.height.saturating_sub(2);
        let thumb_h = ((vis as f32 / total as f32) * sb_h as f32).max(1.0) as u16;
        let thumb_y = if app.stick_to_bottom {
            sb_h.saturating_sub(thumb_h)
        } else {
            let progress = app.scroll as f32 / (total - vis).max(1) as f32;
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
            let before = &line[..cursor_col.min(line.len())];
            let ch = if cursor_col < line.len() { line[cursor_col..].chars().next().unwrap_or(' ') } else { ' ' };
            let after = if cursor_col < line.len() { &line[cursor_col + ch.len_utf8()..] } else { "" };
            row.push(Span::raw(before)); row.push(Span::styled(ch.to_string(), Style::default().bg(Color::White).fg(Color::Black))); row.push(Span::raw(after));
        } else { row.push(Span::raw(*line)); }
        spans.push(Line::from(row));
    }
    let block = RBlock::default().borders(Borders::ALL).border_type(BorderType::Rounded).border_style(Style::default().fg(Color::DarkGray));
    f.render_widget(Paragraph::new(Text::from(spans)).block(block).wrap(Wrap { trim: false }), area);
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

