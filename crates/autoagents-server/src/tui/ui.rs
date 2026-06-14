//! Pixel-level Claude Code clone.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use super::app::{MessageRole, Part, TuiApp};

const C_WHITE: Color = Color::White;
const C_DIM: Color = Color::DarkGray;
const C_MUTED: Color = Color::Gray;
const C_CYAN: Color = Color::Cyan;
const C_GREEN: Color = Color::LightGreen;
const C_RED: Color = Color::Red;
const C_BORDER: Color = Color::DarkGray;
const C_TOOL_BG: Color = Color::Rgb(30, 30, 40);

pub fn render(f: &mut Frame, app: &TuiApp) {
    let area = f.area();
    let w = area.width as usize;
    let sep = "─".repeat(w.saturating_sub(2));

    // ── Layout ──
    let [body_area, sep1_area, input_area, sep2_area, footer_area] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(area);

    // ── Body ──
    let mut lines: Vec<Line> = Vec::new();
    // Header: project name + model (simple, fits one line)
    let header = format!("  personal-assistant        {}  ●", app.model_desc);
    lines.push(Line::from(vec![Span::styled(
        header,
        Style::default().fg(C_DIM),
    )]));
    lines.push(Line::from(""));

    // Messages
    for msg in &app.messages {
        render_message(msg, &mut lines, area.width.saturating_sub(4));
    }

    let total = lines.len().saturating_sub(1);
    let vis = body_area.height.saturating_sub(1) as usize;
    let max_scroll = total.saturating_sub(vis);
    let off = app.scroll_offset.min(max_scroll);
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((off as u16, 0)),
        body_area,
    );

    // ── Separator 1 ──
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            sep.clone(),
            Style::default().fg(C_DIM),
        )])),
        sep1_area,
    );

    // ── Input ──
    let prompt = if app.agent_running {
        "❯ ⏳ waiting…".to_string()
    } else if app.input.is_empty() {
        "❯ ".to_string()
    } else {
        let c = app.cursor.min(app.input.len());
        format!("❯ {}{}│{}", &app.input[..c], "", &app.input[c..])
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            prompt,
            Style::default().fg(C_WHITE),
        )])),
        input_area,
    );

    // ── Separator 2 ──
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            sep,
            Style::default().fg(C_DIM),
        )])),
        sep2_area,
    );

    // ── Footer ──
    let left = "  enter send  esc quit  pgup/pgdn scroll";
    let right = if app.agent_running { "⏳" } else { "◈" };
    let padding = w.saturating_sub(left.len() + right.len() + 4);
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(left, Style::default().fg(C_DIM)),
            Span::styled(" ".repeat(padding), Style::default().fg(C_DIM)),
            Span::styled(right, Style::default().fg(C_DIM)),
        ])),
        footer_area,
    );
}

// ── Message rendering ──

fn render_message(msg: &super::app::Message, lines: &mut Vec<Line>, wrap_w: u16) {
    match &msg.role {
        MessageRole::User => {
            lines.push(Line::from(vec![Span::styled(
                "  you",
                Style::default().fg(C_CYAN).add_modifier(Modifier::BOLD),
            )]));
        }
        MessageRole::Assistant { agent, model_short } => {
            let label = if model_short.is_empty() {
                agent.clone()
            } else {
                format!("{} · {}", agent, model_short)
            };
            lines.push(Line::from(vec![Span::styled(
                format!("  {}", label),
                Style::default().fg(C_GREEN).add_modifier(Modifier::BOLD),
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
                    for wline in textwrap::wrap(line_text, wrap_w as usize) {
                        lines.push(Line::from(vec![
                            Span::raw("  "),
                            Span::styled(wline.into_owned(), Style::default().fg(C_WHITE)),
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
                    format!("  ⎿  {}", text),
                    Style::default().fg(C_MUTED),
                )]));
            }
        }
    }
    lines.push(Line::from(""));
}

fn render_tool_block(lines: &mut Vec<Line>, name: &str, success: bool, result: &str, width: u16) {
    let c = if success { C_GREEN } else { C_RED };
    let icon = if success { "✓" } else { "✗" };
    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled(
            format!("┌ {icon} {name}"),
            Style::default().fg(c).add_modifier(Modifier::BOLD),
        ),
    ]));
    if !result.is_empty() {
        let t = result.trim().to_string();
        let mw = width.saturating_sub(4) as usize;
        if t.len() <= mw {
            lines.push(Line::from(vec![
                Span::raw("  "),
                Span::styled("│ ", Style::default().fg(C_BORDER)),
                Span::styled(t, Style::default().fg(C_MUTED).bg(C_TOOL_BG)),
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
        let rows: Vec<String> = (0..buf.area.height)
            .map(|y| (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect())
            .collect();
        let all = rows.join("\n");
        assert!(all.contains("hello"));
        assert!(all.contains("hi there"));
    }
}
