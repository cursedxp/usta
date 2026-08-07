//! Oturum durumu: konu + system prompt + konuşma geçmişi.
//! Geçmiş düz-metin `Message`'lar olarak tutulur (role + Value::String içerik).
//! Model backend'de yaşar — burada tutulmaz (backend'e göre değişir).

use crate::anthropic::Message;

/// Tek bir öğrenme oturumu.
pub struct Session {
    /// Aktif öğrenme başlığı (ör. "rust") — kapanışta progress dosyasını seçer.
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

    /// Kompaksiyon: history'yi `note` + son `keep_last` mesaja indir.
    /// Ara-flush SONRASI çağrılır — atılan turn'lerin özü zaten progress/
    /// curriculum dosyalarına yazılmıştır, note bunu modele söyler.
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
