//! Session state: topic + system prompt + conversation history.
//! History is `Message`s (role + content): a plain string, or an array of image blocks + one text block for turns with pasted images.
//! The model lives in the backend — not kept here (varies by backend).

use crate::anthropic::Message;
use crate::image_attach::PastedImage;

/// History image budget (spec A1): far under the API's 100-image cap and the
/// ">20 images => <=2000 px" rule; 20 MB base64 leaves ~12 MB of the 32 MB request
/// for text and the closing prompt.
pub const HISTORY_MAX_IMAGES: usize = 20;
pub const HISTORY_MAX_IMAGE_B64: usize = 20 * 1024 * 1024;

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

    /// Add a user turn that carries images (`[Image #N]` tokens stay in `text`), then
    /// prune the oldest history images over budget. Returns how many were dropped.
    pub fn push_user_with_images(&mut self, text: &str, images: &[PastedImage]) -> usize {
        self.history.push(Message::user_with_images(text, images));
        crate::anthropic::prune_images(&mut self.history, HISTORY_MAX_IMAGES, HISTORY_MAX_IMAGE_B64)
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
        assert_eq!(
            h[1].content,
            serde_json::Value::String("spek'in ne?".into())
        );
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
        s.compact(4, "[CHECKPOINT]");
        let h = s.history();
        assert_eq!(h.len(), 5);
        assert_eq!(
            h[0].content,
            serde_json::Value::String("[CHECKPOINT]".into())
        );
        assert_eq!(h[4].content, serde_json::Value::String("m9".into()));
    }

    #[test]
    fn compact_noop_when_history_short() {
        let mut s = Session::new("rust", "sistem");
        s.push_user("tek");
        s.compact(4, "[CHECKPOINT]");
        assert_eq!(s.history().len(), 1);
    }

    #[test]
    fn push_user_with_images_stores_block_content() {
        let mut s = Session::new("rust", "sistem");
        let img = crate::image_attach::PastedImage {
            media_type: "image/png",
            data: vec![1],
            width: 1,
            height: 1,
        };
        s.push_user_with_images("look [Image #1]", &[img]);
        s.push_user_with_images("no image", &[]);
        let h = s.history();
        assert!(h[0].content.is_array());
        assert_eq!(h[0].text(), "look [Image #1]");
        assert_eq!(h[1].content, serde_json::Value::String("no image".into()));
    }

    #[test]
    fn push_user_with_images_keeps_the_history_under_budget() {
        let img = crate::image_attach::PastedImage {
            media_type: "image/png",
            data: vec![1, 2, 3],
            width: 1,
            height: 1,
        };
        let mut s = Session::new("rust", "sys");
        let mut dropped = 0;
        for i in 0..25 {
            dropped += s
                .push_user_with_images(&format!("turn {i} [Image #1]"), std::slice::from_ref(&img));
            s.push_assistant("ok".into());
        }
        assert_eq!(dropped, 5);
        let req =
            crate::anthropic::MessageRequest::new("m".into(), "s".into(), s.history().to_vec());
        let body = serde_json::to_string(&req).unwrap();
        assert_eq!(
            body.matches("\"type\":\"image\"").count(),
            HISTORY_MAX_IMAGES
        );
        assert_eq!(s.history()[0].text(), "turn 0 [Image #1]");
    }
}
