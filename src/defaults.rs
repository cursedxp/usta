//! Repo-kökü brain dosyalarını derleme-zamanında gömer — `usta init`'in global
//! kuruluma yazacağı ilk-kez varsayılanlar. Canonical kaynak repo kökündeki
//! `USTA.md` / `learner/` / `approaches/` dosyalarıdır; bu dosya sadece onları
//! `include_str!` ile pakete taşır.

/// Dosyanın sahibi kim?
/// - `Code`: canonical kaynak repo — global kopya binary'deki gömülü içerikle
///   her açılışta senkron tutulur (repo'da düzenle → rebuild → yayılır).
///   Global kopyayı elle düzenleme, sonraki açılışta ezilir.
/// - `User`: ilk-kez yazılır, sonrası kullanıcınındır (profil, katalog) —
///   asla üstüne yazılmaz.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ownership {
    Code,
    User,
}

/// `(proje-göreceli yol, içerik, sahiplik)` üçlüleri — global köke
/// (`~/.config/usta`) birebir bu yollarla yazılır.
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
        // Code: USTA (indeks) + davranış dosyaları + approaches/*.
        // User: USER.md (kök) + learner/index.md — `learner/` öneki artık
        // sahiplik belirlemez (USER.md kökte ama User-owned).
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

    /// Gömülü default profil (USER.md) kişisel isim TAŞIMAMALI — uygulama
    /// herkese açık, yeni kullanıcı yabancı bir isimle karşılanmamalı (TUI
    /// selamı bu değeri okur). Regresyon bekçisi: birileri seed'e tekrar isim
    /// eklerse kırılır.
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
