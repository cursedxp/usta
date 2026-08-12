//! Session state: topic + system prompt + conversation history.
//! History is kept as plain-text `Message`s (role + Value::String content).
//! The model lives in the backend — not kept here (varies by backend).

use crate::anthropic::Message;

/// A single learning session.
pub struct Session {
    /// Active learning topic (e.g. "rust") — selects the progress file on close.
    pub topic: String,
    /// The combined brain system prompt.
    pub system: String,
    /// Conversation history (user/assistant in order).
    history: Vec<Message>,
}

impl Session {
    /// New session — starts with empty history.
    pub fn new(topic: impl Into<String>, system: impl Into<String>) -> Self {
        Session {
            topic: topic.into(),
            system: system.into(),
            history: Vec::new(),
        }
    }

    /// Add a user turn.
    pub fn push_user(&mut self, text: &str) {
        self.history.push(Message::user(text));
    }

    /// Add the assistant's (Usta's) reply as plain text.
    pub fn push_assistant(&mut self, text: String) {
        self.history.push(Message {
            role: "assistant".into(),
            content: serde_json::Value::String(text),
        });
    }

    /// Read-only access to history (to pass to the backend).
    pub fn history(&self) -> &[Message] {
        &self.history
    }

    /// Compaction: reduce history to `note` + the last `keep_last` messages.
    /// Called AFTER an intermediate flush — the essence of the dropped turns
    /// has already been written to the progress/curriculum files, the note
    /// tells the model this.
    pub fn compact(&mut self, keep_last: usize, note: &str) {
        if self.history.len() <= keep_last {
            return;
        }
        let tail = self.history.split_off(self.history.len() - keep_last);
        self.history.clear();
        self.history.push(Message::user(note));
        self.history.extend(tail);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_preserves_order_and_roles() {
        let mut s = Session::new("rust", "sistem");
        s.push_user("merhaba");
        s.push_assistant("spek'in ne?".into());
        s.push_user("bilmiyorum");

        let h = s.history();
        assert_eq!(h.len(), 3);
        assert_eq!(h[0].role, "user");
        assert_eq!(h[0].content, serde_json::Value::String("merhaba".into()));
        assert_eq!(h[1].role, "assistant");
        assert_eq!(h[1].content, serde_json::Value::String("spek'in ne?".into()));
        assert_eq!(h[2].role, "user");
        assert_eq!(h[2].content, serde_json::Value::String("bilmiyorum".into()));
    }

    #[test]
    fn new_session_starts_empty() {
        let s = Session::new("rust", "sistem");
        assert!(s.history().is_empty());
        assert_eq!(s.topic, "rust");
        assert_eq!(s.system, "sistem");
    }

    #[test]
    fn compact_keeps_note_plus_last_n() {
        let mut s = Session::new("rust", "sistem");
        for i in 0..10 {
            s.push_user(&format!("m{i}"));
        }
        s.compact(4, "[ARA KAYIT]");
        let h = s.history();
        assert_eq!(h.len(), 5);
        assert_eq!(h[0].content, serde_json::Value::String("[ARA KAYIT]".into()));
        assert_eq!(h[4].content, serde_json::Value::String("m9".into()));
    }

    #[test]
    fn compact_noop_when_history_short() {
        let mut s = Session::new("rust", "sistem");
        s.push_user("tek");
        s.compact(4, "[ARA KAYIT]");
        assert_eq!(s.history().len(), 1);
    }
}
