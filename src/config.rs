//! Configuration: API key + model + brain root resolution (global + project).

use std::path::{Path, PathBuf};

use anyhow::Result;

/// Resolve the API key from the environment. Pure function — testable.
pub fn resolve_key(env_value: Option<String>) -> Result<String> {
    match env_value {
        Some(k) if !k.trim().is_empty() => Ok(k),
        _ => anyhow::bail!(
            "ANTHROPIC_API_KEY is not set. Run this:\n  export ANTHROPIC_API_KEY=sk-ant-..."
        ),
    }
}

/// Global brain root: `~/.config/usta` (uses `XDG_CONFIG_HOME` if it's set,
/// otherwise falls back to `~/.config`).
///
/// NOTE: `dirs::config_dir()` isn't used directly — on macOS that function
/// returns `~/Library/Application Support` (Apple platform convention),
/// not `~/.config`. Usta is a terminal-native, dotfile-style tool; the target should
/// always be `~/.config/usta`. `dirs::home_dir()` is used to find the correct
/// home directory across platforms, and `XDG_CONFIG_HOME` takes precedence over it.
pub fn global_root() -> Result<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        let p = PathBuf::from(xdg);
        if p.is_absolute() {
            return Ok(p.join("usta"));
        }
    }
    let home = dirs::home_dir().ok_or_else(|| {
        anyhow::anyhow!("home directory not found (dirs::home_dir() returned None)")
    })?;
    Ok(home.join(".config").join("usta"))
}

/// Walk upward from `start` to find the first ancestor containing `.usta/` — like git's
/// `.git` search. The returned value is the directory that CONTAINS `.usta` (the project root),
/// not `.usta` itself. Returns `None` if not found.
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

/// Should a file be written? — a pure decision; `init`'s "create / already exists,
/// skip" logic becomes testable here.
pub fn should_write(path: &Path) -> bool {
    !path.exists()
}

/// Does the code-owned file need syncing? `true` if the file doesn't exist or the
/// on-disk content differs from the embedded one — a USTA.md edit in the repo gets
/// carried over to global on the first launch after a rebuild (see `defaults::Ownership::Code`).
pub fn needs_sync(path: &Path, embedded: &str) -> bool {
    match std::fs::read_to_string(path) {
        Ok(disk) => disk != embedded,
        Err(_) => true,
    }
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
        // canonicalize: on macOS the temp dir can be a symlink (/tmp -> /private/tmp).
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

        // There's no .usta anywhere in this bounded subtree — should return None here.
        // (It walks all the way to the real filesystem root, but a .usta under /tmp
        // isn't expected.)
        assert!(find_project_root(&nested).is_none());

        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn needs_sync_true_for_missing_or_stale_false_for_current() {
        let base =
            std::env::temp_dir().join(format!("usta_config_test_needssync_{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        let stale = base.join("eski.md");
        let current = base.join("guncel.md");
        let missing = base.join("yok.md");
        fs::write(&stale, "eski içerik").unwrap();
        fs::write(&current, "gömülü içerik").unwrap();

        assert!(needs_sync(&missing, "gömülü içerik"));
        assert!(needs_sync(&stale, "gömülü içerik"));
        assert!(!needs_sync(&current, "gömülü içerik"));

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
