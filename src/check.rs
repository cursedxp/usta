//! Kayıt sonrası `cargo check` — tahmin protokolünün hammaddesi. Sonuç LLM'e
//! "sadece senin gözün için" bloğu olarak gider; kullanıcıya ne zaman
//! açılacağına (önce tahmin ettirerek) USTA.md kuralları karar verir.
//! Cargo projesi değilse / check koşamazsa sessizce yok sayılır — feedback
//! akışı asla engellenmez.

use std::path::Path;
use std::time::Duration;

use tokio::process::Command;

/// Çıktı tavanı — devasa hata listeleri context'i şişirmesin.
pub const MAX_CHECK_BYTES: usize = 4 * 1024;

/// Check zaman tavanı — soğuk cache'te ilk check uzun sürebilir.
const CHECK_TIMEOUT: Duration = Duration::from_secs(60);

/// Proje kökünde Cargo.toml var mı?
pub fn is_cargo_project(root: &Path) -> bool {
    root.join("Cargo.toml").is_file()
}

/// Çıktıyı tavana kırp — UTF-8 char sınırına saygıyla; kırpıldıysa not düş.
pub fn truncate_output(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut cut = max;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n… (kırpıldı — toplam {} bayt)", &s[..cut], s.len())
}

/// `cargo check --message-format=short` koştur. Cargo projesi değilse,
/// cargo çalıştırılamazsa veya timeout'a takılırsa `None` — tahmin protokolü
/// o kayıtta atlanır, feedback normal akar.
pub async fn run_check(root: &Path) -> Option<String> {
    if !is_cargo_project(root) {
        return None;
    }
    let fut = Command::new("cargo")
        .arg("check")
        .arg("--message-format=short")
        .current_dir(root)
        .output();
    let output = tokio::time::timeout(CHECK_TIMEOUT, fut).await.ok()?.ok()?;
    if output.status.success() {
        return Some("TEMİZ — cargo check hatasız geçti.".to_string());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    Some(truncate_output(stderr.trim(), MAX_CHECK_BYTES))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_cargo_project_true_when_manifest_exists() {
        let base = std::env::temp_dir().join(format!(
            "usta_check_test_manifest_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        std::fs::write(base.join("Cargo.toml"), "[package]").unwrap();
        assert!(is_cargo_project(&base));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn is_cargo_project_false_without_manifest() {
        let base = std::env::temp_dir().join(format!(
            "usta_check_test_nomanifest_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        assert!(!is_cargo_project(&base));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn truncate_passes_short_output_through() {
        assert_eq!(truncate_output("kısa", 100), "kısa");
    }

    #[test]
    fn truncate_cuts_long_output_with_note() {
        let long = "a".repeat(200);
        let out = truncate_output(&long, 100);
        assert!(out.len() < 200);
        assert!(out.contains("kırpıldı"));
    }

    #[test]
    fn truncate_respects_utf8_char_boundary() {
        // "ö" 2 bayt — tavan bir char'ın ortasına denk gelirse panik atmamalı.
        let s = "ööööö";
        let out = truncate_output(s, 3);
        assert!(out.contains("kırpıldı"));
    }
}
