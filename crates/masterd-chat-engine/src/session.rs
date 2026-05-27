use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    System,
    User,
    Assistant,
}

impl Role {
    fn as_str(self) -> &'static str {
        match self {
            Role::System    => "system",
            Role::User      => "user",
            Role::Assistant => "assistant",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: String,
}

/// Maintains conversation history and serialises it to ChatML format.
#[derive(Debug, Default, Clone)]
pub struct ChatSession {
    messages: Vec<ChatMessage>,
}

impl ChatSession {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&mut self, role: Role, content: String) {
        self.messages.push(ChatMessage { role, content });
    }

    pub fn push_assistant(&mut self, content: String) {
        self.push(Role::Assistant, content);
    }

    pub fn clear(&mut self) {
        self.messages.clear();
    }

    pub fn len(&self) -> usize {
        self.messages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    /// Serialise to ChatML with the MASTERd system prompt prepended.
    pub fn to_chatml(&self, system_prompt: &str) -> String {
        let mut out = String::new();
        // Always inject system prompt first.
        out.push_str("<|im_start|>system\n");
        out.push_str(system_prompt);
        out.push_str("<|im_end|>\n");

        for msg in &self.messages {
            out.push_str("<|im_start|>");
            out.push_str(msg.role.as_str());
            out.push('\n');
            out.push_str(&msg.content);
            out.push_str("<|im_end|>\n");
        }

        // Prime the model to generate the next assistant turn.
        out.push_str("<|im_start|>assistant\n");
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chatml_format_correct() {
        let mut s = ChatSession::new();
        s.push(Role::User, "hello".to_string());
        let out = s.to_chatml("sys");
        assert!(out.contains("<|im_start|>system\nsys<|im_end|>"));
        assert!(out.contains("<|im_start|>user\nhello<|im_end|>"));
        assert!(out.ends_with("<|im_start|>assistant\n"));
    }

    #[test]
    fn session_tracks_length() {
        let mut s = ChatSession::new();
        assert_eq!(s.len(), 0);
        s.push(Role::User, "q".to_string());
        assert_eq!(s.len(), 1);
        s.clear();
        assert_eq!(s.len(), 0);
    }
}
