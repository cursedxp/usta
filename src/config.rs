//! Yapılandırma: API key + model + brain kökü çözümleme (global + proje).

use std::path::{Path, PathBuf};

use anyhow::Result;

/// API key'i çevreden çöz. Saf fonksiyon — test edilebilir.
pub fn resolve_key(env_value: Option<String>) -> Result<String> {
    match env_value {
        Some(k) if !k.trim().is_empty() => Ok(k),
        _ => anyhow::bail!(
            "ANTHROPIC_API_KEY tanımlı değil. Şunu çalıştır:\n  export ANTHROPIC_API_KEY=sk-ant-..."
        ),
    }
}

/// Global brain kökü: `~/.config/usta` (`XDG_CONFIG_HOME` set'liyse onu
/// kullanır, yoksa `~/.config`'e düşer).
///
/// NOT: `dirs::config_dir()` doğrudan kullanılmadı — macOS'ta o fonksiyon
/// `~/Library/Application Support` döndürüyor (Apple platform konvansiyonu),
/// `~/.config` değil. Usta terminal-native, dotfile-tarzı bir araç; hedef her
/// zaman `~/.config/usta` olmalı. `dirs::home_dir()` platformlar arası doğru
/// ev dizinini bulmak için kullanılıyor, `XDG_CONFIG_HOME` üstüne biner.
pub fn global_root() -> Result<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        let p = PathBuf::from(xdg);
        if p.is_absolute() {
            return Ok(p.join("usta"));
        }
    }
    let home = dirs::home_dir()
        .ok_or_else(|| anyhow::anyhow!("ev dizini bulunamadı (dirs::home_dir() None döndü)"))?;
    Ok(home.join(".config").join("usta"))
}

/// `start`'tan yukarı doğru yürüyerek ilk `.usta/` içeren atayı bul — git'in
/// `.git` araması gibi. Dönen değer `.usta`'yı İÇEREN dizin (proje kökü),
/// `.usta`'nın kendisi değil. Bulunamazsa `None`.
pub fn find_project_root(start: &Path) -> Option<PathBuf> {
    let mut dir = Some(start);
    while let Some(d) = dir {
        if d.join(".usta").is_dir() {
            return Some(d.to_path_buf());
        }
        dir = d.parent();
    }
    None
}

/// Bir dosyaya yazılmalı mı? — saf karar, `init`'in "oluştur / zaten var,
/// atla" mantığı burada test edilebilir hale gelir.
pub fn should_write(path: &Path) -> bool {
    !path.exists()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn resolve_key_ok_when_set() {
        assert_eq!(resolve_key(Some("sk-ant-x".into())).unwrap(), "sk-ant-x");
    }

    #[test]
    fn resolve_key_errors_when_missing_or_blank() {
        assert!(resolve_key(None).is_err());
        assert!(resolve_key(Some("   ".into())).is_err());
    }

    #[test]
    fn find_project_root_walks_up_to_ancestor_with_usta_dir() {
        let base =
            std::env::temp_dir().join(format!("usta_config_test_walkup_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let nested = base.join("a/b/c");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(base.join(".usta")).unwrap();

        let found = find_project_root(&nested);
        // canonicalize: macOS temp dir'i symlink (/tmp -> /private/tmp) olabilir.
        assert_eq!(
            found.unwrap().canonicalize().unwrap(),
            base.canonicalize().unwrap()
        );

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn find_project_root_returns_none_when_absent() {
        let base =
            std::env::temp_dir().join(format!("usta_config_test_noproject_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        let nested = base.join("x/y");
        fs::create_dir_all(&nested).unwrap();

        // Sınırlı bir alt-ağaçta .usta hiçbir yerde yok — burada None dönmeli.
        // (Gerçek filesystem kökü boyunca yürür ama /tmp altında .usta olması
        // beklenmez.)
        assert!(find_project_root(&nested).is_none());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn should_write_true_for_missing_false_for_existing() {
        let base = std::env::temp_dir().join(format!(
            "usta_config_test_shouldwrite_{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let existing = base.join("var.md");
        let missing = base.join("yok.md");
        fs::write(&existing, "içerik").unwrap();

        assert!(!should_write(&existing));
        assert!(should_write(&missing));

        let _ = fs::remove_dir_all(&base);
    }
}
