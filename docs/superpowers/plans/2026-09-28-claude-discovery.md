# Claude Discovery — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** usta `claude`'u her işletim sisteminde kendi bulsun (PATH + OS'e göre dosya adı + bilinen kurulum klasörleri) ve bulduğu tam yolla çağırsın; Windows'ta `USTA_BACKEND` şartı kalksın.

**Architecture:** Yeni küçük modül `src/claude_bin.rs` (keşif + `.cmd` kararı, saf ve test edilebilir). `src/backend.rs` bunu kullanır: `Backend::Cli`'ye `bin: PathBuf` alanı, `run_claude_cli` bu yolu çağırır; `.cmd`/`.bat` ise sistem talimatı geçici dosyadan (`--append-system-prompt-file`).

**Tech Stack:** Rust, `dirs` 5 (mevcut bağımlılık), tokio `Command`.

**Spec:** `docs/superpowers/specs/2026-09-28-claude-discovery-design.md` — önce oku.

**Ön-koşul notu:** `main`, v0.31.3, temiz ağaç. İş dalı: `git switch -c claude-discovery`. `src/backend.rs` 530 satır (bütçe 600) — keşif kodu bu yüzden ayrı modülde.

## Global Constraints

- Keşif sırası: `USTA_CLAUDE` (dosyaysa) → PATH → bilinen klasörler. Her klasörde adlar sırayla: Unix `["claude"]`, Windows `["claude.exe", "claude.cmd"]`.
- Bilinen klasörler: `~/.local/bin`, `~/.claude/local`; Windows ek `dirs::data_dir()/npm`; Unix ek `/opt/homebrew/bin`, `/usr/local/bin`.
- `.exe` ve Unix çağrısı DEĞİŞMEZ: `--append-system-prompt <metin>`. Sadece `.cmd`/`.bat` (büyük/küçük harf duyarsız) → `--append-system-prompt-file <geçici dosya>`, çağrı bitince silinir.
- `USTA_BACKEND=cli` + claude bulunamadı → CLI yine seçilir, `bin = "claude"` (bugünkü davranış).
- Dokunulmayanlar: araç listesi (`CLI_ALLOWED_TOOLS`), `--setting-sources ""`, API backend, wizard metni, TUI.
- `cargo fmt -- <düzenlenen dosyalar>` (crate genelinde değil). Dosya bütçesi 600 satır.
- Bilinen ortam hatası, düzeltilmez: `materials::tests::convert_pdfs_missing_tool_reports_notice_and_no_txt`.
- Push yok, merge yok (controller yapar).

---

### Task 1: `src/claude_bin.rs` — keşif modülü

**Files:**
- Create: `src/claude_bin.rs`
- Modify: `src/main.rs` (`mod check;` satırının altına `mod claude_bin;`)

- [ ] **Step 1: Modülü testleriyle yaz — önce sadece testler + boş gövdeler değil, aşağıdaki tam dosya; ama önce testleri kırmızı görmek için `find_claude_in` gövdesini geçici olarak `None` döndür, `needs_prompt_file` gövdesini `false` döndür.**

`src/claude_bin.rs` (nihai hâli):

```rust
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
        let d = std::env::temp_dir().join(format!(
            "usta-claude-bin-{}-{name}",
            std::process::id()
        ));
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
        assert_eq!(find_claude_in(None, Some(path), &[], UNIX), Some(b.join("claude")));
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
        assert_eq!(find_claude_in(None, os(&d), &[], WIN), Some(d.join("claude.exe")));
    }

    #[test]
    fn windows_npm_cmd_found_in_known_dir() {
        let npm = scratch("win-npm");
        touch(&npm.join("claude.cmd"));
        assert_eq!(find_claude_in(None, None, &[npm.clone()], WIN), Some(npm.join("claude.cmd")));
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
```

`src/main.rs`: `mod check;` satırının hemen altına `mod claude_bin;`.

- [ ] **Step 2: Kırmızı gör**

Geçici gövdelerle (`find_claude_in` → `None`, `needs_prompt_file` → `false`):
Run: `cargo test claude_bin`
Expected: pozitif sonuç bekleyen testler FAIL (`explicit_path_wins_over_path`, `missing_explicit…`, `found_in_second_path_dir`, `known_dir…`, `windows_names…`, `windows_npm…`, `prompt_file_only_for_batch_shims`); `a_directory_named_claude_is_not_a_hit` ve `nothing_anywhere_is_none` geçer (bu beklenen).

(`find_claude` / `names` / `known_dirs` henüz çağrılmıyor → dead-code uyarısı bu task'ta beklenir; Task 2'de kalkar. `#[allow]` EKLEME.)

- [ ] **Step 3: Gerçek gövdeleri koy, yeşil gör**

Run: `cargo test claude_bin`
Expected: 9/9 PASS.

- [ ] **Step 4: fmt + suite**

Run: `cargo fmt -- src/claude_bin.rs src/main.rs && cargo test`
Expected: yalnız bilinen `materials` hatası. (Clippy dead-code uyarıları Task 2'de kapanır; bu task'ta clippy'yi çalıştır ve SADECE dead-code uyarıları olduğunu raporla.)

- [ ] **Step 5: Commit**

```bash
git add src/claude_bin.rs src/main.rs
git commit -m "feat: claude_bin — find the claude executable on any OS"
```

---

### Task 2: `backend.rs` — bulunan yolu kullan

**Files:**
- Modify: `src/backend.rs` (`Backend::Cli`, `select`, `cli_backend`, `claude_on_path` silinir, `complete`'in CLI kolu, `run_claude_cli`, test modülü), `src/file_feedback.rs` (tek test kurulumu, ~satır 1287)

- [ ] **Step 1: Failing test**

`src/backend.rs` test modülüne ekle:

```rust
    #[test]
    fn cli_call_uses_a_prompt_file_for_batch_shims() {
        // Source pin (the args live inside an async spawn fn): the `.cmd`
        // branch must hand the system prompt over as a file, the default
        // branch keeps passing it inline.
        let src = include_str!("backend.rs");
        let production = src.split("#[cfg(test)]").next().unwrap();
        assert!(production.contains("needs_prompt_file(bin)"));
        assert!(production.contains("--append-system-prompt-file"));
        assert!(production.contains("\"--append-system-prompt\""));
        assert!(!production.contains("fn claude_on_path"), "replaced by claude_bin::find_claude");
    }
```

Run: `cargo test cli_call_uses_a_prompt_file` → FAIL.

- [ ] **Step 2: `Backend::Cli`'ye alan**

```rust
    Cli {
        model: String,
        session_id: Option<String>,
        /// Resolved `claude` executable (`claude_bin::find_claude`), or the
        /// bare name `claude` when `USTA_BACKEND=cli` forced it unfound.
        bin: PathBuf,
    },
```

Dosya başına `use std::path::{Path, PathBuf};` ekle (mevcut `use` bloğuna).

- [ ] **Step 3: Seçim**

`select()` içinde:

```rust
        Some("cli") => Ok(cli_backend(
            crate::claude_bin::find_claude().unwrap_or_else(|| PathBuf::from("claude")),
        )),
```

`None =>` kolunda `if claude_on_path() { Ok(cli_backend()) }` →

```rust
            if let Some(bin) = crate::claude_bin::find_claude() {
                Ok(cli_backend(bin))
            } else if …  // (API-key kolu aynen kalır)
```

`cli_backend`:

```rust
fn cli_backend(bin: PathBuf) -> Backend {
    Backend::Cli {
        model: DEFAULT_CLI_MODEL.to_string(),
        session_id: None,
        bin,
    }
}
```

`claude_on_path` fonksiyonunu ve doc comment'ini SİL. Modül doc comment'indeki seçim satırını (`otherwise CLI if \`claude\` is on PATH`) → `otherwise CLI if \`claude\` is found (PATH or its install folders, see \`claude_bin\`)` olarak güncelle.

- [ ] **Step 4: Çağrı**

`complete`'in CLI kolu: `Backend::Cli { model, session_id } =>` → `Backend::Cli { model, session_id, bin } =>`; iki `run_claude_cli(` çağrısının ilk argümanı `bin` olur: `run_claude_cli(bin, model, system, …)`.

`run_claude_cli`'nin üstüne:

```rust
/// Deletes the system-prompt file when the call ends (success, error or spawn failure).
struct PromptFile(PathBuf);

impl Drop for PromptFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
```

`run_claude_cli` imzası ve başı:

```rust
async fn run_claude_cli(
    bin: &Path,
    model: &str,
    system: &str,
    input: &str,
    resume: Option<&str>,
) -> Result<(String, Option<String>, Option<u64>)> {
    let mut cmd = Command::new(bin);
    cmd.arg("-p").arg("--output-format").arg("json");
    // `.cmd`/`.bat` (npm on Windows) run through cmd.exe, where the multi-line
    // system prompt isn't a safe argument → hand it over as a file instead.
    let _prompt_file = if crate::claude_bin::needs_prompt_file(bin) {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("usta-system-{}-{n}.md", std::process::id()));
        std::fs::write(&path, system).context("failed to write the system prompt file")?;
        cmd.arg("--append-system-prompt-file").arg(&path);
        Some(PromptFile(path))
    } else {
        cmd.arg("--append-system-prompt").arg(system);
        None
    };
    cmd.arg("--model")
        .arg(model)
        .arg("--allowedTools")
        .arg(CLI_ALLOWED_TOOLS)
        // (mevcut --setting-sources yorumu + argümanları AYNEN kalır)
```

(Eski zincirdeki `.arg("-p") … .arg("--append-system-prompt").arg(system)` kısmı yukarıdakiyle değişir; `--model`'den sonrası aynen.)

Spawn hata mesajı: `.context("`claude` CLI failed to start — is it on PATH?")` →

```rust
        .with_context(|| format!("`{}` failed to start", bin.display()))?;
```

- [ ] **Step 5: Test kurulumlarını uyarla**

`Backend::Cli { … }` kuran her test (backend.rs'te 3 yer, `src/file_feedback.rs` ~1287'de 1 yer): `bin: PathBuf::from("claude"),` alanını ekle (file_feedback'te gerekirse `std::path::PathBuf` tam yoluyla). `let Backend::Cli { session_id, .. }` desenleri `..` sayesinde değişmez.

- [ ] **Step 6: Yeşil + suite + clippy**

Run: `cargo fmt -- src/backend.rs src/file_feedback.rs && cargo test && cargo clippy --all-targets`
Expected: yalnız bilinen `materials` hatası; clippy 0 uyarı (Task 1'in dead-code uyarıları dahil kapandı). `wc -l src/backend.rs` < 600.

- [ ] **Step 7: Mac'te gerçek çağrı (controller koşar, implementer değil)** — atla, controller yapar.

- [ ] **Step 8: Commit**

```bash
git add src/backend.rs src/file_feedback.rs
git commit -m "feat: CLI backend calls the discovered claude path; .cmd shims get the prompt from a file"
```

---

### Task 3: Doküman + dağıtım notları + sürüm

**Files:**
- Modify: `Cargo.toml`, `Cargo.lock`, `src/tui/welcome_tests.rs`, `SPEC.md` (§6 Selection satırı), `docs/ROADMAP.md`, `README.md` (Install → Windows paragrafı), `packaging/windows/README.txt`, `packaging/windows/OKUBENI.txt`

- [ ] **Step 1: Sürüm** — `0.31.3` → `0.31.4` (Cargo.toml + `version_aligned_with_spec` pini), `cargo check`.

- [ ] **Step 2: SPEC §6** — `  - Selection: \`USTA_BACKEND=cli|api\` takes priority; otherwise \`claude\` on PATH → CLI, otherwise if a key exists → API.` satırını şununla değiştir:

```markdown
  - Selection: `USTA_BACKEND=cli|api` takes priority; otherwise `claude` found → CLI, otherwise if a key exists → API. **Finding `claude` (v0.31.4, `src/claude_bin.rs`):** `USTA_CLAUDE` (explicit path) → PATH with the platform's names (Unix `claude`; Windows `claude.exe`, then `claude.cmd`) → install folders (`~/.local/bin`, `~/.claude/local`, Windows `%APPDATA%\npm`, Unix `/opt/homebrew/bin`, `/usr/local/bin`). The resolved path is what gets spawned. A `.cmd`/`.bat` shim runs through cmd.exe, where the multi-line system prompt isn't a safe argument, so it gets `--append-system-prompt-file <temp>` instead (deleted after the call); `.exe` and Unix keep `--append-system-prompt`. Before this, Windows never found `claude` and needed `USTA_BACKEND=cli`.
```

- [ ] **Step 3: ROADMAP** — `## Completed` altına, ilk maddenin ÜSTÜNE (maddeler arası boş satır):

```markdown
- 2026-09-28: Claude discovery (v0.31.4) — Usta finds `claude` on any OS: PATH with the platform's file names plus the folders Claude Code installs into, `USTA_CLAUDE` as an override. Windows no longer needs `USTA_BACKEND=cli`; npm's `claude.cmd` gets the system prompt from a temp file.
```

- [ ] **Step 4: README Windows paragrafı** — şu cümleyi:

```
On Windows, Usta doesn't find `claude` on its own yet: set `setx USTA_BACKEND "cli"` (Claude Code installed with the native Windows installer) or `setx ANTHROPIC_API_KEY "sk-ant-..."`, then reopen the terminal.
```

şununla değiştir:

```
If Claude Code is installed, Usta finds it on its own; otherwise set `setx ANTHROPIC_API_KEY "sk-ant-..."` and reopen the terminal. If `claude` lives somewhere unusual, point Usta at it: `setx USTA_CLAUDE "C:\path\to\claude.exe"`.
```

- [ ] **Step 5: Paket notları**

`packaging/windows/README.txt` — 3. adımın B şıkkını (`B) Claude Code, installed with …` bloğunun tamamı, "line is required." dahil) şununla değiştir:

```
   B) Claude Code installed — Usta finds it on its own, nothing to set.
      If it isn't found (unusual install folder), point Usta at it:
        setx USTA_CLAUDE "C:\path\to\claude.exe"
```

`packaging/windows/OKUBENI.txt` — 3. adımın B şıkkını (`B) Claude Code yüklüyse …` bloğunun tamamı, "bu satır şart." dahil) şununla değiştir:

```
   B) Claude Code yüklüyse — usta kendisi bulur, bir şey ayarlaman gerekmez.
      Bulamazsa (alışılmadık bir klasöre kurulduysa) yerini göster:
        setx USTA_CLAUDE "C:\...\claude.exe"
```

Aynı dosyadaki `Yeni: bir link verirsen …` satırının hemen ÜSTÜNE yeni satır ekle: `Yeni: Claude Code'u artık kendisi buluyor — USTA_BACKEND ayarına gerek yok (önceden yaptıysan kalabilir, zararı yok).` (link satırı kalır).

- [ ] **Step 6: Suite + commit**

Run: `cargo test && cargo clippy --all-targets` → yalnız bilinen `materials` hatası.

```bash
git add Cargo.toml Cargo.lock src/tui/welcome_tests.rs SPEC.md docs/ROADMAP.md README.md packaging/windows/README.txt packaging/windows/OKUBENI.txt
git commit -m "docs: SPEC + ROADMAP + install notes — claude discovery; bump to v0.31.4"
```

- [ ] **Step 7: Release (controller)** — merge main + push + `cargo install --path .` + Windows zip + macOS tar.gz + `gh release create v0.31.4` (bkz. hafızadaki release zinciri). Mac'te gerçek `usta` çağrısıyla cevap geldiğini doğrula. Windows doğrulaması Anil'in eşinde: `USTA_BACKEND` silinmiş hâlde `usta.exe start` cevap veriyor.
