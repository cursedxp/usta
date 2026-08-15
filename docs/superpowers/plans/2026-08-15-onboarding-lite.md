# Onboarding-Lite Implementation Plan (Roadmap #4 — ilk yarı)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** Çakışan bekleyen plan yok. Spec: `docs/superpowers/specs/2026-08-15-onboarding-lite-design.md` — önce oku.

**Goal:** Backend yokken TTY'de yönlendiren sihirbaz (Enter=re-check, key yapıştır, q=çık; kurulunca aynı süreçte devam); Cargo.toml 0.13.0; `v0.13.0` tag.

**Architecture:** Saf çekirdek (`wizard_action`, `wizard_guidance`) + ince interaktif kabuk (`run_backend_wizard` — stdin loop). `main.rs:60`'taki `backend::select()?` yalnız TTY'de sihirbaza düşer. Girilen key süreç env'ine yazılır, diske asla.

**Tech Stack:** Rust (mevcut crate; `std::io::IsTerminal` — yeni bağımlılık YOK).

## Global Constraints

- Sihirbaz metinleri İngilizce. Key değeri hiçbir çıktıya/log'a yazılmaz; diske kalıcılaştırma YOK.
- TTY-değil ve `USTA_BACKEND` geçersiz-değer yolları mevcut `bail!` davranışını BİREBİR korur.
- Her task sonunda commit (Türkçe mesaj) + push + `cargo test` yeşil.

---

### Task 1: Sihirbaz saf çekirdeği (`src/backend.rs`)

**Files:**
- Modify: `src/backend.rs` (dosya sonu, test modülünün üstü)
- Test: `src/backend.rs` in-module tests

**Interfaces:**
- Produces: `pub enum WizardAction { Recheck, Quit, Key(String), Invalid }`; `pub fn wizard_action(input: &str) -> WizardAction`; `pub fn wizard_guidance() -> &'static str`. Task 2 kullanır.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn wizard_action_interprets_inputs() {
    assert!(matches!(wizard_action(""), WizardAction::Recheck));
    assert!(matches!(wizard_action("   "), WizardAction::Recheck));
    assert!(matches!(wizard_action("q"), WizardAction::Quit));
    assert!(matches!(wizard_action("Q"), WizardAction::Quit));
    assert!(matches!(wizard_action(" quit "), WizardAction::Quit));
    match wizard_action("  sk-ant-abc123  ") {
        WizardAction::Key(k) => assert_eq!(k, "sk-ant-abc123"),
        other => panic!("expected Key, got {other:?}"),
    }
    assert!(matches!(wizard_action("hello"), WizardAction::Invalid));
}

#[test]
fn wizard_guidance_names_both_paths() {
    let g = wizard_guidance();
    assert!(g.contains("claude.com/claude-code"));
    assert!(g.contains("ANTHROPIC_API_KEY"));
    assert!(g.contains("sk-ant-"));
    assert!(g.contains("q to quit"));
}
```

(`WizardAction`'a `#[derive(Debug)]` gerekir — panic mesajı için.)

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test wizard`
Expected: derleme hatası — tipler yok

- [ ] **Step 3: Write minimal implementation**

```rust
/// First-run wizard: what the user typed at the prompt (spec: onboarding-lite).
#[derive(Debug)]
pub enum WizardAction {
    /// Empty line — try `select()` again (they installed Claude Code meanwhile).
    Recheck,
    /// `q` / `quit` — leave with a clean message.
    Quit,
    /// A pasted API key (`sk-ant-...`), already trimmed. NEVER printed or persisted.
    Key(String),
    /// Anything else — re-prompt.
    Invalid,
}

pub fn wizard_action(input: &str) -> WizardAction {
    let t = input.trim();
    if t.is_empty() {
        return WizardAction::Recheck;
    }
    if t.eq_ignore_ascii_case("q") || t.eq_ignore_ascii_case("quit") {
        return WizardAction::Quit;
    }
    if t.starts_with("sk-ant-") {
        return WizardAction::Key(t.to_string());
    }
    WizardAction::Invalid
}

pub fn wizard_guidance() -> &'static str {
    "No LLM backend found. Usta needs one of these:\n\n\
     \x20 1. Claude Code CLI (recommended — uses your subscription, no API key)\n\
     \x20    Install: https://claude.com/claude-code   (then just press Enter here)\n\n\
     \x20 2. Anthropic API key\n\
     \x20    Paste it below (starts with sk-ant-...), or add to your shell first:\n\
     \x20    export ANTHROPIC_API_KEY=sk-ant-...\n\n\
     Press Enter to re-check · paste your API key · or type q to quit"
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test wizard`
Expected: PASS

- [ ] **Step 5: Commit + push**

```bash
git add src/backend.rs
git commit -m "onboarding: sihirbaz çekirdeği — wizard_action + wizard_guidance"
git push
```

---

### Task 2: İnteraktif döngü + main entegrasyonu

**Files:**
- Modify: `src/backend.rs` (`run_backend_wizard`), `src/main.rs:60` (`backend::select()` çağrısı)

**Interfaces:**
- Consumes: Task 1 çekirdeği + mevcut `select()`.
- Produces: `pub fn run_backend_wizard() -> Result<Backend>` — TTY loop; `main` entegrasyonu.

- [ ] **Step 1: Implement `run_backend_wizard`**

`src/backend.rs`:

```rust
use std::io::{BufRead, IsTerminal, Write};

/// Interactive fallback when `select()` finds no backend and we're on a TTY.
/// Loops: Enter = re-check, pasted key = set process env + re-check, q = quit.
/// The key is only written to this process's environment — never to disk, and
/// never echoed back.
pub fn run_backend_wizard() -> Result<Backend> {
    println!("\n{}", wizard_guidance());
    let stdin = std::io::stdin();
    loop {
        print!("> ");
        std::io::stdout().flush().ok();
        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 {
            bail!("no backend configured (EOF)");
        }
        match wizard_action(&line) {
            WizardAction::Quit => bail!("no backend configured — run usta again once one is set up"),
            WizardAction::Key(k) => {
                std::env::set_var("ANTHROPIC_API_KEY", k);
                match select() {
                    Ok(b) => {
                        println!("backend found: {}", b.label());
                        println!("tip: add `export ANTHROPIC_API_KEY=...` to your shell profile to skip this next time");
                        return Ok(b);
                    }
                    Err(e) => println!("still no backend: {e}"),
                }
            }
            WizardAction::Recheck => match select() {
                Ok(b) => {
                    println!("backend found: {}", b.label());
                    return Ok(b);
                }
                Err(e) => println!("still no backend: {e}"),
            },
            WizardAction::Invalid => {
                println!("didn't catch that — Enter to re-check, paste an sk-ant-... key, or q to quit");
            }
        }
    }
}
```

Not (koda bak): `select()`'in `USTA_BACKEND` geçersiz-değer `bail!`'i sihirbaz İÇİNDE de aynı hatayı basar ve döngü devam eder — kabul; ama `main` tarafında sihirbaza HİÇ girilmemesi gerekir (Step 2 koşulu).

- [ ] **Step 2: `main.rs` entegrasyonu**

`main.rs:60` — eski:
```rust
    let mut backend = backend::select()?;
```
yeni:
```rust
    let mut backend = match backend::select() {
        Ok(b) => b,
        // Config error (bad USTA_BACKEND value) is not "no backend" — surface it.
        Err(e) if std::env::var_os("USTA_BACKEND").is_some() => return Err(e),
        Err(e) => {
            use std::io::IsTerminal;
            if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
                backend::run_backend_wizard()?
            } else {
                return Err(e);
            }
        }
    };
```

- [ ] **Step 3: Compile + full tests**

Run: `cargo build && cargo test`
Expected: derleme temiz, tümü PASS (loop TTY-only — unit test yok, mantık Task 1'de)

- [ ] **Step 4: Commit + push**

```bash
git add src/backend.rs src/main.rs
git commit -m "onboarding: backend yoksa TTY sihirbazı — re-check / key yapıştır / çık"
git push
```

---

### Task 3: Sürüm 0.13.0 + politika + docs

**Files:**
- Modify: `Cargo.toml` (`version = "0.13.0"`), `docs/ROADMAP.md`, `SPEC.md`, `README.md`

- [ ] **Step 1: Cargo.toml** → `version = "0.13.0"`; `cargo build` (Cargo.lock güncellenir, commit'e dahil et).

- [ ] **Step 2: Sürüm assert testi** — welcome testlerinin yanına:

```rust
#[test]
fn version_aligned_with_spec() {
    assert_eq!(env!("CARGO_PKG_VERSION"), "0.13.0");
}
```

- [ ] **Step 3: ROADMAP** — başlık notuna ekle: `Sürümleme: her tamamlanan roadmap maddesi minor bump; tag vX.Y.Z.` · #4 satırı: durum `kısmen ✅ (2026-08-15: onboarding-lite + 0.13.0; dağıtım/brew ertelendi)`.

- [ ] **Step 4: SPEC.md** — yeni § (sıradaki numara): onboarding-lite sihirbazı (tetik koşulu: select Err + TTY + USTA_BACKEND yok; akış; key süreç-env-only) + sürüm politikası.

- [ ] **Step 5: README.md (İngilizce)** — Install bölümüne: `First run with no backend? Usta walks you through it — install Claude Code or paste an API key, no restart needed.`

- [ ] **Step 6: Verify + commit + push + tag**

Run: `cargo test` → PASS · `cargo clippy --all-targets` → yeni uyarı 0 · `cargo install --path .` → başarılı

```bash
git add Cargo.toml Cargo.lock docs/ROADMAP.md SPEC.md README.md src/
git commit -m "onboarding: v0.13.0 hizası + sürüm politikası + SPEC/README"
git push
git tag v0.13.0 && git push --tags
```

- [ ] **Step 7 (elle doğrulama — ATLA, Anil koşacak):** `USTA_BACKEND= PATH=/usr/bin:/bin ANTHROPIC_API_KEY= usta` gibi backend'siz ortamda sihirbaz açılmalı; Enter re-check çalışmalı; `q` temiz çıkmalı; `echo | usta` (pipe) sihirbaza girmeden mevcut hatayı basmalı.
