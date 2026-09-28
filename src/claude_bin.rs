//! Find the `claude` executable on any OS: an explicit `USTA_CLAUDE` path,
//! then PATH (with this platform's file names), then the folders Claude Code
//! installs into. Pure core (`find_claude_in`) + a thin env-reading wrapper.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// File names `claude` ships under on this OS, most robust first.
pub fn names() -> &'static [&'static str] {
    if cfg!(windows) {
        &["claude.exe", "claude.cmd"]
    } else {
        &["claude"]
    }
}

/// Install folders checked after PATH (a fresh install, or a terminal whose
/// PATH doesn't carry them).
fn known_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(home) = dirs::home_dir() {
        out.push(home.join(".local").join("bin")); // native installer, every OS
        out.push(home.join(".claude").join("local")); // older "local install"
    }
    if cfg!(windows) {
        if let Some(appdata) = dirs::data_dir() {
            out.push(appdata.join("npm")); // npm global shims
        }
    } else {
        out.push(PathBuf::from("/opt/homebrew/bin"));
        out.push(PathBuf::from("/usr/local/bin"));
    }
    out
}

/// Locate `claude` from the real environment.
pub fn find_claude() -> Option<PathBuf> {
    find_claude_in(
        std::env::var_os("USTA_CLAUDE"),
        std::env::var_os("PATH"),
        &known_dirs(),
        names(),
    )
}

/// Explicit path (if it is a file) → PATH dirs → known dirs; each dir is
/// tried with every name in order. First existing FILE wins.
pub fn find_claude_in(
    explicit: Option<OsString>,
    path_var: Option<OsString>,
    known: &[PathBuf],
    names: &[&str],
) -> Option<PathBuf> {
    if let Some(p) = explicit.map(PathBuf::from) {
        if p.is_file() {
            return Some(p);
        }
    }
    let path_dirs: Vec<PathBuf> = path_var
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    path_dirs
        .iter()
        .chain(known.iter())
        .flat_map(|d| names.iter().map(move |n| d.join(n)))
        .find(|c| c.is_file())
}

/// `.cmd` / `.bat` run through cmd.exe, where a multi-line argument isn't
/// safe — those get the system prompt from a file instead.
pub fn needs_prompt_file(bin: &Path) -> bool {
    bin.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    const UNIX: &[&str] = &["claude"];
    const WIN: &[&str] = &["claude.exe", "claude.cmd"];

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("usta-claude-bin-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn touch(p: &Path) {
        fs::write(p, "").unwrap();
    }

    fn os(p: &Path) -> Option<OsString> {
        Some(p.as_os_str().to_owned())
    }

    #[test]
    fn explicit_path_wins_over_path() {
        let d = scratch("explicit");
        let bin = d.join("my-claude");
        touch(&bin);
        let on_path = scratch("explicit-on-path");
        touch(&on_path.join("claude"));
        assert_eq!(find_claude_in(os(&bin), os(&on_path), &[], UNIX), Some(bin));
    }

    #[test]
    fn missing_explicit_path_falls_through_to_path() {
        let on_path = scratch("missing-explicit");
        touch(&on_path.join("claude"));
        let got = find_claude_in(Some("/nope/claude".into()), os(&on_path), &[], UNIX);
        assert_eq!(got, Some(on_path.join("claude")));
    }

    #[test]
    fn found_in_second_path_dir() {
        let a = scratch("path-a");
        let b = scratch("path-b");
        touch(&b.join("claude"));
        let path = std::env::join_paths([&a, &b]).unwrap();
        assert_eq!(
            find_claude_in(None, Some(path), &[], UNIX),
            Some(b.join("claude"))
        );
    }

    #[test]
    fn known_dir_used_when_path_lacks_it() {
        let empty = scratch("known-empty-path");
        let local_bin = scratch("known-local-bin");
        touch(&local_bin.join("claude"));
        let got = find_claude_in(None, os(&empty), &[local_bin.clone()], UNIX);
        assert_eq!(got, Some(local_bin.join("claude")));
    }

    #[test]
    fn windows_names_prefer_exe_over_cmd_in_same_dir() {
        let d = scratch("win-both");
        touch(&d.join("claude.cmd"));
        touch(&d.join("claude.exe"));
        assert_eq!(
            find_claude_in(None, os(&d), &[], WIN),
            Some(d.join("claude.exe"))
        );
    }

    #[test]
    fn windows_npm_cmd_found_in_known_dir() {
        let npm = scratch("win-npm");
        touch(&npm.join("claude.cmd"));
        assert_eq!(
            find_claude_in(None, None, &[npm.clone()], WIN),
            Some(npm.join("claude.cmd"))
        );
    }

    #[test]
    fn a_directory_named_claude_is_not_a_hit() {
        let d = scratch("dir-named-claude");
        fs::create_dir_all(d.join("claude")).unwrap();
        assert_eq!(find_claude_in(None, os(&d), &[], UNIX), None);
    }

    #[test]
    fn nothing_anywhere_is_none() {
        let d = scratch("nothing");
        assert_eq!(find_claude_in(None, os(&d), &[d.clone()], WIN), None);
        assert_eq!(find_claude_in(None, None, &[], UNIX), None);
    }

    #[test]
    fn prompt_file_only_for_batch_shims() {
        assert!(needs_prompt_file(Path::new(r"C:\npm\claude.cmd")));
        assert!(needs_prompt_file(Path::new("claude.CMD")));
        assert!(needs_prompt_file(Path::new("claude.bat")));
        assert!(!needs_prompt_file(Path::new(r"C:\bin\claude.exe")));
        assert!(!needs_prompt_file(Path::new("/usr/local/bin/claude")));
        assert!(!needs_prompt_file(Path::new("claude")));
    }
}
