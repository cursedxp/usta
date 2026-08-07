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

/// `approaches/` altındaki TÜM `.md` dosyalarını yükle — global ∪ proje,
/// aynı ad proje lehine override edilir (read_approach_with_override).
/// Alfabetik sıra: system prompt deterministik kalsın. Hangi yaklaşımın
/// uygulanacağını kod değil USTA.md "Domaine göre yaklaşım" kuralı seçer.
fn read_all_approaches(project_usta: Option<&PathBuf>, global: &Path, parts: &mut Vec<String>) {
    let mut names: Vec<String> = Vec::new();
    let mut collect = |dir: &std::path::Path| {
        if let Ok(rd) = std::fs::read_dir(dir) {
            for entry in rd.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.ends_with(".md") && !names.contains(&name) {
                    names.push(name);
                }
            }
        }
    };
    collect(&global.join("approaches"));
    if let Some(p) = project_usta {
        collect(&p.join("approaches"));
    }
    names.sort();
    for name in names {
        read_approach_with_override(project_usta, global, &name, parts);
    }
}

/// Global brain + (varsa) proje override/ilerlemesini birleştirip system
/// prompt üret. `project`, `.usta/` İÇEREN proje kökü — proje dosyaları
/// `project.join(".usta")` altında yaşar (`.usta`'nın kendisi değil).
pub fn load_system_prompt(global: &Path, project: Option<&Path>, topic: &str, today: &str) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Model bugünü güvenilir bilmez — "sınava kaç hafta kaldı" gibi hesaplar
    // için sabit referans en başta verilir (USTA.md "Hedefli Öğrenme").
    parts.push(format!("===== BUGÜN =====\n{today}"));

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

    read_all_approaches(project_usta.as_ref(), global, &mut parts);

    if let Some(dir) = &project_usta {
        for rel in [
            format!("learner/curriculum/{topic}.md"),
            format!("learner/progress/{topic}.md"),
        ] {
            read_section(&dir.join(&rel), &rel, &mut parts);
        }
    }

    if parts.len() == 1 {
        // Yalnız BUGÜN bölümü varsa brain dosyaları hiç bulunamamış demektir
        // — çekirdek kural gömülü fallback.
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

        let sys = load_system_prompt(&global, None, "rust", "2026-08-07");
        assert!(sys.contains("ÇEKIRDEK KURAL"));
        assert!(sys.contains("ANIL PROFILI"));
        assert!(sys.contains("USTA.md"));
        assert!(!sys.contains("software.md"));

        let _ = fs::remove_dir_all(global.parent().unwrap());
    }

    #[test]
    fn falls_back_when_no_files() {
        let (global, _project) = temp_pair("empty");
        let sys = load_system_prompt(&global, None, "rust", "2026-08-07");
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

        let sys = load_system_prompt(&global, Some(&project), "rust", "2026-08-07");
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

        let sys = load_system_prompt(&global, Some(&project), "rust", "2026-08-07");
        assert!(sys.contains("SEVIYE: başlangıç"));
        assert!(sys.contains("learner/progress/rust.md"));

        let _ = fs::remove_dir_all(global.parent().unwrap());
    }

    #[test]
    fn project_none_skips_progress_without_panicking() {
        let (global, _project) = temp_pair("noproject");
        fs::write(global.join("USTA.md"), "ÇEKIRDEK").unwrap();
        let sys = load_system_prompt(&global, None, "rust", "2026-08-07");
        assert!(sys.contains("ÇEKIRDEK"));
        assert!(!sys.contains("progress/rust.md"));
        let _ = fs::remove_dir_all(global.parent().unwrap());
    }

    #[test]
    fn loads_every_approach_file_not_just_hardcoded() {
        let (global, _project) = temp_pair("allapproaches");
        fs::create_dir_all(global.join("approaches")).unwrap();
        fs::write(global.join("approaches/software.md"), "YAZILIM YAKLAŞIMI").unwrap();
        fs::write(global.join("approaches/marketing.md"), "MARKETING YAKLAŞIMI").unwrap();
        fs::write(global.join("approaches/_default.md"), "META YAKLAŞIM").unwrap();

        let sys = load_system_prompt(&global, None, "gtm", "2026-08-07");
        assert!(sys.contains("YAZILIM YAKLAŞIMI"));
        assert!(sys.contains("MARKETING YAKLAŞIMI"));
        assert!(sys.contains("META YAKLAŞIM"));

        let _ = fs::remove_dir_all(global.parent().unwrap());
    }

    #[test]
    fn project_only_approach_is_loaded_too() {
        let (global, project) = temp_pair("projonly");
        fs::write(global.join("USTA.md"), "ÇEKIRDEK").unwrap();
        let pa = project.join(".usta/approaches");
        fs::create_dir_all(&pa).unwrap();
        fs::write(pa.join("linux-guvenlik.md"), "KONUYA ÖZEL YAKLAŞIM").unwrap();

        let sys = load_system_prompt(&global, Some(&project), "linux-guvenlik", "2026-08-07");
        assert!(sys.contains("KONUYA ÖZEL YAKLAŞIM"));

        let _ = fs::remove_dir_all(global.parent().unwrap());
    }

    #[test]
    fn curriculum_included_when_present() {
        let (global, project) = temp_pair("curriculum");
        fs::write(global.join("USTA.md"), "ÇEKIRDEK").unwrap();
        let cdir = project.join(".usta/learner/curriculum");
        fs::create_dir_all(&cdir).unwrap();
        fs::write(cdir.join("rust.md"), "HARITA: ownership görüldü").unwrap();

        let sys = load_system_prompt(&global, Some(&project), "rust", "2026-08-07");
        assert!(sys.contains("HARITA: ownership görüldü"));
        assert!(sys.contains("learner/curriculum/rust.md"));

        let _ = fs::remove_dir_all(global.parent().unwrap());
    }

    #[test]
    fn system_prompt_starts_with_today_section() {
        let (global, _project) = temp_pair("today");
        fs::write(global.join("USTA.md"), "ÇEKIRDEK").unwrap();
        let sys = load_system_prompt(&global, None, "rust", "2026-08-07");
        assert!(sys.starts_with("===== BUGÜN =====\n2026-08-07"));
        let _ = fs::remove_dir_all(global.parent().unwrap());
    }
}
