# Gamification Modu Implementation Plan (Roadmap #8)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** `2026-08-15-progress-stats` VE `2026-08-15-mock-exam` planları MERGE EDİLMİŞ olmalı (history.rs streak API'si + /exam boss fight). Spec: `docs/superpowers/specs/2026-08-15-gamification-design.md` — önce oku.

**Goal:** `/game on|off` kalıcı toggle (USER.md `## Tercihler`, kabuk-yönetimli); açıkken TEACHING.md kurallarıyla XP/seviye/rozet anlatısı (süreç-puanlı, ADHD-safe, dozlu); açılışta `[GAME]` streak satırı; v0.17.0.

**Architecture:** Kabuk = toggle + kalıcılık + streak besleme; anlatı = TEACHING.md kuralları ("ince kabuk"). `/watch` intercept deseni; `opening_prompt`'a opsiyonel `[GAME]` bloğu.

**Tech Stack:** Rust, yeni bağımlılık YOK.

## Global Constraints

- Prompt/UI İngilizce; `## Tercihler` başlığı ve seviye adları (Çırak/Kalfa/Usta...) Türkçe (dosya/kimlik konvansiyonu).
- ADHD-safe: hiçbir yüzeyde "current streak: 0" / ceza mekaniği / puan kaybı yok.
- `set_game_pref` USER.md'nin DİĞER içeriğine dokunmaz; idempotent (iki kez on → tek satır).
- Her task sonunda commit (Türkçe mesaj) + push + `cargo test` yeşil.

---

### Task 1: Komut + kalıcılık (`src/main.rs`)

**Interfaces (Produces):** `enum GameCmd { On, Off, Status }` · `parse_game_command(&str) -> Option<GameCmd>` · `game_pref(global: &Path) -> bool` · `set_game_pref(global: &Path, on: bool) -> Result<()>`

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn parse_game_command_variants() {
    assert!(matches!(parse_game_command("/game on"), Some(GameCmd::On)));
    assert!(matches!(parse_game_command(" /game OFF "), Some(GameCmd::Off)));
    assert!(matches!(parse_game_command("/game"), Some(GameCmd::Status)));
    assert!(parse_game_command("/game x").is_none());
    assert!(parse_game_command("/gamer").is_none());
    assert!(parse_game_command("game on").is_none());
}

#[test]
fn game_pref_roundtrip_idempotent_preserves_user_md() {
    let base = std::env::temp_dir().join(format!("usta_game_pref_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(&base).unwrap();
    std::fs::write(base.join("USER.md"), "# Öğrenci Profili\n\n## Kim\n- Anil\n").unwrap();

    assert!(!game_pref(&base)); // default off
    set_game_pref(&base, true).unwrap();
    assert!(game_pref(&base));
    set_game_pref(&base, true).unwrap(); // idempotent
    let c = std::fs::read_to_string(base.join("USER.md")).unwrap();
    assert_eq!(c.matches("- gamification:").count(), 1);
    assert!(c.contains("## Kim")); // diğer içerik korunur
    assert!(c.contains("## Tercihler"));
    set_game_pref(&base, false).unwrap();
    assert!(!game_pref(&base));

    let _ = std::fs::remove_dir_all(&base);
}
```

- [ ] **Step 2:** Run: `cargo test game` → derleme hatası

- [ ] **Step 3: Implement**

```rust
#[derive(Debug)]
pub(crate) enum GameCmd { On, Off, Status }

pub(crate) fn parse_game_command(line: &str) -> Option<GameCmd> {
    let t = line.trim();
    if t == "/game" {
        return Some(GameCmd::Status);
    }
    let rest = t.strip_prefix("/game ")?;
    match rest.trim().to_ascii_lowercase().as_str() {
        "on" => Some(GameCmd::On),
        "off" => Some(GameCmd::Off),
        _ => None,
    }
}

/// Shell-managed preference line in USER.md (`## Tercihler` section).
/// The closing flush is told to keep this section as-is.
pub(crate) fn game_pref(global: &Path) -> bool {
    std::fs::read_to_string(global.join("USER.md"))
        .map(|c| c.lines().any(|l| l.trim() == "- gamification: on"))
        .unwrap_or(false)
}

pub(crate) fn set_game_pref(global: &Path, on: bool) -> Result<()> {
    let path = global.join("USER.md");
    let content = std::fs::read_to_string(&path).unwrap_or_default();
    let value = if on { "- gamification: on" } else { "- gamification: off" };
    let new = if content.lines().any(|l| l.trim().starts_with("- gamification:")) {
        content
            .lines()
            .map(|l| if l.trim().starts_with("- gamification:") { value } else { l })
            .collect::<Vec<_>>()
            .join("\n")
    } else if content.contains("## Tercihler") {
        content.replace("## Tercihler", &format!("## Tercihler\n{value}"))
    } else {
        format!("{}\n\n## Tercihler\n{value}\n", content.trim_end())
    };
    progress::write_atomic(&path, &new)
}
```

(Satır-değiştirme yolunda dosya sonundaki `\n` kaybolabilir — test buna takılırsa `+ "\n"` ekle; davranışı testle kilitle.)

- [ ] **Step 4:** Run: `cargo test game` → PASS

- [ ] **Step 5: Commit + push**

```bash
git add src/main.rs
git commit -m "oyun: /game komutu + USER.md Tercihler kalıcılığı (kabuk-yönetimli)"
git push
```

---

### Task 2: Döngü entegrasyonu (TUI + plain)

**Files:** Modify: `src/tui/run.rs` + `src/main.rs` (`run_plain_loop`) — `/watch` intercept'lerinin yanı

- [ ] **Step 1: Implement** — her iki döngüde:

```rust
                        if let Some(cmd) = crate::parse_game_command(&line) {
                            // TUI: page_user_echo; plain: echo zaten var
                            match cmd {
                                crate::GameCmd::Status => {
                                    let s = if crate::game_pref(&global) { "gamification is on" } else { "gamification is off" };
                                    // notice(s); LLM'e gitmez
                                }
                                crate::GameCmd::On | crate::GameCmd::Off => {
                                    let on = matches!(cmd, crate::GameCmd::On);
                                    if let Err(e) = crate::set_game_pref(&global, on) {
                                        // notice(hata); continue
                                    }
                                    // notice: on → "gamification on — XP, levels and badges are live"
                                    //         off → "gamification off — back to quiet mode"
                                    // oturuma bilgi turu enjekte edilir ve NORMAL ask akışıyla gönderilir:
                                    // on → "[GAME MODE ON] Gamification is now ON — apply the Gamification rules from TEACHING.md from this point on."
                                    // off → "[GAME MODE OFF] Gamification is now OFF — stop all game narration."
                                }
                            }
                            continue; // Status yolu; On/Off yolunda enjeksiyon akışına göre düzenle
                        }
```

`/exam`'daki enjeksiyon yaklaşımının aynısı (satırı prompt'la değiştirip normal akışa bırakma tercih edildiyse burada da onu uygula — tutarlılık). `global` scope'ta yoksa parametre zincirinden getir (koda bak).

- [ ] **Step 2:** `cargo build && cargo test` → PASS. Commit + push:

```bash
git add src/tui/run.rs src/main.rs
git commit -m "oyun: /game intercept — toggle + oturum-içi mod bildirimi (TUI + plain)"
git push
```

---

### Task 3: Açılış `[GAME]` beslemesi + kapanış koruması

**Files:** Modify: `src/progress.rs` (`opening_prompt` + `closing_prompt`), çağrı yerleri (`src/tui/run.rs`, `src/main.rs`)

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn opening_prompt_carries_game_streak_block() {
    let s = opening_prompt("rust", false, false, Some("streak: 3 day(s) (longest 6)"));
    assert!(s.contains("[GAME] streak: 3 day(s) (longest 6)"));
    let s = opening_prompt("rust", false, false, None);
    assert!(!s.contains("[GAME]"));
}

#[test]
fn closing_prompt_protects_tercihler_section() {
    let s = closing_prompt("rust", None, None, None, None, None, None);
    assert!(s.contains("## Tercihler"));
    assert!(s.contains("shell-managed"));
}
```

- [ ] **Step 2:** Run → derleme hatası (opening_prompt parametre)

- [ ] **Step 3: Implement**

- `opening_prompt` imzasına `game_streak: Option<&str>`; Some → format sonuna `\n[GAME] {s}\n`.
- `closing_prompt` profile kuralına ek cümle: `KEEP the '## Tercihler' section (e.g. '- gamification: on') exactly as-is — it is shell-managed.`
- Çağrı yerleri (TUI + plain, opening dalı): `game_pref(&global)` true ise history oku (`learner/history.md`), `history::entries` → `current_streak`/`longest_streak` → string: streak>0 → `streak: N day(s) (longest M)`; streak==0 && longest>0 → `longest streak: M day(s)`; kayıt yok → None. False ise None. `onboarding_prompt`'a dokunma.
- Mevcut `opening_prompt` test çağrılarına `None` ekle.

- [ ] **Step 4:** `cargo test` → PASS. Commit + push:

```bash
git add src/progress.rs src/tui/run.rs src/main.rs
git commit -m "oyun: açılış [GAME] streak beslemesi + Tercihler kapanış koruması"
git push
```

---

### Task 4: TEACHING.md kuralları + help + docs + v0.17.0

**Files:** `TEACHING.md`, `src/help.rs`, `SPEC.md`, `README.md`, `docs/ROADMAP.md`, `Cargo.toml`(+lock), sürüm testi

- [ ] **Step 1: TEACHING.md — `## Gamification` bölümü** (spec §6 içeriği birebir — aktivasyon koşulu, XP tablosu, seviye eşikleri 0/100/250/500/1000/2000 (Çırak→Kalfa Adayı→Kalfa→Usta Çırağı→Usta Adayı→Usta), rozetler, boss fight, DOZ kuralı, overjustification bekçisi, streak-utandırma yasağı).

- [ ] **Step 2: help.rs** — In-session commands: `/game on|off      XP, levels, badges (ADHD-safe)` (+ test güncelle).

- [ ] **Step 3:** SPEC yeni § (v0.17) · README Highlights (İngilizce): `| 🎮 **Gamification (opt-in)** | /game on and Usta narrates XP, levels (Çırak → Usta) and badges — points reward the process (showing up, predicting, submitting exercises), never punish. Mock exams become boss fights. ADHD-safe: a broken streak shows your longest, not a guilt trip. |` · ROADMAP #8 `✅ tamamlandı (2026-08-15)` + Tamamlananlar.

- [ ] **Step 4:** Cargo `0.17.0`; sürüm testi; `cargo build`.

- [ ] **Step 5:** Verify: `cargo test` PASS · clippy yeni uyarı 0 · `cargo install --path .`.

```bash
git add TEACHING.md src/help.rs SPEC.md README.md docs/ROADMAP.md Cargo.toml Cargo.lock src/
git commit -m "oyun: TEACHING kuralları + help + SPEC + README + roadmap #8 kapandı — v0.17.0"
git push
git tag v0.17.0 && git push --tags
```

- [ ] **Step 6 (elle doğrulama — ATLA, Anil koşacak):** `/game on` → onay + oturumda oyun anlatısı başlamalı; kapanış-açılış sonrası tercih kalıcı; açılışta `[GAME]` streak satırı etkisi; `/game off` → sessiz mod; USER.md'de `## Tercihler` tek satır.
