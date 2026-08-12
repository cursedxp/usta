//! Embeds repo-root brain files at compile time — the first-time defaults
//! that `usta init` writes to the global install. The canonical source is the
//! `USTA.md` / `learner/` / `approaches/` files at the repo root; this file
//! just carries them into the package via `include_str!`.

/// Who owns the file?
/// - `Code`: canonical source repo — the global copy is kept in sync with the
///   embedded content in the binary on every startup (edit in repo → rebuild
///   → propagates). Don't edit the global copy by hand, it gets overwritten
///   on the next startup.
/// - `User`: written once, then it's the user's (profile, catalog) — never
///   overwritten again.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ownership {
    Code,
    User,
}

/// `(project-relative path, content, ownership)` triples — written verbatim
/// to the global root (`~/.config/usta`) using these paths.
pub fn global_defaults() -> Vec<(&'static str, &'static str, Ownership)> {
    vec![
        ("USTA.md", include_str!("../USTA.md"), Ownership::Code),
        ("SOUL.md", include_str!("../SOUL.md"), Ownership::Code),
        ("RULES.md", include_str!("../RULES.md"), Ownership::Code),
        (
            "TEACHING.md",
            include_str!("../TEACHING.md"),
            Ownership::Code,
        ),
        ("GOAL.md", include_str!("../GOAL.md"), Ownership::Code),
        ("USER.md", include_str!("../USER.md"), Ownership::User),
        (
            "learner/index.md",
            include_str!("../learner/index.md"),
            Ownership::User,
        ),
        (
            "approaches/software.md",
            include_str!("../approaches/software.md"),
            Ownership::Code,
        ),
        (
            "approaches/_default.md",
            include_str!("../approaches/_default.md"),
            Ownership::Code,
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_all_nonempty_files() {
        let defaults = global_defaults();
        assert_eq!(defaults.len(), 9);
        for (rel, content, _) in defaults {
            assert!(!content.trim().is_empty(), "boş içerik: {rel}");
        }
    }

    #[test]
    fn core_behavior_is_code_owned_learner_is_user_owned() {
        // Code: USTA (index) + behavior files + approaches/*.
        // User: USER.md (root) + learner/index.md — the `learner/` prefix no
        // longer determines ownership (USER.md is at root but User-owned).
        const USER_OWNED: &[&str] = &["USER.md", "learner/index.md"];
        for (rel, _, ownership) in global_defaults() {
            let expected = if USER_OWNED.contains(&rel) {
                Ownership::User
            } else {
                Ownership::Code
            };
            assert_eq!(ownership, expected, "yanlış sahiplik: {rel}");
        }
    }

    /// The embedded default profile (USER.md) must NOT carry a personal
    /// name — the app is public, a new user shouldn't be greeted with a
    /// stranger's name (the TUI greeting reads this value). Regression guard:
    /// breaks if someone adds a name back to the seed.
    #[test]
    fn shipped_profile_carries_no_personal_name() {
        let profile = global_defaults()
            .into_iter()
            .find(|(rel, _, _)| *rel == "USER.md")
            .map(|(_, content, _)| content)
            .expect("USER.md gömülü default'larda olmalı");
        assert_eq!(
            crate::tui::welcome::extract_name(profile),
            None,
            "gömülü profil kişisel isim taşıyor — jenerik olmalı"
        );
    }
}
