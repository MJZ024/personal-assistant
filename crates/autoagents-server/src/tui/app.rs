//! Application state for the TUI — inspired by ReOpenCode's transcript model.

/// A part within a message (text, tool call, system notice).
#[derive(Clone)]
pub enum Part {
    Text {
        text: String,
    },
    Tool {
        tool_name: String,
        success: bool,
        result: String,
    },
    System {
        text: String,
    },
}

/// A single message in the conversation, modelled after ReOpenCode's
/// `TranscriptMessage`: one role + a list of parts. Tool calls and text
/// coexist in the same assistant message.
#[derive(Clone)]
pub struct Message {
    pub role: MessageRole,
    pub parts: Vec<Part>,
}

#[derive(Clone)]
pub enum MessageRole {
    User,
    /// `agent` is "coding" / "ops"; `model_short` is the model name.
    Assistant {
        agent: String,
        model_short: String,
    },
}

impl Message {
    pub fn user(text: &str) -> Self {
        Self {
            role: MessageRole::User,
            parts: vec![Part::Text {
                text: text.to_string(),
            }],
        }
    }
    pub fn assistant(agent: &str, model: &str) -> Self {
        Self {
            role: MessageRole::Assistant {
                agent: agent.to_string(),
                model_short: model.to_string(),
            },
            parts: Vec::new(),
        }
    }
    pub fn push_text(&mut self, text: &str) {
        self.parts.push(Part::Text {
            text: text.to_string(),
        });
    }
    pub fn push_tool(&mut self, tool_name: &str, success: bool, result: &str) {
        self.parts.push(Part::Tool {
            tool_name: tool_name.to_string(),
            success,
            result: result.to_string(),
        });
    }
}

/// Global application state.
pub struct TuiApp {
    pub messages: Vec<Message>,
    pub input: String,
    pub cursor: usize,
    pub status: String,
    pub agent_running: bool,
    pub model_desc: String,
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

    pub fn submit_input(&mut self) -> String {
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return String::new();
        }
        self.messages.push(Message::user(&text));
        self.input.clear();
        self.cursor = 0;
        text
    }

    /// Start a new assistant message (for streaming/agent runs).
    pub fn begin_assistant(&mut self, agent: &str) -> &mut Message {
        let short_model = self
            .model_desc
            .split(" · ")
            .last()
            .unwrap_or("?")
            .to_string();
        self.messages.push(Message::assistant(agent, &short_model));
        self.messages.last_mut().unwrap()
    }

    /// Get the last assistant message (for appending parts while agent runs).
    pub fn last_assistant_mut(&mut self) -> Option<&mut Message> {
        self.messages
            .iter_mut()
            .rev()
            .find(|m| matches!(m.role, MessageRole::Assistant { .. }))
    }

    // ── Input editing helpers ──

    pub fn input_char(&mut self, c: char) {
        self.input.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

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

    pub fn cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.input[..self.cursor]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    pub fn cursor_right(&mut self) {
        if self.cursor < self.input.len() {
            self.cursor = self.input[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.input.len());
        }
    }

    pub fn cursor_home(&mut self) {
        self.cursor = 0;
    }
    pub fn cursor_end(&mut self) {
        self.cursor = self.input.len();
    }
}
