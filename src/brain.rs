//! Brain yükleyici: global (paylaşılan) + proje (özel/ilerleme) markdown
//! dosyalarını birleştirip system prompt üretir.
//! "İnce kabuk, kalın beyin" — davranış burada değil, markdown'da yaşar.
//!
//! Hibrit model: `global` = `~/.config/usta` (çekirdek kurallar + öğrenci
//! profili, bir kere kurulur), `project` = `.usta/` içeren proje kökü
//! (yaklaşım override'ları + konu bazlı ilerleme, `git`in `.git` bulması gibi
//! yukarı doğru aranır — bkz. `config::find_project_root`).

use std::path::{Path, PathBuf};

/// Bir dosyayı oku; boş değilse etiketli bölüm olarak `parts`'a ekle.
/// Eksik/boş dosya sessizce atlanır.
fn read_section(path: &Path, label: &str, parts: &mut Vec<String>) {
    if let Ok(text) = std::fs::read_to_string(path) {
        let text = text.trim();
        if !text.is_empty() {
            parts.push(format!("===== {label} =====\n{text}"));
        }
    }
}

/// `.usta` altındaki proje-özel yaklaşım dosyası varsa onu, yoksa global
/// karşılığını oku — override kazanır.
fn read_approach_with_override(
    project_usta: Option<&PathBuf>,
    global: &Path,
    rel: &str,
    parts: &mut Vec<String>,
) {
    let override_path = project_usta.map(|d| d.join("approaches").join(rel));
    match override_path.as_deref().filter(|p| p.exists()) {
        Some(p) => read_section(p, &format!("approaches/{rel} (proje override)"), parts),
        None => read_section(
            &global.join("approaches").join(rel),
            &format!("approaches/{rel}"),
            parts,
        ),
    }
}

/// Global brain + (varsa) proje override/ilerlemesini birleştirip system
/// prompt üret. `project`, `.usta/` İÇEREN proje kökü — proje dosyaları
/// `project.join(".usta")` altında yaşar (`.usta`'nın kendisi değil).
pub fn load_system_prompt(global: &Path, project: Option<&Path>, topic: &str) -> String {
    let mut parts: Vec<String> = Vec::new();

    read_section(&global.join("USTA.md"), "USTA.md", &mut parts);
    read_section(
        &global.join("learner/profile.md"),
        "learner/profile.md",
        &mut parts,
    );
    read_section(
        &global.join("learner/index.md"),
        "learner/index.md",
        &mut parts,
    );

    let project_usta: Option<PathBuf> = project.map(|p| p.join(".usta"));

    read_approach_with_override(project_usta.as_ref(), global, "software.md", &mut parts);
    read_approach_with_override(project_usta.as_ref(), global, "_default.md", &mut parts);

    if let Some(dir) = &project_usta {
        let rel = format!("learner/progress/{topic}.md");
        read_section(&dir.join(&rel), &rel, &mut parts);
    }

    if parts.is_empty() {
        // Brain dosyaları hiç bulunamazsa çekirdek kural gömülü fallback.
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

    /// Her testin kendi izole global/proje dizin çiftini kurmasını sağlar.
    fn temp_pair(name: &str) -> (PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("usta_brain_{name}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let global = base.join("global");
        let project = base.join("project");
        fs::create_dir_all(&global).unwrap();
        fs::create_dir_all(&project).unwrap();
        (global, project)
    }

    #[test]
    fn concatenates_existing_files_skips_missing() {
        let (global, _project) = temp_pair("concat");
        fs::create_dir_all(global.join("learner")).unwrap();
        fs::write(global.join("USTA.md"), "ÇEKIRDEK KURAL").unwrap();
        fs::write(global.join("learner/profile.md"), "ANIL PROFILI").unwrap();
        // approaches/software.md ve proje/progress bilerek yok.

        let sys = load_system_prompt(&global, None, "rust");
        assert!(sys.contains("ÇEKIRDEK KURAL"));
        assert!(sys.contains("ANIL PROFILI"));
        assert!(sys.contains("USTA.md"));
        assert!(!sys.contains("software.md"));

        let _ = fs::remove_dir_all(global.parent().unwrap());
    }

    #[test]
    fn falls_back_when_no_files() {
        let (global, _project) = temp_pair("empty");
        let sys = load_system_prompt(&global, None, "rust");
        assert!(sys.contains("Usta"));
        let _ = fs::remove_dir_all(global.parent().unwrap());
    }

    #[test]
    fn project_approach_override_wins_over_global() {
        let (global, project) = temp_pair("override");
        fs::create_dir_all(global.join("approaches")).unwrap();
        fs::write(
            global.join("approaches/software.md"),
            "GLOBAL SOFTWARE YAKLAŞIMI",
        )
        .unwrap();

        let project_usta = project.join(".usta/approaches");
        fs::create_dir_all(&project_usta).unwrap();
        fs::write(
            project_usta.join("software.md"),
            "PROJE ÖZEL SOFTWARE YAKLAŞIMI",
        )
        .unwrap();

        let sys = load_system_prompt(&global, Some(&project), "rust");
        assert!(sys.contains("PROJE ÖZEL SOFTWARE YAKLAŞIMI"));
        assert!(!sys.contains("GLOBAL SOFTWARE YAKLAŞIMI"));

        let _ = fs::remove_dir_all(global.parent().unwrap());
    }

    #[test]
    fn project_progress_included_when_present() {
        let (global, project) = temp_pair("progress");
        fs::write(global.join("USTA.md"), "ÇEKIRDEK").unwrap();

        let progress_dir = project.join(".usta/learner/progress");
        fs::create_dir_all(&progress_dir).unwrap();
        fs::write(progress_dir.join("rust.md"), "SEVIYE: başlangıç").unwrap();

        let sys = load_system_prompt(&global, Some(&project), "rust");
        assert!(sys.contains("SEVIYE: başlangıç"));
        assert!(sys.contains("learner/progress/rust.md"));

        let _ = fs::remove_dir_all(global.parent().unwrap());
    }

    #[test]
    fn project_none_skips_progress_without_panicking() {
        let (global, _project) = temp_pair("noproject");
        fs::write(global.join("USTA.md"), "ÇEKIRDEK").unwrap();
        let sys = load_system_prompt(&global, None, "rust");
        assert!(sys.contains("ÇEKIRDEK"));
        assert!(!sys.contains("progress/rust.md"));
        let _ = fs::remove_dir_all(global.parent().unwrap());
    }
}
