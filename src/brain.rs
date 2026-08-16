//! Brain loader: merges global (shared) + project (specific/progress) markdown
//! files to produce the system prompt.
//! "Thin shell, thick brain" — behavior doesn't live here, it lives in markdown.
//!
//! Hybrid model: `global` = `~/.config/usta` (core rules + learner
//! profile, set up once), `project` = the project root containing `.usta/`
//! (approach overrides + topic-based progress, searched upward like
//! `git` finds `.git` — see `config::find_project_root`).

use std::path::{Path, PathBuf};

/// Read a file; if non-empty, add it to `parts` as a labeled section.
/// A missing/empty file is silently skipped.
fn read_section(path: &Path, label: &str, parts: &mut Vec<String>) {
    if let Ok(text) = std::fs::read_to_string(path) {
        let text = text.trim();
        if !text.is_empty() {
            parts.push(format!("===== {label} =====\n{text}"));
        }
    }
}

/// Read the project-specific approach file under `.usta` if it exists, otherwise
/// its global counterpart — the override wins.
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

/// Load ALL `.md` files under `approaches/` — global ∪ project,
/// same-named files are overridden in favor of the project (read_approach_with_override).
/// Alphabetical order: keeps the system prompt deterministic. Which approach
/// gets applied is chosen not by code but by TEACHING.md's "Approach by Domain" rule.
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

/// Merge the global brain + (if present) the project override/progress to
/// produce the system prompt. `project` is the project root CONTAINING `.usta/` —
/// project files live under `project.join(".usta")` (not `.usta` itself).
pub fn load_system_prompt(global: &Path, project: Option<&Path>, topic: &str, today: &str) -> String {
    let mut parts: Vec<String> = Vec::new();

    // The model doesn't reliably know today's date — a fixed reference is given
    // up front for calculations like "how many weeks until the exam" (GOAL.md "Goal-Directed Learning").
    parts.push(format!("===== TODAY =====\n{today}"));

    read_section(&global.join("SOUL.md"), "SOUL.md", &mut parts);
    read_section(&global.join("RULES.md"), "RULES.md", &mut parts);
    read_section(&global.join("TEACHING.md"), "TEACHING.md", &mut parts);

    let project_usta: Option<PathBuf> = project.map(|p| p.join(".usta"));

    // GOAL is only loaded for a goal-oriented topic — saves ~1.5KB in a goalless
    // session, the model doesn't carry irrelevant exam-pace/format rules (spec §3
    // conditional line). The topic's approach file (project override if present,
    // otherwise global — same PRIORITY as in read_approach_with_override) is read
    // here ONCE just to check whether "## Hedef" is present; the full content is
    // already loaded separately by read_all_approaches below, so it's not added
    // to the prompt a SECOND time here.
    let topic_rel = format!("{topic}.md");
    let topic_approach_path = project_usta
        .as_ref()
        .map(|d| d.join("approaches").join(&topic_rel))
        .filter(|p| p.exists())
        .unwrap_or_else(|| global.join("approaches").join(&topic_rel));
    let approach_konu = std::fs::read_to_string(&topic_approach_path).unwrap_or_default();
    if approach_konu.contains("## Hedef") {
        read_section(&global.join("GOAL.md"), "GOAL.md", &mut parts);
    }

    read_all_approaches(project_usta.as_ref(), global, &mut parts);

    read_section(&global.join("USER.md"), "USER.md", &mut parts);

    // GAMIFICATION is only loaded when the user's USER.md has opted in
    // (`- gamification: on`, shell-managed via `/game on`) — saves the XP/level/badge
    // rules from a session where the player never turned the game on (spec §3
    // conditional line, same pattern as the GOAL block above). Exact-line match, not
    // a naive `contains`, mirrors `game_pref` in main.rs.
    let user_md = std::fs::read_to_string(global.join("USER.md")).unwrap_or_default();
    if user_md.lines().any(|l| l.trim() == "- gamification: on") {
        read_section(&global.join("GAMIFICATION.md"), "GAMIFICATION.md", &mut parts);
    }

    // User-facing project context: definition + status live in the VISIBLE
    // `mentor/` dir at the project root (not under `.usta/`) so the user can
    // read and hand-edit them (spec: mentor layer). Loaded right after the
    // profile: who first, then which project, then how to teach.
    if let Some(p) = project {
        read_section(&p.join("mentor/PROJECT.md"), "mentor/PROJECT.md", &mut parts);
        read_section(&p.join("mentor/PROGRESS.md"), "mentor/PROGRESS.md", &mut parts);
    }

    read_section(
        &global.join("learner/index.md"),
        "learner/index.md",
        &mut parts,
    );

    if let Some(dir) = &project_usta {
        for rel in [
            format!("learner/progress/{topic}.md"),
            format!("learner/curriculum/{topic}.md"),
        ] {
            read_section(&dir.join(&rel), &rel, &mut parts);
        }
    }

    if parts.len() == 1 {
        // If only the TODAY section is present, no brain files were found at all
        // — fall back to the embedded core rule.
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

    /// Lets each test set up its own isolated global/project directory pair.
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
        fs::write(global.join("SOUL.md"), "ÇEKIRDEK KURAL").unwrap();
        fs::write(global.join("USER.md"), "ANIL PROFILI").unwrap();
        // approaches/software.md ve proje/progress bilerek yok.

        let sys = load_system_prompt(&global, None, "rust", "2026-08-07");
        assert!(sys.contains("ÇEKIRDEK KURAL"));
        assert!(sys.contains("ANIL PROFILI"));
        assert!(sys.contains("SOUL.md"));
        assert!(!sys.contains("software.md"));

        let _ = fs::remove_dir_all(global.parent().unwrap());
    }

    #[test]
    fn system_prompt_loads_split_files_not_index() {
        let (global, _project) = temp_pair("split");
        fs::write(global.join("SOUL.md"), "SOUL-İÇERİK").unwrap();
        fs::write(global.join("RULES.md"), "RULES-İÇERİK").unwrap();
        fs::write(global.join("TEACHING.md"), "TEACHING-İÇERİK").unwrap();
        fs::write(global.join("USTA.md"), "İNDEKS-İÇERİK").unwrap();

        let sys = load_system_prompt(&global, None, "rust", "2026-08-07");
        assert!(sys.contains("SOUL-İÇERİK"));
        assert!(sys.contains("RULES-İÇERİK"));
        assert!(sys.contains("TEACHING-İÇERİK"));
        assert!(!sys.contains("İNDEKS-İÇERİK"));

        let _ = fs::remove_dir_all(global.parent().unwrap());
    }

    #[test]
    fn goal_loaded_only_when_approach_has_hedef_section() {
        let (global, _project) = temp_pair("goal");
        fs::create_dir_all(global.join("approaches")).unwrap();
        fs::write(global.join("GOAL.md"), "GOAL-İÇERİK").unwrap();
        fs::write(global.join("approaches/rust.md"), "YAKLAŞIM — hedef yok").unwrap();

        let sys = load_system_prompt(&global, None, "rust", "2026-08-07");
        assert!(!sys.contains("GOAL-İÇERİK"));

        fs::write(
            global.join("approaches/rust.md"),
            "YAKLAŞIM\n## Hedef\n2026-12-01",
        )
        .unwrap();

        let sys2 = load_system_prompt(&global, None, "rust", "2026-08-07");
        assert!(sys2.contains("GOAL-İÇERİK"));

        let _ = fs::remove_dir_all(global.parent().unwrap());
    }

    #[test]
    fn gamification_loaded_only_when_user_md_opts_in() {
        let (global, _project) = temp_pair("gamification");
        fs::write(global.join("GAMIFICATION.md"), "GAMIFICATION-İÇERİK").unwrap();
        fs::write(global.join("USER.md"), "# Profil\n\n## Tercihler\n- gamification: off\n").unwrap();

        let sys = load_system_prompt(&global, None, "rust", "2026-08-07");
        assert!(!sys.contains("GAMIFICATION-İÇERİK"));

        fs::write(global.join("USER.md"), "# Profil\n\n## Tercihler\n- gamification: on\n").unwrap();
        let sys2 = load_system_prompt(&global, None, "rust", "2026-08-07");
        assert!(sys2.contains("GAMIFICATION-İÇERİK"));

        let _ = fs::remove_dir_all(global.parent().unwrap());
    }

    #[test]
    fn user_md_replaces_profile_in_prompt() {
        let (global, _project) = temp_pair("usermd");
        fs::create_dir_all(global.join("learner")).unwrap();
        fs::write(global.join("USER.md"), "USER-İÇERİK").unwrap();
        fs::write(global.join("learner/profile.md"), "PROFILE-İÇERİK").unwrap();

        let sys = load_system_prompt(&global, None, "rust", "2026-08-07");
        assert!(sys.contains("USER-İÇERİK"));
        assert!(!sys.contains("PROFILE-İÇERİK"));

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
        fs::write(global.join("SOUL.md"), "ÇEKIRDEK").unwrap();

        let progress_dir = project.join(".usta/learner/progress");
        fs::create_dir_all(&progress_dir).unwrap();
        fs::write(progress_dir.join("rust.md"), "SEVIYE: başlangıç").unwrap();

        let sys = load_system_prompt(&global, Some(&project), "rust", "2026-08-07");
        assert!(sys.contains("SEVIYE: başlangıç"));
        assert!(sys.contains("learner/progress/rust.md"));

        let _ = fs::remove_dir_all(global.parent().unwrap());
    }

    #[test]
    fn mentor_files_included_when_present_skipped_when_absent() {
        let (global, project) = temp_pair("mentor");
        fs::create_dir_all(global.join("approaches")).unwrap();
        fs::write(global.join("SOUL.md"), "SOUL").unwrap();

        // absent → no mentor section at all
        let sys = load_system_prompt(&global, Some(&project), "rust", "2026-08-14");
        assert!(!sys.contains("mentor/PROJECT.md"));

        // present → both labeled sections appear
        let mentor = project.join("mentor");
        fs::create_dir_all(&mentor).unwrap();
        fs::write(mentor.join("PROJECT.md"), "PRJICERIK").unwrap();
        fs::write(mentor.join("PROGRESS.md"), "PPGICERIK").unwrap();
        let sys = load_system_prompt(&global, Some(&project), "rust", "2026-08-14");
        assert!(sys.contains("mentor/PROJECT.md"));
        assert!(sys.contains("PRJICERIK"));
        assert!(sys.contains("mentor/PROGRESS.md"));
        assert!(sys.contains("PPGICERIK"));

        let _ = fs::remove_dir_all(global.parent().unwrap());
    }

    #[test]
    fn project_none_skips_progress_without_panicking() {
        let (global, _project) = temp_pair("noproject");
        fs::write(global.join("SOUL.md"), "ÇEKIRDEK").unwrap();
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
        fs::write(global.join("SOUL.md"), "ÇEKIRDEK").unwrap();
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
        fs::write(global.join("SOUL.md"), "ÇEKIRDEK").unwrap();
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
        fs::write(global.join("SOUL.md"), "ÇEKIRDEK").unwrap();
        let sys = load_system_prompt(&global, None, "rust", "2026-08-07");
        assert!(sys.starts_with("===== TODAY =====\n2026-08-07"));
        let _ = fs::remove_dir_all(global.parent().unwrap());
    }
}
