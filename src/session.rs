//! Oturum durumu: konu + system prompt + konuşma geçmişi.
//! Geçmiş düz-metin `Message`'lar olarak tutulur (role + Value::String içerik).
//! Model backend'de yaşar — burada tutulmaz (backend'e göre değişir).

use crate::anthropic::Message;

/// Tek bir öğrenme oturumu.
pub struct Session {
    /// Aktif öğrenme başlığı (ör. "rust").
    pub topic: String,
    /// Birleştirilmiş brain system prompt'u.
    pub system: String,
    /// Konuşma geçmişi (user/assistant sırayla).
    history: Vec<Message>,
}

impl Session {
    /// Yeni oturum — boş geçmişle başlar.
    pub fn new(topic: impl Into<String>, system: impl Into<String>) -> Self {
        Session {
            topic: topic.into(),
            system: system.into(),
            history: Vec::new(),
        }
    }

    /// Kullanıcı turn'ü ekle.
    pub fn push_user(&mut self, text: &str) {
        self.history.push(Message::user(text));
    }

    /// Asistan (Usta) yanıtını düz-metin olarak ekle.
    pub fn push_assistant(&mut self, text: String) {
        self.history.push(Message {
            role: "assistant".into(),
            content: serde_json::Value::String(text),
        });
    }

    /// Geçmişe salt-okunur erişim (backend'e geçmek için).
    pub fn history(&self) -> &[Message] {
        &self.history
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
}
