//! Repo-kökü brain dosyalarını derleme-zamanında gömer — `usta init`'in global
//! kuruluma yazacağı ilk-kez varsayılanlar. Canonical kaynak repo kökündeki
//! `USTA.md` / `learner/` / `approaches/` dosyalarıdır; bu dosya sadece onları
//! `include_str!` ile pakete taşır.

/// `(proje-göreceli yol, içerik)` çiftleri — global köke (`~/.config/usta`)
/// birebir bu yollarla yazılır.
pub fn global_defaults() -> Vec<(&'static str, &'static str)> {
    vec![
        ("USTA.md", include_str!("../USTA.md")),
        ("learner/profile.md", include_str!("../learner/profile.md")),
        ("learner/index.md", include_str!("../learner/index.md")),
        (
            "approaches/software.md",
            include_str!("../approaches/software.md"),
        ),
        (
            "approaches/_default.md",
            include_str!("../approaches/_default.md"),
        ),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_five_nonempty_files() {
        let defaults = global_defaults();
        assert_eq!(defaults.len(), 5);
        for (rel, content) in defaults {
            assert!(!content.trim().is_empty(), "boş içerik: {rel}");
        }
    }
}
