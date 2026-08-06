//! Dosya değişiklik yükü: LLM'e ne gideceğine buradaki saf mantık karar verir.
//! İlk görüşte tam içerik (bağlam kurulsun), sonraki kayıtlarda unified diff
//! (token tasarrufu + "ne değişti" sinyali), boyut tavanı üstünde tek seferlik
//! yerel uyarı. IO yok — main okur, biz karar veririz.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use similar::TextDiff;

/// Bu boyutun üstündeki dosyalar LLM'e gönderilmez (context + maliyet koruması).
pub const MAX_FILE_BYTES: usize = 64 * 1024;

/// Bir kayıt olayının LLM'e yansıma biçimi.
pub enum ChangePayload {
    /// Dosya ilk kez görüldü — tam içerik gönderilir.
    FirstSight(String),
    /// Önceki görüşe göre unified diff.
    Diff(String),
    /// Boyut tavanı aşıldı — sadece yerel uyarı, LLM çağrısı yok (dosya başına bir kez).
    TooLarge(usize),
    /// İçerik değişmemiş veya daha önce uyarılmış büyük dosya — sessiz geç.
    Skip,
}

/// Oturum boyunca görülen dosya içeriklerinin hafızası.
pub struct FileMemory {
    seen: HashMap<PathBuf, String>,
    warned_large: HashSet<PathBuf>,
}

impl FileMemory {
    pub fn new() -> Self {
        FileMemory {
            seen: HashMap::new(),
            warned_large: HashSet::new(),
        }
    }

    /// Yeni kaydedilen içeriği gözlemle, LLM yükünü üret, hafızayı güncelle.
    pub fn observe(&mut self, path: &Path, current: String) -> ChangePayload {
        if current.len() > MAX_FILE_BYTES {
            if self.warned_large.insert(path.to_path_buf()) {
                return ChangePayload::TooLarge(current.len());
            }
            return ChangePayload::Skip;
        }
        match self.seen.insert(path.to_path_buf(), current.clone()) {
            None => ChangePayload::FirstSight(current),
            Some(prev) if prev == current => ChangePayload::Skip,
            Some(prev) => {
                let diff = TextDiff::from_lines(&prev, &current)
                    .unified_diff()
                    .context_radius(3)
                    .header("önce", "sonra")
                    .to_string();
                ChangePayload::Diff(diff)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn first_sight_returns_full_content() {
        let mut m = FileMemory::new();
        match m.observe(Path::new("a.rs"), "fn main() {}".into()) {
            ChangePayload::FirstSight(s) => assert_eq!(s, "fn main() {}"),
            _ => panic!("ilk görüş FirstSight olmalı"),
        }
    }

    #[test]
    fn unchanged_content_is_skipped() {
        let mut m = FileMemory::new();
        let _ = m.observe(Path::new("a.rs"), "ayni".into());
        assert!(matches!(
            m.observe(Path::new("a.rs"), "ayni".into()),
            ChangePayload::Skip
        ));
    }

    #[test]
    fn changed_content_yields_unified_diff() {
        let mut m = FileMemory::new();
        let _ = m.observe(Path::new("a.rs"), "eski satir\n".into());
        match m.observe(Path::new("a.rs"), "yeni satir\n".into()) {
            ChangePayload::Diff(d) => {
                assert!(d.contains("-eski satir"));
                assert!(d.contains("+yeni satir"));
            }
            _ => panic!("değişiklik Diff olmalı"),
        }
    }

    #[test]
    fn oversized_file_warns_once_then_skips() {
        let mut m = FileMemory::new();
        let big = "x".repeat(MAX_FILE_BYTES + 1);
        assert!(matches!(
            m.observe(Path::new("big.rs"), big.clone()),
            ChangePayload::TooLarge(_)
        ));
        assert!(matches!(
            m.observe(Path::new("big.rs"), big),
            ChangePayload::Skip
        ));
    }

    #[test]
    fn diff_is_per_file_not_global() {
        let mut m = FileMemory::new();
        let _ = m.observe(Path::new("a.rs"), "a icerik\n".into());
        // b.rs ilk kez görülüyor — a.rs'nin geçmişiyle diff'lenmemeli.
        assert!(matches!(
            m.observe(Path::new("b.rs"), "b icerik\n".into()),
            ChangePayload::FirstSight(_)
        ));
    }
}
