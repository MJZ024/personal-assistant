//! Minimal TUI rendering — Claude Code style.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::app::{MessageRole, Part, TuiApp};

const C_TEXT: Color = Color::White;
const C_MUTED: Color = Color::Gray;
const C_DIM: Color = Color::DarkGray;
const C_PRIMARY: Color = Color::Cyan;
const C_ACCENT: Color = Color::LightGreen;
const C_SUCCESS: Color = Color::Green;
const C_ERROR: Color = Color::Red;
const C_BORDER: Color = Color::Gray;
const C_TOOL_BG: Color = Color::Rgb(30, 30, 40);

pub fn render(f: &mut Frame, app: &TuiApp) {
    let area = f.area();
    let max_w = area.width.saturating_sub(4) as usize;

    // ── Layout ──
    let [body_area, input_area, footer_area] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    // ── Messages body ──
    let mut lines: Vec<Line> = Vec::new();
    // Header: model line (dim, right-aligned feel)
    let header = if app.agent_running {
        format!(
            "{}  ●  ⏳ {}",
            app.model_desc,
            app.status.split(" | ").last().unwrap_or("running…")
        )
    } else {
        format!("{}  ●", app.model_desc)
    };
    lines.push(Line::from(vec![Span::styled(
        format!("  {header}"),
        Style::default().fg(C_DIM),
    )]));
    lines.push(Line::from(""));

    for msg in &app.messages {
        render_message(msg, &mut lines, area.width.saturating_sub(4));
    }

    let total = lines.len().saturating_sub(1);
    let max_scroll = total.saturating_sub(body_area.height.saturating_sub(1) as usize);
    let off = app.scroll_offset.min(max_scroll);
    let history = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((off as u16, 0));
    f.render_widget(history, body_area);

    // ── Separator + input ──
    let input_text = if app.agent_running {
        "  ⏳ waiting…".to_string()
    } else {
        let c = app.cursor.min(app.input.len());
        format!(
            "❯ {}{}│{}",
            &app.input[..c],
            if app.input.is_empty() { "" } else { "" },
            &app.input[c..]
        )
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![Span::styled(
                "─".repeat(max_w),
                Style::default().fg(C_DIM),
            )]),
            Line::from(vec![Span::styled(
                input_text,
                Style::default().fg(C_PRIMARY).add_modifier(Modifier::BOLD),
            )]),
        ]),
        input_area,
    );

    // ── Footer ──
    let footer = format!("  ? help  esc quit  pgup/pgdn scroll");
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            footer,
            Style::default().fg(C_DIM),
        )])),
        footer_area,
    );
}

// ── Message rendering ──

fn render_message(msg: &super::app::Message, lines: &mut Vec<Line>, wrap_w: u16) {
    match &msg.role {
        MessageRole::User => {
            lines.push(Line::from(vec![Span::styled(
                "you",
                Style::default().fg(C_PRIMARY).add_modifier(Modifier::BOLD),
            )]));
        }
        MessageRole::Assistant { agent, model_short } => {
            let label = if model_short.is_empty() {
                agent.clone()
            } else {
                format!("{} · {}", agent, model_short)
            };
            lines.push(Line::from(vec![Span::styled(
                label,
                Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD),
            )]));
        }
    }
    lines.push(Line::from(""));

    for part in &msg.parts {
        match part {
            Part::Text { text } => {
                if text.trim().is_empty() {
                    continue;
                }
                for line_text in text.lines() {
                    let wrapped = textwrap::wrap(line_text, wrap_w as usize);
                    for wline in wrapped {
                        lines.push(Line::from(vec![
                            Span::raw("  "),
                            Span::styled(wline.into_owned(), Style::default().fg(C_TEXT)),
                        ]));
                    }
                }
            }
            Part::Tool {
                tool_name,
                success,
                result,
            } => {
                render_tool_block(lines, tool_name, *success, result, wrap_w);
            }
            Part::System { text } => {
                lines.push(Line::from(vec![Span::styled(
                    format!("  {}", text),
                    Style::default().fg(C_MUTED),
                )]));
            }
        }
    }
    lines.push(Line::from(""));
}

fn render_tool_block(lines: &mut Vec<Line>, name: &str, success: bool, result: &str, width: u16) {
    let color = if success { C_SUCCESS } else { C_ERROR };
    let icon = if success { "✓" } else { "✗" };
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("┌ {icon} {name}"),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ]));
    if !result.is_empty() {
        let trimmed = result.trim().to_string();
        let w = width.saturating_sub(4) as usize;
        if trimmed.len() <= w {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("│ ", Style::default().fg(C_BORDER)),
                Span::styled(trimmed, Style::default().fg(C_MUTED).bg(C_TOOL_BG)),
            ]));
        }
    }
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("└─", Style::default().fg(C_BORDER)),
    ]));
    lines.push(Line::from(""));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tui::app::Message;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    fn buffer_contains(buffer: &ratatui::buffer::Buffer, needle: &str) -> bool {
        for y in 0..buffer.area.height {
            let row: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            if row.contains(needle) {
                return true;
            }
        }
        false
    }

    #[test]
    fn renders_user_and_agent() {
        let mut app = TuiApp::new("deepseek-chat".into());
        app.messages.push(Message::user("hello"));
        let mut am = Message::assistant("coding", "deepseek-chat");
        am.push_text("hi there");
        app.messages.push(am);
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "hello"));
        assert!(buffer_contains(buf, "hi there"));
    }
}
