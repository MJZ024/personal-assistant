//! Application state for the TUI.

/// A chat message rendered in the conversation history.
#[derive(Clone)]
pub enum Message {
    /// A message from the user.
    User { text: String },
    /// A completed response from the assistant.
    Agent { text: String },
    /// A tool call result (inline in the conversation).
    Tool {
        tool_name: String,
        success: bool,
        result_summary: String,
    },
    /// A system notification (errors, info, etc.).
    System { text: String },
}

/// Global application state shared between the event loop and the renderer.
pub struct TuiApp {
    /// Ordered chat messages (oldest first).
    pub messages: Vec<Message>,
    /// Current input text (simple single-line edit buffer).
    pub input: String,
    /// Cursor position in the input buffer (byte index).
    pub cursor: usize,
    /// Top status bar text.
    pub status: String,
    /// Whether an agent is currently running (disables input, shows spinner).
    pub agent_running: bool,
    /// Model description for the status bar.
    pub model_desc: String,
    /// Whether the terminal size changed.
    pub needs_resize: bool,
}

impl TuiApp {
    pub fn new(model_desc: String) -> Self {
        Self {
            messages: Vec::new(),
            input: String::new(),
            cursor: 0,
            status: format!("{} | idle", model_desc),
            agent_running: false,
            model_desc,
            needs_resize: false,
        }
    }

    /// Push a user message and clear the input.
    pub fn submit_input(&mut self) -> String {
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return String::new();
        }
        self.messages.push(Message::User { text: text.clone() });
        self.input.clear();
        self.cursor = 0;
        text
    }

    /// Push a system notification.
    pub fn push_system(&mut self, text: &str) {
        self.messages.push(Message::System {
            text: text.to_string(),
        });
    }

    /// Push an agent response.
    pub fn push_agent(&mut self, text: String) {
        self.messages.push(Message::Agent { text });
    }

    /// Push a tool call result.
    pub fn push_tool(&mut self, tool_name: &str, success: bool, result_summary: &str) {
        self.messages.push(Message::Tool {
            tool_name: tool_name.to_string(),
            success,
            result_summary: result_summary.to_string(),
        });
    }

    /// Handle a character being typed.
    pub fn input_char(&mut self, c: char) {
        self.input.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Handle backspace.
    pub fn input_backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.input[..self.cursor]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.input.drain(prev..self.cursor);
            self.cursor = prev;
        }
    }

    /// Handle delete.
    pub fn input_delete(&mut self) {
        if self.cursor < self.input.len() {
            let next = self.input[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.input.len());
            self.input.drain(self.cursor..next);
        }
    }

    /// Move cursor left.
    pub fn cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.input[..self.cursor]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    /// Move cursor right.
    pub fn cursor_right(&mut self) {
        if self.cursor < self.input.len() {
            self.cursor = self.input[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.input.len());
        }
    }

    /// Move cursor to start.
    pub fn cursor_home(&mut self) {
        self.cursor = 0;
    }

    /// Move cursor to end.
    pub fn cursor_end(&mut self) {
        self.cursor = self.input.len();
    }
}
