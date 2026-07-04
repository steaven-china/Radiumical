//! Renders the agent-role selection panel inside a [`PanelSlot`].

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};
use ratatui::Frame;

use radiumical_core::agent_pool::AgentDef;

use crate::panel::PanelSlot;

/// Render the agent roles list, highlighting the currently active role.
pub fn render_agents_panel(
    f: &mut Frame,
    slot: &PanelSlot,
    agents: &[AgentDef],
    current_role: &str,
) {
    let inner = Rect {
        x: slot.rect.x + 1,
        y: slot.rect.y + 1,
        width: slot.rect.width.saturating_sub(2),
        height: slot.rect.height.saturating_sub(2),
    };

    if agents.is_empty() {
        let empty = Paragraph::new("  No agents found.\n  Place .md files in ~/.radi/agents/")
            .style(Style::default().fg(Color::DarkGray))
            .wrap(Wrap { trim: false });
        f.render_widget(empty, inner);
        return;
    }

    let mut lines: Vec<Line> = Vec::new();

    let title_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    lines.push(Line::from(vec![Span::styled("  Agent Roles", title_style)]));
    lines.push(Line::from(""));

    for agent in agents {
        let is_active = agent.name == current_role;
        let marker = if is_active { "▶" } else { " " };
        let marker_fg = if is_active {
            Color::Yellow
        } else {
            Color::DarkGray
        };

        let mode_str = match agent.mode {
            radiumical_core::agent_pool::AgentRoleMode::Auto => "auto",
            radiumical_core::agent_pool::AgentRoleMode::Plan => "plan",
            radiumical_core::agent_pool::AgentRoleMode::Exec => "exec",
        };

        let tools_str = if agent.tools.is_empty() {
            "all tools".to_string()
        } else {
            agent.tools.join(", ")
        };

        let name_fg = if is_active {
            Color::White
        } else {
            Color::Rgb(160, 160, 170)
        };
        let mode_fg = Color::Rgb(100, 100, 110);
        let tools_fg = Color::Rgb(90, 90, 100);

        lines.push(Line::from(vec![
            Span::styled(format!(" {marker} "), Style::default().fg(marker_fg)),
            Span::styled(
                format!("{:<12}", agent.name),
                Style::default().fg(name_fg).add_modifier(if is_active {
                    Modifier::BOLD
                } else {
                    Modifier::empty()
                }),
            ),
            Span::styled(format!("{:<5}", mode_str), Style::default().fg(mode_fg)),
            Span::styled(tools_str, Style::default().fg(tools_fg)),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "  /agents <name> to switch",
        Style::default().fg(Color::DarkGray),
    )]));

    let para = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(para, inner);
}
