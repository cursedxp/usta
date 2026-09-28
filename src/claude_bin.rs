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

/// Locate `claude` from the real environment. If `USTA_CLAUDE` is set but
/// isn't a file, warns once to stderr and falls through to PATH / known
/// dirs exactly as if it weren't set.
pub fn find_claude() -> Option<PathBuf> {
    let explicit = std::env::var_os("USTA_CLAUDE");
    if let Some(p) = &explicit {
        let path = PathBuf::from(p);
        if !path.is_file() {
            eprintln!("{}", ignoring_bad_env_warning(&path));
        }
    }
    find_claude_in(explicit, std::env::var_os("PATH"), &known_dirs(), names())
}

/// Wording for `find_claude()`'s stderr warning — split out so it's
/// unit-testable without touching real env vars.
fn ignoring_bad_env_warning(path: &Path) -> String {
    format!(
        "usta: USTA_CLAUDE is set but \"{}\" is not a file — ignoring it",
        path.display()
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

/// Whether the system prompt must be handed to `claude` via a temp file
/// (`--append-system-prompt-file`) instead of inline (`--append-system-prompt
/// <text>`). Two independent reasons force this:
/// - **Windows, always** (`windows` = `cfg!(windows)`): `CreateProcessW` caps
///   the whole command line at 32,767 UTF-16 chars, and usta's system prompt
///   is already ~25K chars and growing — this applies to every spawn,
///   including `claude.exe`, not just shims.
/// - **`.cmd` / `.bat`, on any OS:** these run through `cmd.exe`, which can't
///   safely take a multi-line argument at all, regardless of length.
pub fn prompt_via_file(bin: &Path, windows: bool) -> bool {
    windows
        || bin
            .extension()
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
        let got = find_claude_in(None, os(&empty), std::slice::from_ref(&local_bin), UNIX);
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
            find_claude_in(None, None, std::slice::from_ref(&npm), WIN),
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
        assert_eq!(
            find_claude_in(None, os(&d), std::slice::from_ref(&d), WIN),
            None
        );
        assert_eq!(find_claude_in(None, None, &[], UNIX), None);
    }

    #[test]
    fn prompt_via_file_on_windows_always_true_else_only_batch_shims() {
        // Windows: every spawn goes through the file (command-line cap),
        // exe included.
        assert!(prompt_via_file(Path::new(r"C:\bin\claude.exe"), true));
        assert!(prompt_via_file(Path::new("claude"), true));
        // Non-Windows: only `.cmd`/`.bat` shims need it.
        assert!(prompt_via_file(Path::new("x.cmd"), false));
        assert!(prompt_via_file(Path::new("x.CMD"), false));
        assert!(prompt_via_file(Path::new("x.bat"), false));
        assert!(!prompt_via_file(Path::new("x.exe"), false));
        assert!(!prompt_via_file(Path::new("/usr/local/bin/claude"), false));
    }

    #[test]
    fn ignoring_bad_env_warning_names_the_path_and_the_var() {
        let msg = ignoring_bad_env_warning(Path::new("/nope/claude"));
        assert!(msg.contains("USTA_CLAUDE"));
        assert!(msg.contains("/nope/claude"));
        assert!(msg.contains("ignoring"));
    }

    #[test]
    fn find_claude_warning_lives_in_the_env_wrapper_not_the_pure_core() {
        // Source pin: `find_claude()` (env-reading wrapper) must own the
        // stderr warning; `find_claude_in` (pure core) must not.
        let src = include_str!("claude_bin.rs");
        let (before_find_claude_in, rest) = src.split_once("pub fn find_claude_in").unwrap();
        assert!(before_find_claude_in.contains("eprintln!"));
        let pure_core = rest.split("#[cfg(test)]").next().unwrap();
        assert!(!pure_core.contains("eprintln!"));
    }
}
