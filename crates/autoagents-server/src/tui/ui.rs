//! TUI rendering — converts [`crate::tui::app::TuiApp`] into widgets.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use super::app::{Message, TuiApp};

/// Render the full TUI frame.
pub fn render(f: &mut Frame, app: &TuiApp) {
    let area = f.area();

    // ── Layout ──
    let [status_area, messages_area, input_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .areas(area);

    // ── Status bar ──
    let status = Paragraph::new(Line::from(vec![Span::styled(
        &app.status,
        Style::default()
            .fg(Color::White)
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    )]));
    f.render_widget(status, status_area);

    // ── Chat history ──
    let mut lines: Vec<Line> = app.messages.iter().map(render_message).collect();
    if app.agent_running {
        lines.push(Line::from(vec![
            Span::styled("  ⏳ ", Style::default().fg(Color::Yellow)),
            Span::styled(
                "agent working…",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::ITALIC),
            ),
        ]));
    }
    let history = Paragraph::new(lines)
        .block(Block::default().borders(Borders::NONE))
        .wrap(Wrap { trim: false });
    f.render_widget(history, messages_area);

    // ── Input area ──
    let border_style = if app.agent_running {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::Cyan)
    };
    let input_text = if app.agent_running {
        "(waiting for agent…)".to_string()
    } else {
        format_input_with_cursor(&app.input, app.cursor)
    };
    let input_widget = Paragraph::new(input_text)
        .block(Block::default().borders(Borders::TOP).title(" > "))
        .style(border_style);
    f.render_widget(input_widget, input_area);
}

/// Build a display string with a cursor indicator at `pos`.
fn format_input_with_cursor(text: &str, cursor: usize) -> String {
    if text.is_empty() {
        return "│".to_string();
    }
    let before = &text[..cursor.min(text.len())];
    let after = &text[cursor.min(text.len())..];
    format!("{before}│{after}")
}

fn render_message(msg: &Message) -> Line<'static> {
    match msg {
        Message::User { text } => Line::from(vec![
            Span::styled(
                "  [You] ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(text.clone()),
        ]),
        Message::Agent { text } => Line::from(vec![
            Span::styled(
                "  [Agent] ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(text.clone()),
        ]),
        Message::System { text } => Line::from(vec![
            Span::styled("  [··] ", Style::default().fg(Color::DarkGray)),
            Span::styled(text.clone(), Style::default().fg(Color::DarkGray)),
        ]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn renders_user_and_agent_messages() {
        let mut app = TuiApp::new("deepseek-chat".into());
        app.messages.push(Message::User {
            text: "hello".into(),
        });
        app.messages.push(Message::Agent {
            text: "hi there".into(),
        });

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let buffer = terminal.backend().buffer();
        assert!(buffer_contains(buffer, "[You]"), "missing [You]");
        assert!(buffer_contains(buffer, "hello"), "missing user text");
        assert!(buffer_contains(buffer, "[Agent]"), "missing [Agent]");
        assert!(buffer_contains(buffer, "hi there"), "missing agent text");
    }

    #[test]
    fn shows_spinner_when_agent_running() {
        let mut app = TuiApp::new("deepseek-chat".into());
        app.agent_running = true;

        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let buffer = terminal.backend().buffer();

        assert!(buffer_contains(buffer, "agent working"), "missing spinner");
    }

    #[test]
    fn status_bar_shows_model() {
        let app = TuiApp::new("deepseek-chat".into());
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| render(f, &app)).unwrap();
        let buffer = terminal.backend().buffer();

        assert!(
            buffer_contains(buffer, "deepseek-chat"),
            "missing model in status"
        );
    }
}
