//! Brain yükleyici: markdown dosyalarını birleştirip system prompt üretir.
//! "İnce kabuk, kalın beyin" — davranış burada değil, markdown'da yaşar.

use std::path::Path;

/// Birleştirilecek dosyalar (brain kökünden görece). Eksik olan sessizce atlanır.
fn brain_files(topic: &str) -> Vec<String> {
    vec![
        "USTA.md".to_string(),
        "learner/profile.md".to_string(),
        "approaches/software.md".to_string(),
        "approaches/_default.md".to_string(),
        format!("learner/progress/{topic}.md"),
    ]
}

/// Brain dosyalarını oku, birleştir. Eksik dosyalar atlanır.
pub fn load_system_prompt(root: &Path, topic: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    for rel in brain_files(topic) {
        let path = root.join(&rel);
        if let Ok(text) = std::fs::read_to_string(&path) {
            let text = text.trim();
            if !text.is_empty() {
                parts.push(format!("===== {rel} =====\n{text}"));
            }
        }
    }
    if parts.is_empty() {
        // Brain dosyaları bulunamazsa çekirdek kural gömülü fallback.
        return FALLBACK_SYSTEM.to_string();
    }
    parts.join("\n\n")
}

const FALLBACK_SYSTEM: &str = "\
Sen Usta'sın: yaparak-öğrenmeyi yürüten senior bir mühendislik mentorusun. \
Asla kullanıcının yerine kod yazma veya düzeltme. Neyin hatalı olduğunu ve \
nasıl yapılması gerektiğini göster; kodu kullanıcı yazar. Bilmediğin bir şeyi \
uydurma — web_search ile araştır, sonra öğret. Kullanıcı ADHD; yargılama yok, \
'suya gir' — mükemmel spek bekleme, parçaya böl.";

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn concatenates_existing_files_skips_missing() {
        let dir = std::env::temp_dir().join(format!("usta_brain_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("learner/progress")).unwrap();
        fs::create_dir_all(dir.join("approaches")).unwrap();
        fs::write(dir.join("USTA.md"), "ÇEKIRDEK KURAL").unwrap();
        fs::write(dir.join("learner/profile.md"), "ANIL PROFILI").unwrap();
        // approaches/software.md ve progress/rust.md bilerek yok.

        let sys = load_system_prompt(&dir, "rust");
        assert!(sys.contains("ÇEKIRDEK KURAL"));
        assert!(sys.contains("ANIL PROFILI"));
        assert!(sys.contains("USTA.md"));
        assert!(!sys.contains("software.md"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn falls_back_when_no_files() {
        let dir = std::env::temp_dir().join(format!("usta_brain_empty_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let sys = load_system_prompt(&dir, "rust");
        assert!(sys.contains("Usta"));
        let _ = fs::remove_dir_all(&dir);
    }
}
