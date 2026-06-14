//! TUI rendering — modelled after ReOpenCode's transcript renderer.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Borders, Paragraph, Wrap};

use super::app::{MessageRole, Part, TuiApp};

const C_TEXT: Color = Color::White;
const C_MUTED: Color = Color::DarkGray;
const C_PRIMARY: Color = Color::Cyan;
const C_ACCENT: Color = Color::LightGreen;
const C_SUCCESS: Color = Color::Green;
const C_ERROR: Color = Color::Red;
const C_BORDER: Color = Color::Gray;
const C_TOOL_BG: Color = Color::Rgb(30, 30, 40);

pub fn render(f: &mut Frame, app: &TuiApp) {
    let area = f.area();

    // ── Layout: input at bottom, messages fill the rest ──
    let [messages_area, input_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(3)]).areas(area);

    // ── Messages ──
    let mut lines: Vec<Line> = Vec::new();
    for msg in &app.messages {
        render_message(msg, &mut lines, area.width.saturating_sub(4));
    }
    if app.agent_running {
        lines.push(Line::from(vec![
            Span::styled("  ⏳", Style::default().fg(Color::Yellow)),
            Span::styled(
                " agent working…",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::ITALIC),
            ),
        ]));
        lines.push(Line::from(""));
    }
    // Status on its own line at the very top
    lines.insert(
        0,
        Line::from(vec![Span::styled(
            format!("  {}", app.status),
            Style::default().fg(C_MUTED),
        )]),
    );

    let history = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(history, messages_area);

    // ── Input area ──
    let border_style = if app.agent_running {
        Style::default().fg(C_MUTED)
    } else {
        Style::default().fg(C_PRIMARY)
    };
    let input_text = if app.agent_running {
        "(waiting for agent…)".to_string()
    } else {
        format_input_with_cursor(&app.input, app.cursor)
    };
    let input_widget = Paragraph::new(input_text)
        .block(ratatui::widgets::Block::default().borders(Borders::TOP))
        .style(border_style);
    f.render_widget(input_widget, input_area);
}

fn format_input_with_cursor(text: &str, cursor: usize) -> String {
    if text.is_empty() {
        return "│".to_string();
    }
    let before = &text[..cursor.min(text.len())];
    let after = &text[cursor.min(text.len())..];
    format!("{before}│{after}")
}

// ── Message rendering (ReOpenCode-style) ──

fn render_message(msg: &super::app::Message, lines: &mut Vec<Line>, wrap_w: u16) {
    // Role badge line
    match &msg.role {
        MessageRole::User => {
            lines.push(Line::from(vec![Span::styled(
                "you",
                Style::default().fg(C_PRIMARY).add_modifier(Modifier::BOLD),
            )]));
        }
        MessageRole::Assistant { agent, model_short } => {
            let mut spans = vec![Span::styled(
                agent.clone(),
                Style::default().fg(C_ACCENT).add_modifier(Modifier::BOLD),
            )];
            if !model_short.is_empty() {
                spans.push(Span::styled(
                    format!(" · {}", model_short),
                    Style::default().fg(C_MUTED),
                ));
            }
            lines.push(Line::from(spans));
        }
    }
    lines.push(Line::from("")); // blank after badge

    // Parts (body with 2-char indent)
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
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(text.clone(), Style::default().fg(C_MUTED)),
                ]));
            }
        }
    }
    lines.push(Line::from("")); // blank after message
}

// ── Tool block rendering ──

fn render_tool_block(lines: &mut Vec<Line>, name: &str, success: bool, result: &str, width: u16) {
    let color = if success { C_SUCCESS } else { C_ERROR };
    let icon = if success { "✓" } else { "✗" };
    let w = width.saturating_sub(4) as usize;

    // Header line
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("┌ {icon} {name}"),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ),
    ]));

    // Result body (truncated, one line for short output)
    if !result.is_empty() {
        let trimmed = result.trim().to_string();
        if trimmed.len() <= w {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("│ ", Style::default().fg(C_BORDER)),
                Span::styled(trimmed, Style::default().fg(C_MUTED).bg(C_TOOL_BG)),
            ]));
        } else {
            for chunk in trimmed.as_bytes().chunks(w) {
                let s = String::from_utf8_lossy(chunk);
                lines.push(Line::from(vec![
                    Span::raw("  "),
                    Span::styled("│ ", Style::default().fg(C_BORDER)),
                    Span::styled(s.to_string(), Style::default().fg(C_MUTED).bg(C_TOOL_BG)),
                ]));
            }
        }
    }

    // Footer
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
        assert!(buffer_contains(buf, "coding"));
    }

    #[test]
    fn renders_tool_block() {
        let mut app = TuiApp::new("deepseek-chat".into());
        let mut am = Message::assistant("coding", "deepseek-chat");
        am.push_tool("write_file", true, "hello world");
        app.messages.push(am);

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let buf = terminal.backend().buffer();
        assert!(buffer_contains(buf, "write_file"));
        assert!(buffer_contains(buf, "hello world"));
    }
}
