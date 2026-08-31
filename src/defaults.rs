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
        (
            "GAMIFICATION.md",
            include_str!("../GAMIFICATION.md"),
            Ownership::Code,
        ),
        (
            "MATERIAL.md",
            include_str!("../MATERIAL.md"),
            Ownership::Code,
        ),
        (
            "PREDICTION.md",
            include_str!("../PREDICTION.md"),
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
        assert_eq!(defaults.len(), 12);
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
            crate::tui::welcome_data::extract_name(profile),
            None,
            "gömülü profil kişisel isim taşıyor — jenerik olmalı"
        );
    }

    #[test]
    fn teaching_promise_matches_ride_along_watcher() {
        // Spec K5.3: saving a file must not promise an automatic review turn —
        // the watcher accumulates and the review comes with the user's next
        // message (spec K1/K2). Pins the embedded default TEACHING.md.
        let teaching = include_str!("../TEACHING.md");
        assert!(!teaching.contains("triggers your review automatically"));
        assert!(teaching.contains("saving alone does not start your review"));
    }

    #[test]
    fn soul_carries_the_terminology_lock() {
        // Live sessions (2026-08-31): mirroring the user's language turned into
        // translating the field's vocabulary — "arguments" became "kelimeler", the
        // same word named index 0 and index 1 four sentences apart, and one
        // simplification stated outright that cargo compiles the code (rustc does).
        // The rule that stops this is prompt text, so the only thing that can guard
        // it is a pin: nobody gets to quietly tidy it away while simplifying Voice.
        // The block is inserted verbatim from the design spec and its bullets
        // are hard-wrapped; needles must not span a line break, or `contains`
        // fails on the embedded newline even though the text is correct.
        let soul = include_str!("../SOUL.md");
        for needle in [
            "TERMINOLOGY LOCK",
            "Simplify the explanation, never the name",
            "practitioners of that field actually use",
            "not protected by looking exotic",
            "One concept, one word",
            "Precision outranks simplicity",
            "Write natively in the user's language",
        ] {
            assert!(
                soul.contains(needle),
                "SOUL.md lost the terminology rule: {needle}"
            );
        }
    }

    #[test]
    fn terminology_lock_follows_the_language_lock() {
        // Order carries meaning: which language to write in first, then how to write
        // its vocabulary. Reversed, the terminology rule reads as free-standing
        // style advice instead of the boundary on the language mirror.
        let soul = include_str!("../SOUL.md");
        let language = soul
            .find("LANGUAGE LOCK")
            .expect("SOUL.md lost the language lock");
        let terminology = soul
            .find("TERMINOLOGY LOCK")
            .expect("SOUL.md lost the terminology lock");
        assert!(language < terminology);
    }
}
