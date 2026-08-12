# UX Fixes + English Base Language — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add multi-line input, single-Esc cancel, and a companion-mode toggle to Usta's TUI, and make the app's base language English (mirroring the user's language) — including brain files and code comments.

**Architecture:** Two session loops kept in parity — TUI (`src/tui/run.rs`, `tokio::select!` + `ask_live`) and plain (`src/main.rs::run_plain_loop`, rustyline). Behavior lives in editable markdown brain files loaded by `brain.rs`; Rust owns UI strings, prompt generators, terminal setup, and the input editor. Work is two phases: A = behavior (Tasks 1–5), B = mechanical translation (Tasks 6–7).

**Tech Stack:** Rust 2021, tokio, ratatui 0.29, crossterm 0.28, tui-input 0.11, rustyline 14.

## Global Constraints

- Newline fallback MUST work on every terminal: Ctrl+J (LF) is the universal binding; Shift+Enter/Alt+Enter are best-effort via kitty keyboard protocol. Bare Enter still submits.
- Single Esc cancels a live LLM turn; double Ctrl-C stays for quit. TUI only — plain mode keeps Ctrl-C.
- Companion (`watching`) defaults to **true** each session; no persistence. Slash forms: `/watch`, `/watch on`, `/watch off`. Slash lines are never sent to the LLM (echo + notice only).
- When `watching == false`, file batches MUST still be synced into `FileMemory` via `files.observe(...)` (no LLM call) so re-enabling never triggers a giant diff.
- Language: base English, mirror the user's language, soft preference (no hard rule). Copy this policy verbatim into SOUL.md (Task 6).
- `interpret_topic_input` resume-phrase keywords are LOGIC, not display — keep them bilingual (Turkish + English). Do NOT remove Turkish keywords.
- Crossterm's exact `KeyEvent` for Ctrl+J / Shift+Enter is terminal-dependent — verify empirically with a temporary debug print during Task 1; bind newline to whatever those keys actually produce, always including Ctrl+J.
- Every task ends green: `cargo build` and `cargo test`.

---

### Task 1: Newline key (multi-line input)

**Files:**
- Modify: `src/tui/term.rs` (setup/restore — kitty enhancement flags)
- Modify: `src/tui/editor.rs` (`handle_key` — newline binding; tests)

**Interfaces:**
- Consumes: existing `InputBox::handle_key(KeyEvent) -> Action`, `tui_input::InputRequest::InsertChar`.
- Produces: newline-insert behavior; no new public API.

- [ ] **Step 1: Write the failing tests** in `src/tui/editor.rs` `mod tests`:

```rust
#[test]
fn shift_enter_inserts_newline_not_submit() {
    let mut b = InputBox::new();
    type_str(&mut b, "a");
    let se = KeyEvent::new(KeyCode::Enter, KeyModifiers::SHIFT);
    assert!(matches!(b.handle_key(se), Action::None));
    type_str(&mut b, "b");
    assert_eq!(b.value(), "a\nb");
    match b.handle_key(code(KeyCode::Enter)) {
        Action::Submit(s) => assert_eq!(s, "a\nb"),
        o => panic!("Submit bekleniyordu: {o:?}"),
    }
}

#[test]
fn ctrl_j_inserts_newline() {
    let mut b = InputBox::new();
    type_str(&mut b, "x");
    let cj = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::CONTROL);
    assert!(matches!(b.handle_key(cj), Action::None));
    assert_eq!(b.value(), "x\n");
}

#[test]
fn alt_enter_inserts_newline() {
    let mut b = InputBox::new();
    let ae = KeyEvent::new(KeyCode::Enter, KeyModifiers::ALT);
    assert!(matches!(b.handle_key(ae), Action::None));
    assert_eq!(b.value(), "\n");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p usta --lib tui::editor`
Expected: FAIL — Shift/Alt+Enter currently hit `KeyCode::Enter => Submit`; Ctrl+J falls through to insert 'j' (or Enter). New asserts fail.

- [ ] **Step 3: Add newline binding** in `src/tui/editor.rs::handle_key`, immediately after the Ctrl-C/Ctrl-D exit check and before `match key.code {`:

```rust
// Newline insert (multi-line input): Shift+Enter / Alt+Enter (modern terminals via
// kitty keyboard protocol) or Ctrl+J (LF — universal fallback, works everywhere).
// Bare Enter still submits.
let newline = (matches!(key.code, KeyCode::Enter)
        && key.modifiers.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT))
    || (key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('j'));
if newline {
    self.cursor = None;
    self.input.handle(tui_input::InputRequest::InsertChar('\n'));
    return Action::None;
}
```

- [ ] **Step 4: Enable kitty flags** in `src/tui/term.rs`. Add to imports:

```rust
use crossterm::event::{KeyboardEnhancementFlags, PushKeyboardEnhancementFlags, PopKeyboardEnhancementFlags};
```

In `setup()`, after the `EnableBracketedPaste` line:

```rust
// Kitty keyboard protocol: lets modern terminals disambiguate Shift+Enter / Alt+Enter
// from bare Enter. Unsupported terminals are skipped silently (Ctrl+J still works).
if matches!(crossterm::terminal::supports_keyboard_enhancement(), Ok(true)) {
    let _ = crossterm::execute!(
        std::io::stdout(),
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    );
}
```

In `restore()`, before the `DisableBracketedPaste` line (harmless if nothing was pushed):

```rust
let _ = crossterm::execute!(std::io::stdout(), PopKeyboardEnhancementFlags);
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p usta --lib tui::editor`
Expected: PASS (all editor tests, including existing ones).

- [ ] **Step 6: Manual smoke + Ctrl+J verification**

Run `cargo run` in a terminal. Type a line, press Ctrl+J → cursor drops to a second line (⏎ shown). Press Enter → whole block submits as one message. If Shift+Enter does nothing on this terminal, that's expected (Ctrl+J is the guarantee). If Ctrl+J does NOT insert a newline, add a temporary `eprintln!("{key:?}")` at the top of `handle_key`, observe the real `KeyEvent`, and widen the `newline` condition to match it.

- [ ] **Step 7: Commit**

```bash
git add src/tui/term.rs src/tui/editor.rs
git commit -m "tui: çok-satırlı girdi — Shift+Enter/Alt+Enter (kitty) + Ctrl+J newline"
```

---

### Task 2: Single Esc cancels + status line English

**Files:**
- Modify: `src/tui/run.rs` (`ask_live` — Esc branch)
- Modify: `src/tui/status.rs` (English labels + hints; tests)

**Interfaces:**
- Consumes: `AskOutcome::Cancelled`, existing `ask_live` select loop, `render_status(&Status, Option<u64>, u64)`.
- Produces: `render_status` signature UNCHANGED in this task (watch indicator is Task 3).

- [ ] **Step 1: Update status.rs tests** to expect English in `src/tui/status.rs mod tests`:

```rust
#[test]
fn thinking_shows_spinner_frame() {
    let l = render_status(&Status::Thinking { frame: 0, cancel_hint: false }, None, 1_000_000);
    assert!(text(&l).contains("thinking"));
    assert!(text(&l).contains("esc to stop"));
}

#[test]
fn thinking_with_cancel_hint_shows_hint() {
    let l = render_status(&Status::Thinking { frame: 0, cancel_hint: true }, None, 1_000_000);
    assert!(text(&l).contains("ctrl-c again"));
}

#[test]
fn gauge_shows_ratio() {
    let l = render_status(&Status::Idle, Some(500_000), 1_000_000);
    assert!(text(&l).contains("context 500k/1000k"));
    assert!(text(&l).contains("▓▓▓▓░░░░"));
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p usta --lib tui::status`
Expected: FAIL — current strings are Turkish ("düşünüyor", "iptal", "bağlam").

- [ ] **Step 3: Translate status.rs** — in `render_status`, replace the Thinking block and gauge label:

```rust
if let Status::Thinking { frame, cancel_hint } = s {
    let hint = if *cancel_hint {
        " (press ctrl-c again to quit · esc to stop)"
    } else {
        " (esc to stop)"
    };
    spans.push(Span::styled(
        format!("{} Usta is thinking…{hint} ", FRAMES[frame % FRAMES.len()]),
        Style::default().fg(Color::DarkGray),
    ));
}
```

Gauge span format string: change `"…bağlam {}k/{}k"` to `"…context {}k/{}k"` (keep the `▓`/`░` bar and `t/1000`, `window/1000` args identical).

- [ ] **Step 4: Add the Esc branch** in `src/tui/run.rs::ask_live`, inside `else if let Event::Key(k) = ev {`, as the FIRST check before `match classify_locked_key(k)`:

```rust
// Single Esc = instant cancel (drops fut → kill_on_drop kills the child).
if matches!(k.code, KeyCode::Esc) {
    return Ok(AskOutcome::Cancelled);
}
```

- [ ] **Step 5: Run status + build**

Run: `cargo test -p usta --lib tui::status && cargo build -p usta`
Expected: PASS, build OK.

- [ ] **Step 6: Manual smoke**

`cargo run`, ask something, press Esc mid-"thinking" → turn cancels immediately with the existing "yanıt iptal edildi…" notice (that notice is translated in Task 4). Status line reads "Usta is thinking… (esc to stop)".

- [ ] **Step 7: Commit**

```bash
git add src/tui/run.rs src/tui/status.rs
git commit -m "tui: tek Esc anında iptal + durum satırı İngilizce"
```

---

### Task 3: Companion toggle (/watch)

**Files:**
- Modify: `src/main.rs` (pure `parse_watch_command` + `apply_watch`; wire into `run_plain_loop`; tests)
- Modify: `src/tui/run.rs` (wire into main loop + gate debounce flush)
- Modify: `src/tui/status.rs` (watch indicator param)

**Interfaces:**
- Produces: `pub(crate) enum WatchCmd { On, Off, Toggle }`, `pub(crate) fn parse_watch_command(line: &str) -> Option<WatchCmd>`, `pub(crate) fn apply_watch(cmd: WatchCmd, cur: bool) -> (bool, &'static str)`.
- Produces: `render_status(&Status, Option<u64>, u64, Option<bool>)` — new trailing `watching` arg (`None` = don't show, pre-session/live; `Some(b)` = show indicator).
- Consumes: `page_user_echo`, `page_notice` (run.rs); `println!`/`ui::notice` (main.rs); `files.observe`.

- [ ] **Step 1: Write pure-fn tests** in `src/main.rs mod tests`:

```rust
#[test]
fn parse_watch_command_variants() {
    assert_eq!(parse_watch_command("/watch"), Some(WatchCmd::Toggle));
    assert_eq!(parse_watch_command("/watch on"), Some(WatchCmd::On));
    assert_eq!(parse_watch_command("/watch off"), Some(WatchCmd::Off));
    assert_eq!(parse_watch_command("  /watch off  "), Some(WatchCmd::Off));
    assert_eq!(parse_watch_command("hello"), None);
    assert_eq!(parse_watch_command("/quit"), None);
}

#[test]
fn apply_watch_transitions() {
    assert_eq!(apply_watch(WatchCmd::Off, true).0, false);
    assert_eq!(apply_watch(WatchCmd::On, false).0, true);
    assert_eq!(apply_watch(WatchCmd::Toggle, true).0, false);
    assert_eq!(apply_watch(WatchCmd::Toggle, false).0, true);
    assert!(apply_watch(WatchCmd::On, false).1.contains("on"));
    assert!(apply_watch(WatchCmd::Off, true).1.contains("off"));
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p usta --lib parse_watch_command apply_watch`
Expected: FAIL — functions don't exist.

- [ ] **Step 3: Add the pure functions** to `src/main.rs` (near `slug_system`, ~line 610):

```rust
/// Companion (file-watch feedback) slash command. Slash lines never reach the LLM.
#[derive(Debug, PartialEq)]
pub(crate) enum WatchCmd { On, Off, Toggle }

pub(crate) fn parse_watch_command(line: &str) -> Option<WatchCmd> {
    match line.trim() {
        "/watch" => Some(WatchCmd::Toggle),
        "/watch on" => Some(WatchCmd::On),
        "/watch off" => Some(WatchCmd::Off),
        _ => None,
    }
}

pub(crate) fn apply_watch(cmd: WatchCmd, cur: bool) -> (bool, &'static str) {
    let next = match cmd {
        WatchCmd::On => true,
        WatchCmd::Off => false,
        WatchCmd::Toggle => !cur,
    };
    let msg = if next {
        "companion on — watching your files"
    } else {
        "companion paused — file feedback off"
    };
    (next, msg)
}
```

- [ ] **Step 4: Run pure tests**

Run: `cargo test -p usta --lib parse_watch_command apply_watch`
Expected: PASS.

- [ ] **Step 5: Add `watching` param to `render_status`** in `src/tui/status.rs`. Change signature to:

```rust
pub fn render_status(s: &Status, tokens: Option<u64>, window: u64, watching: Option<bool>) -> Line<'static> {
```

At the START of the function (before the Thinking block), push the indicator:

```rust
let mut spans: Vec<Span> = Vec::new();
if let Some(on) = watching {
    let (txt, col) = if on { ("👁 watching ", Color::DarkGray) } else { ("watch off ", Color::DarkGray) };
    spans.push(Span::styled(txt.to_string(), Style::default().fg(col)));
}
```

Update the existing status tests to pass a 4th arg `None` (e.g. `render_status(&Status::Idle, None, 1_000_000, None)`), and add:

```rust
#[test]
fn watch_indicator_shows_when_some() {
    assert!(text(&render_status(&Status::Idle, None, 1_000_000, Some(true))).contains("watching"));
    assert!(text(&render_status(&Status::Idle, None, 1_000_000, Some(false))).contains("watch off"));
    assert!(!text(&render_status(&Status::Idle, None, 1_000_000, None)).contains("watch"));
}
```

- [ ] **Step 6: Update `draw` in `src/tui/run.rs`** to thread `watching`. Change signature:

```rust
fn draw(tui: &mut Tui, editor: &InputBox, status: &Status, tokens: Option<u64>, window: u64, watching: Option<bool>) -> Result<()> {
```

and its body call `render_status(status, tokens, window, watching)`. Update every `draw(...)` call site to add a trailing arg: `ask_live` → `None`, `ask_topic` → `None`, `tui_confirm` → `None`. The MAIN loop `draw` call (inside `run`, ~line 508) → `Some(watching)` (declared in Step 7).

- [ ] **Step 7: Wire toggle into TUI main loop** in `src/tui/run.rs::run`. Before the main `loop {` (~line 502) add:

```rust
let mut watching = true;
```

Inside the `Action::Submit(line)` arm, as the FIRST statement (before the `/quit` check):

```rust
if let Some(cmd) = crate::parse_watch_command(&line) {
    page_user_echo(&mut tui, &line)?;
    let (next, msg) = crate::apply_watch(cmd, watching);
    watching = next;
    page_notice(&mut tui, msg)?;
    continue;
}
```

Gate the debounce-flush arm (~line 556): wrap the per-path feedback loop so that when `!watching`, batches are only synced:

```rust
} else if !watching {
    // Companion off: keep the diff baseline current, no LLM feedback.
    for path in batch {
        if let Ok(c) = std::fs::read_to_string(&path) {
            let _ = files.observe(&path, c);
        }
    }
} else {
    for path in batch { /* existing handle_file_change loop unchanged */ }
}
```

- [ ] **Step 8: Wire toggle into plain loop** in `src/main.rs::run_plain_loop`. Before the `loop {` (~line 224) add `let mut watching = true;`. In the `InputEvent::Line(line)` arm, after `let line = line.trim().to_string();` and before the `/quit` check:

```rust
if let Some(cmd) = parse_watch_command(&line) {
    let (next, msg) = apply_watch(cmd, watching);
    watching = next;
    ui::notice(msg);
    let _ = ready_tx.send(());
    continue;
}
```

Gate the plain debounce-flush arm (~line 253) the same way: when `!watching`, only `files.observe(&path, c)` for each path in `batch`, skipping `handle_file_change`.

- [ ] **Step 9: Build + test**

Run: `cargo test -p usta && cargo build -p usta`
Expected: PASS.

- [ ] **Step 10: Manual smoke**

`cargo run`, type `/watch off` → notice "companion paused…", status shows "watch off". Save a watched file → no feedback. `/watch on` → "companion on…", status "👁 watching"; next save produces feedback with no giant diff.

- [ ] **Step 11: Commit**

```bash
git add src/main.rs src/tui/run.rs src/tui/status.rs
git commit -m "tui+plain: companion toggle — /watch on|off, açık başlar, kapalıyken sessiz senkron"
```

---

### Task 4: English UI / notice strings (Rust)

**Files:**
- Modify: `src/tui/run.rs` (all `page_notice`/prompt strings)
- Modify: `src/tui/welcome.rs` (welcome + learning-status box copy)
- Modify: `src/main.rs` (plain-loop `ui::warn`/`ui::notice`, `handle_file_change` messages)
- Modify: `src/ui.rs`, `src/feedback.rs`, `src/check.rs` (any user-facing strings)

**Interfaces:** No signature changes. Pure string translation of display text.

- [ ] **Step 1: Translate `run.rs` notices.** Replace each Turkish user-facing string with English, meaning preserved. Required set (search these):
  - `"Ne öğrenmek istiyorsun? (kısa yaz ya da cümleyle anlat)"` → `"What do you want to learn? (a word, or describe it in a sentence)"`
  - `"devam: {t}"` → `"resuming: {t}"`
  - `"konu: {slug} — detayı sohbette anlatırsın"` → `"topic: {slug} — tell me the details in chat"`
  - `"vazgeçildi — Enter = devam, ya da başka konu yaz"` → `"cancelled — Enter = resume, or type another topic"`
  - `"vazgeçildi"` → `"cancelled"`
  - `"açılış turu iptal edildi"` → `"opening turn cancelled"`
  - `"açılış turu atlandı: {e}"` → `"opening turn skipped: {e}"`
  - `"yanıt iptal edildi — mesajın kaldı, istersen devam et"` → `"response cancelled — your message is kept, continue if you like"`
  - `"hata: {e}"` → `"error: {e}"`
  - `"toplu değişiklik ({n} dosya) — feedback atlandı, izleme sürüyor"` → `"bulk change ({n} files) — feedback skipped, still watching"`
  - `"Bu konuda başka oturum açık olabilir — progress çakışabilir. Devam? [e/H]"` → `"Another session may be open for this topic — progress could clash. Continue? [y/N]"`
  - `"dosya feedback atlandı: {path}: {e}"` → `"file feedback skipped: {path}: {e}"`
  - Note: `tui_confirm` accepts `e`/`E`. When you change the `[e/H]` copy to `[y/N]`, ALSO update `tui_confirm` to accept `y`/`Y` (keep `e`/`E` too for back-compat): match `'y' | 'Y' | 'e' | 'E' => true`.

- [ ] **Step 2: Translate `welcome.rs`** copy — headings, "öğrenme durumu", "sırada ne var", identity box labels, and the intro line "kısa kişisel tanışma" text → English equivalents. Preserve layout/width logic and any `extract_name` behavior. Update `welcome.rs` tests that assert Turkish substrings to the new English substrings (keep structural asserts).

- [ ] **Step 3: Translate `main.rs` plain-path strings** — `ui::warn("açılış drilli atlandı: …")` → `"opening drill skipped: …"`, `"tanışma turu atlandı: …"` → `"introduction turn skipped: …"`, `"hata: {e}"` → `"error: {e}"`, plus the same bulk-change and file-feedback notices as run.rs. Translate `handle_file_change` user-facing message text (the `FileFeedback::Bildirim` payloads) to English.

- [ ] **Step 4: Sweep `ui.rs`, `feedback.rs`, `check.rs`** for remaining Turkish user-facing strings (prefixes, warnings) and translate. Do NOT touch comments here (Task 7).

- [ ] **Step 5: Build + test + grep**

Run: `cargo test -p usta && cargo build -p usta`
Expected: PASS (update any test asserting a translated substring).
Run: `grep -rnE '[çğşöüıİĞŞÖÜÇ]' src/tui/run.rs src/tui/welcome.rs` → only comments remain (translated in Task 7), no user-facing string literals.

- [ ] **Step 6: Manual smoke** — launch, confirm topic prompt, notices, welcome box, and cancel messages all read in English.

- [ ] **Step 7: Commit**

```bash
git add src/tui/run.rs src/tui/welcome.rs src/main.rs src/ui.rs src/feedback.rs src/check.rs
git commit -m "usta: UI ve bildirim metinleri İngilizce (kullanıcı-yönelik stringler)"
```

---

### Task 5: English model-facing prompts + slug logic

**Files:**
- Modify: `src/progress.rs` (`opening_prompt`, `onboarding_prompt`, curriculum prompt, `MEET_BLOCK`, profile-update prompt; tests)
- Modify: `src/main.rs` (`SLUG_SYSTEM`, `slug_system`, `new_topic_confirm_msg`, `finalize_slug` sentinel; tests)

**Interfaces:** Signatures unchanged. Prompt TEXT translated; behavior invariants preserved.

- [ ] **Step 1: Translate `progress.rs` prompts to English**, keeping structure and renaming Turkish structural markers:
  - `[OTURUM AÇILIŞI — GERİ ÇAĞIRMA DRİLLİ]` → `[SESSION OPENING — RECALL DRILL]`
  - `[YENİ KONU — TANIŞMA]` → `[NEW TOPIC — INTRODUCTION]`
  - `[PROFİL BOŞ]` (in `MEET_BLOCK`) → `[PROFILE EMPTY]`
  - "İLK CEVABI" → "FIRST ANSWER", "tekrar sorma" → "don't ask again", "KENDİN çıkar" → "infer it YOURSELF", "kabuğu yazar" → "the shell writes the files", "en fazla iki soru" → "at most two questions".
  - Preserve every behavior: topic embedded, intro block only when intro present, ≤2 questions, no-file-writing instruction, meet-block only when `profile_generic`, exam/CEFR web-research guidance, and (in the profile-update prompt at ~line 90-112) the `===DOSYA: profile===` marker and "no topic knowledge" rule.

- [ ] **Step 2: Update `progress.rs` tests** substring asserts to the English strings, keeping the invariant asserts. Mapping:
  - `contains("İLK CEVABI")` → `contains("FIRST ANSWER")`; `contains("tekrar sorma")` → `contains("don't ask again")`
  - `contains("KENDİN çıkar")` → `contains("infer it YOURSELF")`; `contains("tarihe/sınava")` → new English backup-question phrase (e.g. `contains("a deadline or exam")`); `contains("en fazla iki soru")` → `contains("at most two questions")`; `contains("Hedef")` → `contains("Goal")`
  - `contains("GERİ ÇAĞIRMA DRİLLİ")` → `contains("RECALL DRILL")`; `contains("SOR")` → `contains("ASK")`; `contains("TANIŞMA")` → `contains("INTRODUCTION")`; `contains("form")` stays (English word); `contains("harita")` → `contains("map")`
  - `contains("kabuğu yazar")` → `contains("shell writes")`; `!contains("dosyalara yazacaksın")` → `!contains("you will write files")`
  - `contains("[PROFİL BOŞ]")` → `contains("[PROFILE EMPTY]")`; `contains("1-2 soru")` → `contains("1-2 questions")`
  - Profile-update test: `contains("MEVCUT PROFİL")` → English (e.g. `contains("CURRENT PROFILE")`), `contains("KONU BİLGİSİ YAZILMAZ")` → English (e.g. `contains("NO TOPIC KNOWLEDGE")`), `contains("yalnız")` → English (e.g. `contains("only")`).

- [ ] **Step 3: Translate `main.rs` slug + confirm** :
  - `SLUG_SYSTEM` const (the base slug prompt) → English; wherever it instructs the model to answer `"genel"` for generic, change the sentinel word to `"general"`.
  - `slug_system` known-topics block: `"Mevcut konular: …"` → `"Existing topics: …"`, `"DEVAM ETME isteğiyse … slug'ını AYNEN döndür"` → English preserving the "return that slug verbatim" rule and the `DEVAM`/continue concept; keep the word the test checks (see Step 4).
  - `finalize_slug`: change `if s == "genel"` → `if s == "general"`.
  - `new_topic_confirm_msg`: `"yeni konu: {slug} — açayım mı? [e = evet / başka tuş = geri dön]"` → `"new topic: {slug} — open it? [e = yes / any other key = go back]"` (KEEP the leading `[e` — the confirm handler and its test rely on `e`).

- [ ] **Step 4: Update `main.rs` slug tests** :
  - `slug_system_injects_known_topics`: `contains("linux-guvenlik, rust")` stays; `contains("DEVAM")` → the English continuation keyword you used (e.g. `contains("CONTINUE")` — make prompt and test agree).
  - `slug_system_without_topics_is_base_only`: `contains("slug")` stays; `!contains("Mevcut konular")` → `!contains("Existing topics")`.
  - `finalize_slug_falls_back_to_raw_when_model_gives_genel`: rename call arg `"genel"` → `"general"` (test name may stay).
  - `new_topic_confirm_msg_names_slug_and_keys`: `contains("[e")` stays valid.
  - Do NOT change `interpret_topic_input` or its tests — resume keywords stay bilingual.

- [ ] **Step 5: Build + test**

Run: `cargo test -p usta`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/progress.rs src/main.rs
git commit -m "usta: modele giden promptlar İngilizce (drill/tanışma/slug) — davranış değişmez"
```

---

### Task 6: Translate brain markdown to English (+ mirror policy)

**Files:**
- Modify: `SOUL.md`, `RULES.md`, `TEACHING.md`, `GOAL.md`, `USTA.md`, `approaches/software.md`, `approaches/_default.md`, `learner/index.md` (any Turkish `.md` pulled via `include_str!` in `src/defaults.rs`)

**Interfaces:** These files are embedded verbatim by `defaults.rs::include_str!` and loaded as the system prompt by `brain.rs`. Translating the files updates the embedded defaults automatically — no Rust change.

- [ ] **Step 1: Confirm the embed set**

Run: `grep -n 'include_str!' src/defaults.rs`
Expected: lists USTA/SOUL/RULES/TEACHING/GOAL/USER/learner-index/approaches. Translate every listed Turkish `.md` EXCEPT `USER.md` (user profile — leave as-is; it's user data, and mirror policy handles language).

- [ ] **Step 2: Translate each brain file to English**, content preserved 1:1 (headings, rules, tables, intervention map). Behavior must not change — only language.

- [ ] **Step 3: Set the language policy in `SOUL.md`.** Replace the line `Kullanıcıyla **Türkçe** konuşursun.` with, verbatim:

```
**Operate in English by default.** Mirror the user's language: if the user writes in Turkish, reply in Turkish; if in English, reply in English. This is a soft preference, not a hard rule — follow the user's lead.
```

- [ ] **Step 4: Build + test** (embedded `include_str!` must still compile; brain-loading tests must pass)

Run: `cargo test -p usta && cargo build -p usta`
Expected: PASS. If any `brain.rs`/`defaults.rs` test asserts a Turkish brain substring, update it to the English equivalent.

- [ ] **Step 5: Manual smoke — language mirror**

`cargo run`, write an English message → Usta replies in English. Write a Turkish message → Usta replies in Turkish. Confirm persona/teaching behavior unchanged.

- [ ] **Step 6: Reinstall + commit**

```bash
cargo install --path .   # per USTA.md: brain changes require reinstall of the embedded binary
git add SOUL.md RULES.md TEACHING.md GOAL.md USTA.md approaches/ learner/index.md
git commit -m "brain: persona dosyaları İngilizce + dil politikası (base EN, kullanıcı dilini yansıt)"
```

---

### Task 7: Translate code comments to English

**Files:**
- Modify: all Turkish comments across `src/**/*.rs` (~22 files): `anthropic.rs backend.rs brain.rs check.rs config.rs defaults.rs feedback.rs index.rs input.rs main.rs progress.rs session.rs transcript.rs ui.rs watcher.rs tui/{convert,editor,mod,run,status,term,welcome}.rs`

**Interfaces:** Comments only. NO code, string, or test-logic changes.

- [ ] **Step 1: Translate comments file-by-file.** Only `//!` and `//` comment text. Leave code, string literals, and test data untouched. Files that change together can be split across subagents (each subagent gets 3-5 files).

- [ ] **Step 2: Verify nothing but comments changed**

Run: `cargo test -p usta && cargo build -p usta`
Expected: PASS, identical behavior.
Run: `grep -rnE '[çğşöüıİĞŞÖÜÇ]' src` → remaining hits should ONLY be intentional Turkish inside test data / string literals (e.g. `type_str(&mut b, "çğşü")`, slug tests like `"Linux Güvenlik"`, bilingual resume keywords), never comments.

- [ ] **Step 3: Commit**

```bash
git add src
git commit -m "chore: tüm kod yorumları İngilizce — davranış değişmez"
```

---

## Self-Review

**Spec coverage:** Newline (Task 1), Esc stop (Task 2), companion toggle (Task 3), UI strings English (Task 4), model prompts + SOUL policy English (Tasks 5–6), brain md (Task 6), code comments (Task 7). All five spec units covered; Faz A = Tasks 1–5, Faz B = Tasks 6–7. ✓

**Placeholder scan:** No TBD/TODO; concrete code for Tasks 1–3; explicit string maps + test-assert maps for Tasks 4–5; embed/reinstall steps for Task 6. ✓

**Type consistency:** `render_status` gains `Option<bool>` in Task 3 and every `draw`/call site updated in the same task; `WatchCmd`/`parse_watch_command`/`apply_watch` defined and used in Task 3; `finalize_slug` sentinel `"genel"→"general"` changed together with `SLUG_SYSTEM` and its test in Task 5; `tui_confirm` copy `[e/H]→[y/N]` paired with accepting `y`/`Y` in Task 4. ✓

**Cross-task ordering:** status.rs touched by Task 2 (English) then Task 3 (param) — sequential, no conflict. Task 4 does not re-touch status.rs. SOUL policy delivered in Task 6 (which fully rewrites SOUL) — Task 5 leaves SOUL alone. ✓
