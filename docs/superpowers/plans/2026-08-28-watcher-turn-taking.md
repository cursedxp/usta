# Watcher Turn-Taking — Accumulate & Ride Along — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** Spec commit'i (`ed956c8`) üstüne — `docs/superpowers/specs/2026-08-28-watcher-turn-taking-design.md` ağaçta OLMALI ve çelişkide o kazanır. Önce spec'i oku, özellikle "İsimlendirme (bağlayıcı)", "Kararlar" (K1–K6) ve "Davranış (bağlayıcı detay)" bölümleri. DİKKAT — dal durumu: bu plan yazılırken spec commit'leri `intro-welcome-panel` dalındaydı ve o dalda ilgisiz bir düzeltme (`20420d5`, dosyaları: `src/tui/welcome.rs`, `src/tui/welcome_tests.rs`, `src/tui/entry.rs`, `src/tui/intro.rs`) vardır. Bu planın HİÇBİR görevi o dört dosyaya dokunmaz; iş dalını spec'in bulunduğu commit'ten aç (`git switch -c watcher-turn-taking ed956c8`), `intro-welcome-panel` dalının kendisini geliştirme tabanı olarak sahiplenme.

**Goal:** Gözcü (watcher) hiçbir koşulda kendi başına LLM turu açmaz (K1, istisna yok — `exercises/` dahil); flush olan batch `PendingChanges`'te birikir ve kullanıcının BİR SONRAKİ mesajıyla aynı çağrıya binerek gider (K2 ride-along: dosya bloğu önce, kullanıcının sözü son). Varlık deterministiktir: durum satırı `👁 watching · N changes noted` sayacı gösterir, teslimde sıfırlanır (K3). Anlık geri bildirim kullanıcının AÇIK kararıyla yaşamaya devam eder: `/watch live [on|off]` (`/watch polite`'ın yerine) ve approach dosyasındaki `watch: live` satırı artık zamanlama seçer (K4). Prompt tarafında üç değişiklik (K5): eyes-only kuralı, tekrar yasağı, TEACHING.md:85 vaat düzeltmesi. `Cargo.lock` gözcünün yok sayılanlarına girer (K6). Hedef sürüm **v0.28.0**.

**Architecture:** Biriktirme, tekilleştirme, sayaç ve teslim kararının TAMAMI deterministik shell işi — modele yalnız payload ve çerçeve gider. Saf mantık iki yerde: `src/tui/polite.rs` (`PendingChanges`, 4-kollu `route`, flush dağıtıcısı `dispatch_flush`, teslim sarmalayıcısı `attach_pending`) ve `src/file_feedback.rs` (`pending_preamble` + `ride_along_turn` + `deliver_pending`; payload üretimi zaten oradaki `build_batch_payload`'da). `src/tui/run.rs` (598 satır / 600 bütçe, belgeli istisna — BÜYÜTÜLEMEZ) yalnız ince çağrı yerleri tutar: deadline kolu tek `dispatch_flush` çağrısına iner (yerinden etme burada kazanılır), Submit koluna tek `attach_pending` satırı girer. Wiring pin testleri (iki kez sessiz silme yüzünden varlar) gevşetilmez — vacuous kalacak iğneler hâlâ ısıran yenileriyle DEĞİŞTİRİLİR.

**Tech Stack:** Rust, tokio `select!` (run.rs döngüsü), ratatui inline viewport. Binary crate — test filtreli `cargo test <filtre>`. `tokio::select!` makro gövdesini rustfmt biçimlemez — run.rs'in kol içi uzun tek-satır çağrıları bilinçlidir, bütçe bu sayede tutar.

## Global Constraints

- TÜM yeni tanımlayıcılar, kullanıcıya dönük string'ler, kod yorumları ve commit mesajları İNGİLİZCE (spec "İsimlendirme" bölümü birebir bağlayıcı: `PendingChanges`, `Route::Hold`, `pending_preamble`, `changes noted`, `WatchCmd::Live{Toggle,On,Off}`).
- Her task: TDD — önce failing test, doğru sebeple fail ettiği görülür, sonra minimal implementasyon; task sonunda `cargo test` TÜMÜ yeşil, sonra commit. **Push / tag / `cargo install` YOK** — bunlar manuel doğrulama sonrası insanın kararı; plan yalnız lokal commit üretir.
- `cargo clippy --all-targets` her task sonunda **0 uyarı**. Henüz tüketilmeyen aşamalı öğeler `#[allow(dead_code)]` alır (`src/tui/theme.rs` / `src/tui/editor.rs` emsali) ve tüketen task'ta bu attribute'lar SÖKÜLÜR.
- `cargo fmt` yalnız dokunulan dosyalara scoped (`cargo fmt -- <dosyalar>`); en sonda (Task 6) `cargo fmt --check` crate-genelinde temiz.
- `src/tui/run.rs` 598 satırda, 600 hard bütçesinin belgeli istisnası — **BÜYÜYEMEZ** (`grep -c "" src/tui/run.rs` ≤ 598). run.rs'e dokunan HER task sonunda sayı doğrulanır. Eklenen satır kadarı yerinden edilir: mantık `polite.rs`'e taşınır, run.rs'te yalnız çağrı kalır (Task 5'te deadline kolu tam bu sebeple `dispatch_flush`'a iner; beklenen final ≈ 593).
- `src/plain.rs` DEĞİŞMEZ (plain yolunda watcher yok; `parse_watch_command`'ı opak `cmd` ile `apply_watch`'a geçirir, varyant adı yazmaz — enum yeniden adlandırması onu derlemede kırmaz, doğrulanacak).
- Davranış regresyonu yasak: `usta start <topic>`, resume, lock-çakışma onayı, katalog upsert, transcript kaydı, altı-dosyalık closing flush sözleşmesi, `/watch on|off`, bulk-skip davranışı.
- Prompt diet: biriktirme, tekilleştirme, sayaç ve teslim kararı tamamen shell'de; modele yalnız payload + çerçeve gider. Ambient panelde LLM üretimi metin YASAK (K3).
- Wiring pin testleri (`run_rs_wiring_call_sites_are_pinned` — `src/tui/polite.rs`, `polite_branch_selecting_flow_frame_is_pinned` — `src/file_feedback.rs`) gevşetilmez; iğnesi vacuous'laşan pin, hâlâ ısıran bir eşdeğerle değiştirilir (Task 4/5 bunu açıkça yapar).

---

### Task 1: `Cargo.lock` gözcünün yok sayılanlarına girer (K6)

**Files:**
- Modify: `src/watcher.rs` (`is_ignored` gövdesine 1 kol + test modülüne 1 test)

**Interfaces:**
- Consumes: `watcher::is_ignored(path: &Path) -> bool` (mevcut; path bileşenlerini lowercase edip desenle eler).
- Produces: aynı fonksiyon, artık `Cargo.lock` bileşenini de eler. Başka hiçbir task buna bağımlı değil — bağımsız teslim edilebilir.

- [ ] **Step 1: Failing test.** `src/watcher.rs` test modülüne (`is_ignored_flags_emacs_autosave_file`'ın altına) ekle:

```rust
    #[test]
    fn is_ignored_flags_cargo_lock() {
        // Machine-written lockfile — a `cargo run` side effect, never the
        // user's own work; it bought an LLM turn in the 2026-08-28 live
        // session (spec K6).
        assert!(is_ignored(Path::new("Cargo.lock")));
        assert!(is_ignored(Path::new("sub/crate/Cargo.lock")));
        // The source file next to it must stay watchable.
        assert!(!is_ignored(Path::new("src/main.rs")));
    }
```

- [ ] **Step 2:** Çalıştır: `cargo test is_ignored_flags_cargo_lock` → Beklenen: **FAIL** (`assertion failed: is_ignored(Path::new("Cargo.lock"))`).
- [ ] **Step 3: Minimal implementasyon.** `src/watcher.rs` içinde `is_ignored`'ın match kolunda, `|| s == "node_modules"` satırının hemen altına ekle (dikkat: `s` bu noktada zaten lowercase):

```rust
                || s == "node_modules"
                // Machine-written lockfile (`cargo run` side effect) — never
                // the user's work, never worth a turn (spec K6).
                || s == "cargo.lock"
```

- [ ] **Step 4:** `cargo test is_ignored` → tüm `is_ignored_*` testleri PASS. Ardından `cargo test` (tümü) → yeşil.
- [ ] **Step 5:** `cargo fmt -- src/watcher.rs` · `cargo clippy --all-targets` → 0 uyarı.
- [ ] **Step 6:** Commit:

```bash
git add src/watcher.rs
git commit -m "fix: ignore Cargo.lock in the watcher — machine-written, never user work"
```

---

### Task 2: Prompt tarafı — eyes-only + tekrar yasağı + TEACHING.md vaadi (K5)

**Files:**
- Modify: `src/file_feedback.rs` (`flow_frame` gövdesi + `flow_frame_pins_the_four_lesson_rules` testinin yenilenmesi)
- Modify: `TEACHING.md` (satır 85, "Exercise Loop" bölümü — vaat cümlesi)
- Modify: `src/defaults.rs` (test modülüne 1 prompt-pin testi; TEACHING.md `include_str!` ile gömülü, dağıtımı mevcut `write_global_defaults` senkronu yapar — ekstra iş yok)

**Interfaces:**
- Consumes: `file_feedback::flow_frame(files_payload: &str, any_exercise: bool) -> String` (mevcut, `pub(crate)`); `EXERCISE_REVIEW_RULE` sabiti (mevcut, değişmiyor).
- Produces: `flow_frame` aynı imza, gövde 5 kurallı (kural 2 yeniden yazıldı, kural 5 yeni). Task 4'ün `ride_along_turn`'ü bu `flow_frame`'i sarar — "part of the ongoing lesson", "FOR YOUR EYES ONLY", "never repeat the full question" metinleri sonraki task testlerinin iğneleridir, birebir korunmalı.

Not: Bu kurallar birincil savunma DEĞİL — K1 (Task 5) sızıntı fırsatını zaten kaldırıyor; bunlar ride-along payload'ı için yedek. Zamanlama henüz değişmedi; `flow_frame` şimdilik anlık yolda kullanılıyor olsa da metin her iki zamanlamada geçerli.

- [ ] **Step 1: Failing testler.** `src/file_feedback.rs` test modülünde `flow_frame_pins_the_four_lesson_rules` testini SİL ve yerine şunu koy:

```rust
    #[test]
    fn flow_frame_pins_the_five_lesson_rules() {
        let s = flow_frame("FILE: src/main.rs\n...", false);
        // (1) step check + advance, (2) one-sentence nudge — never a full
        // repeat (spec K5.2), (3) scaffold in one sentence, (4) answer then
        // recall the task, (5) eyes-only until the user reports (spec K5.1)
        assert!(s.contains("part of the ongoing lesson"));
        assert!(s.contains("next step"));
        assert!(s.contains("unanswered question"));
        assert!(s.contains("ONE short sentence"));
        assert!(s.contains("never repeat the full question"));
        assert!(s.contains("scaffold"));
        assert!(s.contains("hand-written"));
        assert!(s.contains("FOR YOUR EYES ONLY"));
        assert!(s.contains("verify it against"));
        assert!(!s.to_lowercase().contains("standalone code review"));
    }
```

`src/defaults.rs` test modülüne (`shipped_profile_carries_no_personal_name`'in altına) ekle:

```rust
    #[test]
    fn teaching_promise_matches_ride_along_watcher() {
        // Spec K5.3: saving a file must not promise an automatic review turn —
        // the watcher accumulates and the review comes with the user's next
        // message (spec K1/K2). Pins the embedded default TEACHING.md.
        let teaching = include_str!("../TEACHING.md");
        assert!(!teaching.contains("triggers your review automatically"));
        assert!(teaching.contains("saving alone does not start your review"));
    }
```

- [ ] **Step 2:** `cargo test flow_frame_pins_the_five_lesson_rules teaching_promise` sırayla çalıştır: `cargo test flow_frame_pins` → FAIL (`ONE short sentence` iğnesi yok); `cargo test teaching_promise` → FAIL (`triggers your review automatically` hâlâ mevcut).
- [ ] **Step 3: Implementasyon.** `src/file_feedback.rs` içinde `flow_frame`'in `format!` bloğunu şu gövdeyle değiştir (kural 1, 3, 4 birebir aynı; kural 2 yeniden yazıldı; kural 5 yeni; `if any_exercise` kuyruğu değişmiyor):

```rust
pub(crate) fn flow_frame(files_payload: &str, any_exercise: bool) -> String {
    let mut frame = format!(
        "[Files changed]\n{files_payload}\n\n\
This change is part of the ongoing lesson — respond as the mentor guiding it, not as a reviewer opening a fresh audit. Apply these rules:\n\
1. If your last message asked for a step and this change satisfies it: confirm briefly, flag any errors, move to the next step.\n\
2. If there's an unanswered question from you still pending: nudge it in ONE short sentence — never repeat the full question text.\n\
3. First-sight full-content files may be tool-generated scaffold (e.g. a `cargo new` template) — acknowledge scaffold in one sentence, don't review it line by line; focus on the user's hand-written change.\n\
4. If the user asks a question in the middle of this, answer it, then recall the task.\n\
5. If your assignment asked the user to read, run, or describe an artifact, its content in this block is FOR YOUR EYES ONLY until the user reports on it: do not quote, summarize, or explain it — when the report comes, verify it against what you saw."
    );
    if any_exercise {
        frame.push_str(&format!(
            "\n\nThis batch includes an exercise submission. {EXERCISE_REVIEW_RULE}"
        ));
    }
    frame
}
```

(`flow_frame`'in fonksiyon-üstü doc comment'inde "the four rules" geçen cümleyi "the five rules" yap ve sonuna şu cümleyi ekle: `Rules 2 and 5 are the K5 backup layer for ride-along payloads — K1 removes the leak opportunity, these guard the payload itself.`)

`TEACHING.md` satır 85'te şu cümleyi:

```
The user writes the file; saving it triggers your review automatically.
```

şununla değiştir (satırın geri kalanı aynı kalır):

```
The user writes the file and tells you when it's done — saving alone does not start your review; the saved work rides along with their next message, and your review comes in that turn.
```

- [ ] **Step 4:** `cargo test flow_frame teaching_promise` → PASS. `cargo test` (tümü) → yeşil (özellikle `flow_frame_carries_exercise_rule_when_flagged` hâlâ geçmeli — `AS AN EXERCISE` kuyruğu korunuyor).
- [ ] **Step 5:** `cargo fmt -- src/file_feedback.rs src/defaults.rs` · `cargo clippy --all-targets` → 0 uyarı.
- [ ] **Step 6:** Commit:

```bash
git add src/file_feedback.rs src/defaults.rs TEACHING.md
git commit -m "feat: eyes-only + no-repeat rules in flow_frame; TEACHING.md drops the auto-review promise"
```

---

### Task 3: Tek dürüst eksen — `/watch live`, durum satırı, `polite` → `live` (K4 yüzeyi)

Bu task komut yüzeyini ve durum satırını yeni eksene çevirir; **zamanlama henüz değişmez** (companion hâlâ anlık — flip Task 5'te). Ara durum bilinçli: `live == false` bugünkü `polite == true` ile aynı davranır (`flow_frame`), `live == true` bugünkü `polite == false` ile (`feedback_frame`).

**Files:**
- Modify: `src/slash.rs` (`WatchCmd` varyantları, `parse_watch_command`, `apply_polite` → `apply_live`, `apply_watch` savunma kolu; testler)
- Modify: `src/help.rs` (satır 20 komut satırı; satır ~67 test iğneleri)
- Modify: `src/tui/status.rs` (`render_status` watch parametresi 3'lü tuple + yeni etiketler; testler)
- Modify: `src/tui/page.rs` (`draw` imzasındaki `watch` parametre tipi)
- Modify: `src/tui/run.rs` (durum bloğu ~421-428, slash kolu ~461-474, draw çağrısı ~440, `process_batch` çağrısına `!live` köprüsü ~588)
- Modify: `src/tui/polite.rs` (yalnız `approach_text`/`live_from_approach` doc comment'leri — davranış aynı)

**Interfaces:**
- Consumes: `slash::parse_watch_command(line: &str) -> Option<WatchCmd>`, `slash::apply_watch(cmd: WatchCmd, cur: bool) -> (bool, &'static str)`, `status::render_status`, `page::draw`, `polite::live_from_approach(text: &str) -> bool` (hepsi mevcut).
- Produces (sonraki task'lar bunlara yaslanır):
  - `enum WatchCmd { On, Off, Toggle, LiveOn, LiveOff, LiveToggle }` (Polite* varyantları SİLİNDİ)
  - `pub(crate) fn apply_live(cmd: WatchCmd, cur: bool) -> (bool, &'static str)` (`apply_polite` SİLİNDİ)
  - `pub fn render_status(s: &Status, tokens: Option<u64>, window: u64, watch: Option<(bool, bool, usize)>) -> Line<'static>` — tuple = `(watching, live, pending_count)`
  - `pub(crate) fn draw(tui, editor, status, tokens, window, watch: Option<(bool, bool, usize)>) -> Result<()>`
  - run.rs'te state: `let mut live: bool` (`polite` değişkeni YOK artık); draw çağrısı `Some((watching, live, 0))` — `0` yer tutucu, Task 5 `pending.len()` bağlar.

- [ ] **Step 1: Failing testler.** `src/slash.rs` test modülünde `parse_watch_polite_variants` ve `apply_polite_transitions` testlerini SİL, yerine ekle:

```rust
    #[test]
    fn parse_watch_live_variants() {
        assert_eq!(parse_watch_command("/watch live"), Some(WatchCmd::LiveToggle));
        assert_eq!(parse_watch_command("/watch live on"), Some(WatchCmd::LiveOn));
        assert_eq!(parse_watch_command("/watch live off"), Some(WatchCmd::LiveOff));
        assert_eq!(parse_watch_command("/WATCH LIVE OFF"), Some(WatchCmd::LiveOff));
        assert_eq!(parse_watch_command("/watch lively"), None);
        // The old surface is gone — `polite` no longer parses (spec K4).
        assert_eq!(parse_watch_command("/watch polite"), None);
        assert_eq!(parse_watch_command("/watch polite on"), None);
    }

    #[test]
    fn apply_live_transitions() {
        assert!(apply_live(WatchCmd::LiveOn, false).0);
        assert!(!apply_live(WatchCmd::LiveOff, true).0);
        assert!(apply_live(WatchCmd::LiveToggle, false).0);
        assert!(!apply_live(WatchCmd::LiveToggle, true).0);
        assert_eq!(
            apply_live(WatchCmd::LiveOn, false).1,
            "live feedback on — every save gets an immediate review"
        );
        assert_eq!(
            apply_live(WatchCmd::LiveOff, true).1,
            "live feedback off — changes ride along with your next message"
        );
        // Non-live variants pass through untouched (defensive arm).
        assert_eq!(apply_live(WatchCmd::On, true), (true, ""));
    }
```

`src/help.rs` testinde (`help_text_lists_shortcuts_commands_and_cli`) iğne listesindeki `"/watch polite",` ve `"lesson-flow feedback framing",` satırlarını şunlarla değiştir: `"/watch live",` ve `"immediate review on every save",`. Döngünün ALTINA ekle:

```rust
        assert!(!h.contains("/watch polite"), "old polite surface must be gone");
```

`src/tui/status.rs` test modülünde `watch_indicator_shows_polite_state` testini SİL, `watch_indicator_shows_when_some` içindeki `Some((true, false))` → `Some((true, false, 0))`, `Some((false, false))` → `Some((false, false, 0))` yap ve yeni test ekle:

```rust
    #[test]
    fn watch_indicator_live_and_companion_states() {
        // live: explicit marker, no counter even if a count is passed (spec K4/K3)
        let live = text(&render_status(&Status::Idle, None, 1_000_000, Some((true, true, 3))));
        assert!(live.contains("watching·live"));
        assert!(!live.contains("changes noted"));
        // companion with nothing pending: plain watching, no counter
        let idle = text(&render_status(&Status::Idle, None, 1_000_000, Some((true, false, 0))));
        assert!(idle.contains("👁 watching"));
        assert!(!idle.contains("live") && !idle.contains("noted"));
        // companion with pending: the deterministic counter (spec K3)
        let noted = text(&render_status(&Status::Idle, None, 1_000_000, Some((true, false, 2))));
        assert!(noted.contains("👁 watching · 2 changes noted"));
        // watch off wins regardless of the rest
        let off = text(&render_status(&Status::Idle, None, 1_000_000, Some((false, true, 5))));
        assert!(off.contains("watch off") && !off.contains("noted"));
    }
```

- [ ] **Step 2:** `cargo test parse_watch_live_variants` → Beklenen: **derlenmez** (`LiveToggle` yok — doğru sebeple fail).
- [ ] **Step 3: Implementasyon — `src/slash.rs`.** `WatchCmd`'de `PoliteOn, PoliteOff, PoliteToggle` → `LiveOn, LiveOff, LiveToggle`. `parse_watch_command` match kollarını değiştir:

```rust
        "/watch live" => Some(WatchCmd::LiveToggle),
        "/watch live on" => Some(WatchCmd::LiveOn),
        "/watch live off" => Some(WatchCmd::LiveOff),
```

(`"/watch polite*"` kolları SİLİNİR.) `apply_watch`'ın savunma kolu:

```rust
        // Live variants never reach this function; defensive, not unreachable!.
        WatchCmd::LiveOn | WatchCmd::LiveOff | WatchCmd::LiveToggle => cur,
```

`apply_polite` fonksiyonunu SİL, yerine:

```rust
/// Timing toggle for `/watch live` (spec K4): on, every debounce flush opens
/// an immediate plain-review turn; off (the default), changes accumulate and
/// ride along with the user's next message. Session-only, never persisted —
/// the per-topic default is the approach file's `watch: live` line. Only ever
/// called with the `Live*` variants; other variants return `cur` unchanged
/// (defensive, not `unreachable!`).
pub(crate) fn apply_live(cmd: WatchCmd, cur: bool) -> (bool, &'static str) {
    let next = match cmd {
        WatchCmd::LiveOn => true,
        WatchCmd::LiveOff => false,
        WatchCmd::LiveToggle => !cur,
        WatchCmd::On | WatchCmd::Off | WatchCmd::Toggle => return (cur, ""),
    };
    let msg = if next {
        "live feedback on — every save gets an immediate review"
    } else {
        "live feedback off — changes ride along with your next message"
    };
    (next, msg)
}
```

- [ ] **Step 4: Implementasyon — `src/help.rs`.** `help_text()` içindeki

```
     \x20\x20/watch polite    lesson-flow feedback framing, not plain review (on|off, default: on)\n\
```

satırını şununla değiştir:

```
     \x20\x20/watch live      immediate review on every save instead of ride-along (on|off, default: off)\n\
```

- [ ] **Step 5: Implementasyon — `src/tui/status.rs` + `src/tui/page.rs`.** `render_status` imzasında `watch: Option<(bool, bool)>` → `watch: Option<(bool, bool, usize)>`; gövdedeki watch bloğunu değiştir:

```rust
    // (watching, live, pending): `pending` is the accumulated-but-undelivered
    // change count — deterministic presence, zero tokens (spec K3). Live mode
    // shows its marker and never a counter; companion shows the counter only
    // when something is noted.
    if let Some((watching, live, pending)) = watch {
        let txt = match (watching, live) {
            (false, _) => "watch off ".to_string(),
            (true, true) => "👁 watching·live ".to_string(),
            (true, false) if pending > 0 => format!("👁 watching · {pending} changes noted "),
            (true, false) => "👁 watching ".to_string(),
        };
        spans.push(Span::styled(txt, theme::info()));
    }
```

`src/tui/page.rs`'te `draw`'un parametresi `watch: Option<(bool, bool)>` → `watch: Option<(bool, bool, usize)>` (gövde değişmez — `render_status`'a geçirir). Diğer `draw` çağrıları (`ask.rs:41,109`, `intro.rs:99`, `entry.rs:284`) `None` geçiyor — DOKUNMA, derlenmeye devam ederler.

- [ ] **Step 6: Implementasyon — `src/tui/run.rs`** (üç nokta; sonda satır sayısı kontrolü):

(a) Durum bloğu — mevcut 8 satırı (`let mut watching = true;` … `let mut polite = !crate::tui::polite::live_from_approach(&approach);`) şu 7 satırla değiştir:

```rust
    let mut watching = true;
    // One honest axis (spec K4): `live` = immediate feedback at every flush,
    // only by the user's explicit choice (`/watch live` or a `watch: live`
    // approach line). Off (default) = companion: lesson-flow framing — and,
    // once the timing flip lands, accumulate-and-ride-along delivery.
    let approach = crate::tui::polite::approach_text(project_root, global, &topic);
    let mut live = crate::tui::polite::live_from_approach(&approach);
```

(b) Draw çağrısındaki `Some((watching, polite)),` → `Some((watching, live, 0)),` (`0` = yer tutucu; Task 5 `pending.len()` bağlar).

(c) Slash kolunda `PoliteOn | PoliteOff | PoliteToggle` kolunu şununla değiştir:

```rust
                                LiveOn | LiveOff | LiveToggle => {
                                    // Session-only timing choice (spec K4).
                                    let (next, m) = crate::slash::apply_live(cmd, live);
                                    live = next;
                                    m
                                }
```

(d) Deadline kolundaki `process_batch(..., &batch, polite)` çağrısında son argüman `polite` → `!live`; üstündeki iki yorum satırını şununla değiştir:

```rust
                    // One combined LLM turn for the whole batch, right now;
                    // `live` picks the plain-review frame (timing flips next).
```

(e) `src/tui/polite.rs` doc düzeltmeleri: `live_from_approach` doc'una şu cümleyi ekle: `Selects the timing axis (spec K4): live = immediate feedback.`; `approach_text` doc'undaki `which keeps polite mode on` → `which keeps live off (companion default)`.

- [ ] **Step 7:** Doğrula: `grep -c "" src/tui/run.rs` → Beklenen: **597** (598'den küçük ya da eşit olmak ZORUNDA; sapma varsa yorum satırlarında fazlalık aranır, koddan kırpılmaz).
- [ ] **Step 8:** `cargo test` → tümü yeşil (özellikle: `parse_watch_live_variants`, `apply_live_transitions`, `watch_indicator_live_and_companion_states`, `help_text_lists_shortcuts_commands_and_cli`, ve DEĞİŞMEMESİ gerekenler: `apply_watch_transitions`, `run_rs_wiring_call_sites_are_pinned` — dört iğnesi hâlâ yerinde, `polite_branch_selecting_flow_frame_is_pinned` — `handle_batch_change` bu task'ta değişmedi).
- [ ] **Step 9:** `cargo fmt -- src/slash.rs src/help.rs src/tui/status.rs src/tui/page.rs src/tui/run.rs src/tui/polite.rs` · `cargo clippy --all-targets` → 0 uyarı · `grep -c "" src/tui/run.rs` tekrar ≤ 598.
- [ ] **Step 10:** Commit:

```bash
git add src/slash.rs src/help.rs src/tui/status.rs src/tui/page.rs src/tui/run.rs src/tui/polite.rs
git commit -m "feat: one honest axis — /watch live replaces /watch polite (surface only, timing flips next)"
```

---

### Task 4: Biriktirme çekirdeği — `PendingChanges` + ride-along teslim üretimi (K2/K3, aşamalı)

Saf, bağımsız test edilebilir katman. Bu task'ta run.rs'e BAĞLANMAZ (Task 5 bağlar) — henüz tüketilmeyen öğeler `#[allow(dead_code)]` taşır ve Task 5'te sökülür.

**Files:**
- Modify: `src/file_feedback.rs` (`PENDING_PREAMBLE` sabiti, `ride_along_turn`, `deliver_pending` + testler; `build_batch_payload` PRİVATE kalır — dışarıya yalnız `deliver_pending` açılır)
- Modify: `src/tui/polite.rs` (`PendingChanges` + `attach_pending` + testler)

**Interfaces:**
- Consumes: `file_feedback::build_batch_payload(files: &mut feedback::FileMemory, project_root: &Path, paths: &[PathBuf]) -> (String, BatchMeta)` (mevcut, private — aynı dosyadan çağrılır); `file_feedback::flow_frame(files_payload: &str, any_exercise: bool) -> String` (Task 2'nin 5-kurallı hâli); `tui::page::page_notice(tui: &mut Tui, msg: &str) -> Result<()>` (mevcut); `feedback::FileMemory` (mevcut).
- Produces (Task 5 bunları tüketir):
  - `file_feedback.rs`: `const PENDING_PREAMBLE: &str` (private, spec adı `pending_preamble` — Rust sabit adlandırması SCREAMING_SNAKE) · `fn ride_along_turn(files_payload: &str, any_exercise: bool, user_text: &str) -> String` (private) · `pub(crate) fn deliver_pending(files: &mut feedback::FileMemory, project_root: &Path, paths: &[PathBuf], user_text: String) -> (Vec<String>, String)` — `.0` teslim anında basılacak notisler (büyük/ikili dosya — mevcut kanal), `.1` birleşik giden metin (hiçbir dosya kalmadıysa `user_text` DEĞİŞMEDEN).
  - `polite.rs`: `pub(crate) struct PendingChanges` — `new() -> Self`, `hold(&mut self, batch: Vec<PathBuf>)` (sıra korunur, tekrar tekilleşir), `len(&self) -> usize`, `is_empty(&self) -> bool`, `take(&mut self) -> Vec<PathBuf>` (boşaltır — sayaç bununla sıfırlanır) · `pub(crate) fn attach_pending(tui: &mut Tui, pending: &mut PendingChanges, files: &mut FileMemory, project_root: &Path, user_text: String) -> Result<String>`.

- [ ] **Step 1: Failing testler — `src/tui/polite.rs`** test modülüne ekle:

```rust
    #[test]
    fn pending_changes_dedup_preserve_order_and_reset_on_take() {
        let mut p = PendingChanges::new();
        assert!(p.is_empty());
        assert_eq!(p.len(), 0);
        p.hold(vec![PathBuf::from("a.rs"), PathBuf::from("b.rs")]);
        p.hold(vec![PathBuf::from("a.rs"), PathBuf::from("c.rs")]);
        assert_eq!(p.len(), 3);
        // Order preserved, repeats collapsed (spec: Davranış/Akış step 1).
        assert_eq!(
            p.take(),
            vec![
                PathBuf::from("a.rs"),
                PathBuf::from("b.rs"),
                PathBuf::from("c.rs")
            ]
        );
        // take() drains — the status counter resets with it (spec K3).
        assert!(p.is_empty());
        assert!(p.take().is_empty());
    }
```

- [ ] **Step 2: Failing testler — `src/file_feedback.rs`** test modülüne (batch testlerinin yanına, `scratch_dir` yardımc1sını kullanarak) ekle:

```rust
    #[test]
    fn ride_along_turn_selects_flow_frame_and_keeps_user_words_last() {
        // Companion frame axis (spec K4): ride-along wraps the payload in
        // flow_frame — the plain-review wording must NOT appear. This is the
        // direct-test replacement for the old polite_branch source pin (the
        // frame choice is now pure and testable, so no crude pin is needed).
        let t = ride_along_turn(
            "FILE: src/main.rs (full contents)\nfn main() {}",
            false,
            "here is my report",
        );
        assert!(t.starts_with(PENDING_PREAMBLE));
        assert!(t.contains("part of the ongoing lesson"));
        assert!(!t.contains("Give project-grounded"));
        // The user's words are the LAST word (spec: Sıralama ve içerik).
        assert!(t.trim_end().ends_with("here is my report"));
    }

    #[test]
    fn deliver_pending_rides_payload_before_user_text() {
        let dir = scratch_dir("deliver-pending");
        let file = dir.join("main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let mut files = feedback::FileMemory::new();
        let (notices, outgoing) =
            deliver_pending(&mut files, &dir, &[file.clone()], "done, take a look".to_string());
        assert!(notices.is_empty());
        assert!(outgoing.starts_with(PENDING_PREAMBLE));
        let pos_payload = outgoing.find("FILE:").unwrap();
        let pos_user = outgoing.rfind("done, take a look").unwrap();
        assert!(pos_payload < pos_user, "payload must precede the user's text");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn deliver_pending_everything_dropped_returns_user_text_unchanged() {
        // A vanished file (deleted between hold and delivery) is a silent
        // skip; with nothing left, NO payload is attached (spec:
        // total_included == 0 → payload eklenmez).
        let dir = scratch_dir("deliver-pending-empty");
        let gone = dir.join("gone.rs");
        let mut files = feedback::FileMemory::new();
        let (notices, outgoing) =
            deliver_pending(&mut files, &dir, &[gone], "just a question".to_string());
        assert!(notices.is_empty());
        assert_eq!(outgoing, "just a question");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn deliver_pending_flags_exercise_batches_and_keeps_notices() {
        let dir = scratch_dir("deliver-pending-exercise");
        let ex = dir.join("exercises").join("rust").join("notes.md");
        std::fs::create_dir_all(ex.parent().unwrap()).unwrap();
        std::fs::write(&ex, "my answer\n").unwrap();
        // An oversized companion file exercises the notice channel at delivery
        // (spec: build_batch_payload'ın mevcut notis kanalı korunur).
        let big = dir.join("big.rs");
        std::fs::write(&big, "x".repeat(feedback::MAX_FILE_BYTES + 1)).unwrap();
        let mut files = feedback::FileMemory::new();
        let (notices, outgoing) =
            deliver_pending(&mut files, &dir, &[ex, big], "done".to_string());
        assert_eq!(notices.len(), 1);
        assert!(notices[0].contains("large file"));
        assert!(outgoing.contains("AS AN EXERCISE"));
        std::fs::remove_dir_all(&dir).ok();
    }
```

(Not: `feedback::MAX_FILE_BYTES` = 64 KiB, `src/feedback.rs:12` — bir üstü `ChangePayload::TooLarge` üretir ve `build_batch_payload` mevcut `(large file — not watched: …)` notisini basar; `batch_payload_too_large_file_drops_with_existing_notice_text` testi emsaldir.)

- [ ] **Step 3:** `cargo test pending_changes_dedup` → **derlenmez** (`PendingChanges` yok); `cargo test deliver_pending` → **derlenmez** (`deliver_pending` yok). Doğru sebeple fail.
- [ ] **Step 4: Implementasyon — `src/file_feedback.rs`.** `handle_batch_change`'in ALTINA ekle:

```rust
/// One-line frame at the head of a ride-along delivery (spec name:
/// `pending_preamble`). Purely descriptive — the behavioral rules live in
/// `flow_frame`, which follows it.
const PENDING_PREAMBLE: &str = "[The user changed the files below while working; they are delivered together with the user's message, which follows after the file block — the user's own words are the message to answer.]";

/// Compose the combined outgoing turn for a ride-along delivery (spec K2):
/// the one-line pending preamble, the lesson-flow-framed file block
/// (companion frame axis, spec K4), then the user's own words LAST — their
/// message is the one to answer (spec: Sıralama ve içerik).
fn ride_along_turn(files_payload: &str, any_exercise: bool, user_text: &str) -> String {
    format!(
        "{PENDING_PREAMBLE}\n{}\n\n{user_text}",
        flow_frame(files_payload, any_exercise)
    )
}

/// Deterministic ride-along delivery (spec K2): build the payload NOW — at
/// delivery time, not at flush time — so intermediate saves collapse into one
/// diff and meanwhile-deleted files drop out as silent skips. `.0` is the
/// notice channel (large/binary files), printed by the caller at delivery;
/// `.1` is the combined outgoing turn, or `user_text` UNCHANGED when nothing
/// made it into the payload. No LLM call happens here — the caller sends the
/// returned string through the normal ask path (prompt diet: only payload and
/// frame ever reach the model).
#[allow(dead_code)] // staged: consumed by the timing-flip task
pub(crate) fn deliver_pending(
    files: &mut feedback::FileMemory,
    project_root: &Path,
    paths: &[PathBuf],
    user_text: String,
) -> (Vec<String>, String) {
    let (payload, meta) = build_batch_payload(files, project_root, paths);
    if meta.total_included == 0 {
        return (meta.notices, user_text);
    }
    let turn = ride_along_turn(&payload, meta.any_exercise, &user_text);
    (meta.notices, turn)
}
```

- [ ] **Step 5: Implementasyon — `src/tui/polite.rs`.** `route`'un ALTINA ekle:

```rust
/// Accumulated-but-undelivered watcher batches (spec K2): only PATHS are
/// held — the payload is built at delivery time via
/// `file_feedback::deliver_pending`, so intermediate saves collapse into one
/// diff. Order preserved, repeats collapsed. `len` feeds the status line's
/// deterministic counter (spec K3); `take` drains, which is also the counter
/// reset.
#[derive(Default)]
pub(crate) struct PendingChanges {
    paths: Vec<PathBuf>,
}

#[allow(dead_code)] // staged: consumed by the timing-flip task
impl PendingChanges {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Accumulate a flushed batch — order preserved, duplicates collapsed.
    pub(crate) fn hold(&mut self, batch: Vec<PathBuf>) {
        for p in batch {
            if !self.paths.contains(&p) {
                self.paths.push(p);
            }
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.paths.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// Drain for delivery — resets the counter (spec K3).
    pub(crate) fn take(&mut self) -> Vec<PathBuf> {
        std::mem::take(&mut self.paths)
    }
}

/// Ride-along delivery at the user's submit (spec K2): with nothing pending
/// the user's text passes through untouched; otherwise the pending paths are
/// drained (counter reset, spec K3), the payload is built at THIS moment, its
/// notices are printed, and the combined turn — file block first, the user's
/// words last — is returned for the normal ask path. Deterministic shell
/// work; no LLM call here.
#[allow(dead_code)] // staged: consumed by the timing-flip task
pub(crate) fn attach_pending(
    tui: &mut Tui,
    pending: &mut PendingChanges,
    files: &mut FileMemory,
    project_root: &Path,
    user_text: String,
) -> Result<String> {
    if pending.is_empty() {
        return Ok(user_text);
    }
    let paths = pending.take();
    let (notices, outgoing) =
        crate::file_feedback::deliver_pending(files, project_root, &paths, user_text);
    for notice in &notices {
        crate::tui::page::page_notice(tui, notice)?;
    }
    Ok(outgoing)
}
```

- [ ] **Step 6:** `cargo test pending_changes deliver_pending ride_along_turn` sırayla → hepsi PASS. `cargo test` (tümü) → yeşil.
- [ ] **Step 7:** `cargo fmt -- src/file_feedback.rs src/tui/polite.rs` · `cargo clippy --all-targets` → **0 uyarı** (dead_code uyarısı kalırsa attribute eksik demektir — yukarıdaki üç `#[allow(dead_code)]` yerinde mi kontrol et).
- [ ] **Step 8:** Commit:

```bash
git add src/file_feedback.rs src/tui/polite.rs
git commit -m "feat: pending-changes accumulator + ride-along delivery core (staged)"
```

---

### Task 5: Zamanlama flip'i — `Route::Hold`, `dispatch_flush`, run.rs kablolaması (K1/K2/K3 yürürlükte)

Davranış bu task'ta döner: companion modda gözcü TUR AÇMAZ, biriktirir; teslim kullanıcının Submit'ine biner; sayaç canlıdır. `live` anlık kalır. Wiring pinleri yeni gerçeğe göre dürüstçe yeniden yazılır.

**Files:**
- Modify: `src/tui/polite.rs` (modül doc'u, `Route::Hold`, `route` + `live` parametresi, `dispatch_flush`, `process_batch`'ten `polite` parametresinin sökülmesi, `#[allow(dead_code)]`'ların sökülmesi, `route_truth_table` + pin testleri)
- Modify: `src/file_feedback.rs` (`handle_batch_change`'ten `polite` parametresinin sökülmesi — artık hep `feedback_frame`; `polite_branch_selecting_flow_frame_is_pinned` pin testinin SÖKÜLMESİ — Task 4'ün `ride_along_turn_selects_flow_frame_and_keeps_user_words_last` doğrudan testi yerini aldı; `deliver_pending` üstündeki `#[allow(dead_code)]` sökülür)
- Modify: `src/tui/run.rs` (durum bloğuna `pending`, Submit koluna `attach_pending`, deadline kolu → tek `dispatch_flush` çağrısı, draw'a `pending.len()`)

**Interfaces:**
- Consumes: Task 3'ün `live: bool` state'i ve 3'lü draw tuple'ı; Task 4'ün `PendingChanges` / `attach_pending` / `deliver_pending`'i; mevcut `bulk_skip`, `sync_baseline`, `process_batch`, `lifecycle::sleep_until_deadline`, `watcher::Debouncer`.
- Produces:
  - `pub(crate) enum Route { Bulk, ObserveOnly, Feedback, Hold }`
  - `pub(crate) fn route(batch_len: usize, max_batch: usize, watching: bool, live: bool) -> Route`
  - `pub(crate) async fn process_batch(tui, editor, events, backend, session, files, recorder, project_root, topic, last_tokens, paths) -> Result<()>` (`polite` parametresi YOK — canlı yol hep düz inceleme)
  - `pub(crate) async fn handle_batch_change(backend, session, files, project_root, paths, recorder) -> Result<(Vec<String>, FileFeedback)>` (`polite` parametresi YOK)
  - `#[allow(clippy::too_many_arguments)] pub(crate) async fn dispatch_flush(tui: &mut Tui, editor: &mut InputBox, events: &mut EventStream, backend: &mut Backend, session: &mut Session, files: &mut FileMemory, recorder: &Recorder, project_root: &Path, topic: &str, last_tokens: &mut Option<u64>, batch: Vec<PathBuf>, max_batch: usize, watching: bool, live: bool, pending: &mut PendingChanges) -> Result<()>`

- [ ] **Step 1: Failing test — route.** `src/tui/polite.rs`'te `route_truth_table` testini şununla DEĞİŞTİR:

```rust
    #[test]
    fn route_truth_table() {
        use Route::*;
        // bulk wins over everything, watching/live or not
        assert_eq!(route(11, 10, true, false), Bulk);
        assert_eq!(route(11, 10, true, true), Bulk);
        assert_eq!(route(11, 10, false, false), Bulk);
        // watching off → observe only, live or not
        assert_eq!(route(1, 10, false, false), ObserveOnly);
        assert_eq!(route(1, 10, false, true), ObserveOnly);
        // watching on, within the limit: live → immediate feedback,
        // companion default → hold, NEVER a turn (spec K1)
        assert_eq!(route(1, 10, true, true), Feedback);
        assert_eq!(route(1, 10, true, false), Hold);
        assert_eq!(route(5, 10, true, false), Hold);
        // boundary: exactly max is NOT bulk (existing `>` comparison)
        assert_eq!(route(10, 10, true, false), Hold);
        assert_eq!(route(10, 10, true, true), Feedback);
    }
```

- [ ] **Step 2:** `cargo test route_truth_table` → **derlenmez** (`route` 3 parametreli, `Hold` yok). Doğru sebeple fail.
- [ ] **Step 3: Implementasyon — `src/tui/polite.rs` çekirdek.**

(a) Modül doc'unu (dosyanın ilk 7 satırı) şununla değiştir — eski "polite is a prompt-frame switch, not a delay" cümlesi artık YALAN, tersine döndü:

```rust
//! The watcher's decision layer: which route a flushed debounce batch takes,
//! and what happens on each route. The watcher NEVER initiates an LLM turn on
//! its own (spec K1, no exceptions): the companion default HOLDS flushed
//! batches in `PendingChanges` and delivers them with the user's next submit
//! (`attach_pending` — ride along, spec K2). An immediate turn at flush exists
//! only as the user's explicit choice (`live`: `/watch live` or a
//! `watch: live` approach line), framed as plain review. Mostly pure logic;
//! `dispatch_flush` and `process_batch` are the impure pieces, kept here so
//! `run.rs` stays connective tissue.
```

(b) `Route`'a varyant ekle + `Feedback` doc'unu güncelle:

```rust
/// The four ways a flushed file-change batch can be handled — decided once,
/// up front, so the dispatcher only matches on the outcome.
#[derive(Debug, PartialEq)]
pub(crate) enum Route {
    /// Too many files at once — feedback skipped, baseline still synced.
    Bulk,
    /// Companion off — baseline synced, no LLM feedback, nothing accumulates.
    ObserveOnly,
    /// Live mode (explicit user choice): give feedback now, plain review frame.
    Feedback,
    /// Companion default: hold — paths accumulate in `PendingChanges` and
    /// ride along with the user's next submit. No turn (spec K1).
    Hold,
}
```

(c) `route`'u değiştir (doc dahil):

```rust
/// Picks the route for a flushed batch. Order matters, same as the original
/// if/else chain: a bulk save is skipped before the watching gate, and the
/// watching gate before the timing axis. `live` selects timing (spec K4):
/// an immediate turn only on the user's explicit say-so — the default is
/// Hold, because the watcher never initiates (spec K1).
pub(crate) fn route(batch_len: usize, max_batch: usize, watching: bool, live: bool) -> Route {
    if batch_len > max_batch {
        Route::Bulk
    } else if !watching {
        Route::ObserveOnly
    } else if live {
        Route::Feedback
    } else {
        Route::Hold
    }
}
```

(d) `process_batch`'ten `polite: bool` parametresini ve onu `handle_batch_change`'e geçiren argümanı SÖK; doc comment'indeki "`polite` picks the prompt frame there: the lesson-flow companion frame when on, plain review when off." cümlesini şununla değiştir: `Only the live path reaches here, and live is plain review by definition (spec K4) — the companion frame travels with ride-along delivery instead (attach_pending).`

(e) `process_batch`'in ALTINA dağıtıcıyı ekle:

```rust
/// The single flush entry point `run.rs` calls from the debounce deadline arm:
/// route the batch, then act — so the whole watcher policy lives here and
/// run.rs keeps one thin call site (its 600-line budget is why). Bulk and
/// observe-only are unchanged; a bulk batch never enters `PendingChanges`
/// (spec: Kenar durumlar), so the cap keeps meaning.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn dispatch_flush(
    tui: &mut Tui,
    editor: &mut InputBox,
    events: &mut EventStream,
    backend: &mut Backend,
    session: &mut Session,
    files: &mut FileMemory,
    recorder: &Recorder,
    project_root: &Path,
    topic: &str,
    last_tokens: &mut Option<u64>,
    batch: Vec<PathBuf>,
    max_batch: usize,
    watching: bool,
    live: bool,
    pending: &mut PendingChanges,
) -> Result<()> {
    match route(batch.len(), max_batch, watching, live) {
        Route::Bulk => bulk_skip(tui, files, batch)?,
        Route::ObserveOnly => sync_baseline(files, batch),
        // Live mode — the user's explicit timing choice: immediate turn.
        Route::Feedback => {
            process_batch(
                tui, editor, events, backend, session, files, recorder, project_root, topic,
                last_tokens, &batch,
            )
            .await?
        }
        // Companion default: accumulate; delivery rides the next submit (K2).
        Route::Hold => pending.hold(batch),
    }
    Ok(())
}
```

(f) Task 4'ün `#[allow(dead_code)]` attribute'larını SÖK: `impl PendingChanges` üstünden ve `attach_pending` üstünden (bu dosya), `deliver_pending` üstünden (`src/file_feedback.rs`). (`// staged: consumed by the timing-flip task` yorumları da gider.)

- [ ] **Step 4: Implementasyon — `src/file_feedback.rs`.** `handle_batch_change`'ten `polite: bool` parametresini sök; çerçeve seçimini şuna indir:

```rust
    let mut injected = feedback_frame(
        meta.any_exercise,
        &meta.displays.join(", "),
        &payload,
        false,
    );
```

Doc comment'indeki "Frame choice: `polite` selects the lesson-flow frame (`flow_frame`) or the same review frame …" cümlesini şununla değiştir: `Frame: always the plain review frame (feedback_frame) — only the live path calls this, and live is plain review by definition (spec K4); the companion frame ships with ride-along delivery (deliver_pending).` (exercise-flag ve cargo-check cümleleri kalır.)

`polite_branch_selecting_flow_frame_is_pinned` testini SİL — gerekçe yorumu Task 4'te eklenen `ride_along_turn_selects_flow_frame_and_keeps_user_words_last` testinde duruyor: çerçeve seçimi artık saf fonksiyonda, kaba kaynak pinine gerek kalmadı; pin gevşetilmedi, daha keskin bir doğrudan testle DEĞİŞTİRİLDİ. `handle_batch_change` çağıran async testler varsa (`batch_change_skips_llm_call_when_everything_drops` gibi) `polite` argümanını çağrılardan sök.

- [ ] **Step 5: Implementasyon — `src/tui/run.rs`** (dört nokta):

(a) Durum bloğu — Task 3'ün 7 satırını şu 8 satırla değiştir (yorum finale çekilir, `pending` eklenir):

```rust
    let mut watching = true;
    // One honest axis (spec K4): `live` = immediate plain-review turn at
    // every flush, only by the user's explicit choice (`/watch live` or a
    // `watch: live` approach line). Off (default) = companion: accumulate,
    // deliver on the user's next submit with the lesson-flow frame (K1/K2).
    let approach = crate::tui::polite::approach_text(project_root, global, &topic);
    let mut live = crate::tui::polite::live_from_approach(&approach);
    let mut pending = crate::tui::polite::PendingChanges::new();
```

(b) Draw çağrısındaki `Some((watching, live, 0)),` → `Some((watching, live, pending.len())),`

(c) Submit kolunda, `let outgoing = …;` bloğunun kapanışından (`};`) hemen sonra, `session.push_user(&outgoing);` satırından ÖNCE şu 2 satırı ekle (select! makro gövdesi — rustfmt dokunmaz, uzun tek satır bilinçli):

```rust
                        // Ride-along (spec K2): pending changes join THIS turn — payload first, the user's words last.
                        let outgoing = crate::tui::polite::attach_pending(&mut tui, &mut pending, &mut files, project_root, outgoing)?;
```

(d) Deadline kolunu — `_ = crate::lifecycle::sleep_until_deadline(…)` kolunun TÜM gövdesini (`let batch = debouncer.flush();`'tan match'in kapanışına kadar, `use crate::tui::polite::Route;` satırı dahil) — şu 2 satırla değiştir:

```rust
                // Watcher flush: routing + action live in polite::dispatch_flush (spec K1/K2) — run.rs keeps the call site only.
                crate::tui::polite::dispatch_flush(&mut tui, &mut editor, &mut events, backend, &mut session, &mut files, &recorder, project_root, &topic, &mut last_tokens, debouncer.flush(), max_feedback_batch, watching, live, &mut pending).await?;
```

- [ ] **Step 6: Pin testlerini dürüstle.** `src/tui/polite.rs`'te `run_rs_wiring_call_sites_are_pinned` iğne listesini şununla değiştir (gerekçe yorumu aynen kalır; sürüm notu eklenir):

```rust
        // v0.28.0: the four route arms moved into polite::dispatch_flush, so
        // the old per-arm needles went vacuous — replaced by needles for the
        // two new call sites plus the state wiring that feeds them. The arms
        // themselves are pinned by dispatch_flush_route_arms_are_pinned.
        let src = include_str!("run.rs");
        for needle in [
            "polite::approach_text(",
            "polite::live_from_approach(",
            "polite::PendingChanges::new(",
            "polite::dispatch_flush(",
            "polite::attach_pending(",
        ] {
            assert!(
                src.contains(needle),
                "run.rs lost its watcher wiring: {needle}"
            );
        }
```

Aynı test modülüne YENİ pin ekle:

```rust
    #[test]
    fn dispatch_flush_route_arms_are_pinned() {
        // dispatch_flush needs a live Backend + Tui, so its arms can't be
        // driven from a unit test (same class as
        // run_rs_wiring_call_sites_are_pinned): pin this file's own
        // production source, split at the test module so this assert's own
        // text can't match itself. Deleting an arm's body would otherwise
        // leave the suite green while a whole route silently died — the
        // exact failure class these pins exist for.
        let production_src = include_str!("polite.rs")
            .split("#[cfg(test)]")
            .next()
            .unwrap();
        for needle in [
            "match route(batch.len()",
            "Route::Bulk => bulk_skip(",
            "Route::ObserveOnly => sync_baseline(",
            "Route::Hold => pending.hold(",
        ] {
            assert!(
                production_src.contains(needle),
                "dispatch_flush lost an arm: {needle}"
            );
        }
        // process_batch appears once as its own definition; the second
        // occurrence is dispatch_flush's live-arm call.
        assert!(
            production_src.matches("process_batch(").count() >= 2,
            "dispatch_flush no longer calls process_batch on the live route"
        );
    }
```

- [ ] **Step 7:** Doğrula: `grep -c "" src/tui/run.rs` → Beklenen: **≈593** ve KESİNLİKLE ≤ 598. `cargo test` → TÜMÜ yeşil (özellikle `route_truth_table`, `dispatch_flush_route_arms_are_pinned`, `run_rs_wiring_call_sites_are_pinned`, `pending_changes_dedup_preserve_order_and_reset_on_take`, `deliver_pending_*`, `ride_along_turn_*`; ve REGRESYON yoklamaları: `watch_indicator_live_and_companion_states`, `apply_watch_transitions`, closing/transcript/lock testleri).
- [ ] **Step 8:** `cargo fmt -- src/tui/polite.rs src/file_feedback.rs src/tui/run.rs` · `cargo clippy --all-targets` → 0 uyarı (staged allow'lar söküldü, yenisi YOK) · `grep -c "" src/tui/run.rs` tekrar ≤ 598.
- [ ] **Step 9: Elle duman testi (5 dk, LLM anahtarı olan ortamda; yoksa atla ve commit mesajına `smoke: skipped` yaz).** Bir deneme projesinde `usta start rust` aç; başka terminalde projede bir dosya kaydet → TUR AÇILMADIĞINI ve durum satırının `👁 watching · 1 changes noted` olduğunu gör; ikinci bir dosya kaydet → sayaç 2; mesaj yaz → tek yanıt gelir, sayaç sıfırlanır. `/watch live` → notis `live feedback on — every save gets an immediate review`; dosya kaydet → anında tur, durum `👁 watching·live`. `/watch off` → `watch off`. `/quit` → normal kapanış.
- [ ] **Step 10:** Commit:

```bash
git add src/tui/polite.rs src/file_feedback.rs src/tui/run.rs
git commit -m "feat: watcher never initiates — accumulate by default, ride along on the user's turn"
```

---

### Task 6: Dokümantasyon + sürüm — v0.28.0

**Files:**
- Modify: `Cargo.toml` (satır 3: `version = "0.27.0"` → `"0.28.0"`) + `Cargo.lock` (cargo yeniler)
- Modify: `docs/ROADMAP.md` ("Completed" bölümü: yeni 2026-08-28 girdisi en üste; 2026-08-26 v0.24.0 girdisine tarihî not)
- Modify: `SPEC.md` (§4.21 bloğunun sonuna, v0.25.0 paragrafının "Design detail" satırından sonra v0.28.0 paragrafı; §4.21 başlığındaki "superseded by v0.25.0 below" → "superseded by v0.25.0 and v0.28.0 below")
- Modify: `README.md` ("Polite watching" bölümü baştan; "In-session commands" satırı; "Practice:" satırındaki save→review vaadi)

**Interfaces:**
- Consumes: Task 1–5'in tamamı yürürlükte (davranış metinleri koda birebir uymalı: `👁 watching · N changes noted`, `/watch live`, `watch: live` = zamanlama).
- Produces: v0.28.0 sürüm damgası. Tag/push/`cargo install` YOK — manuel doğrulama sonrası insan yapar.

- [ ] **Step 1: `Cargo.toml`.** `version = "0.28.0"` yap; `cargo check` çalıştır (Cargo.lock'taki `usta` sürümünü yeniler — ve ironinin hakkı: bu dosya artık gözcüde yok sayılıyor, git'te değil).
- [ ] **Step 2: `docs/ROADMAP.md`.** "## Completed" başlığının hemen altına yeni girdi:

```markdown
- 2026-08-28: Watcher turn-taking — the watcher never initiates an LLM turn (no exceptions, `exercises/` included): a flushed debounce batch is held in `PendingChanges` (paths only, order kept, dedup'd) and rides along with the user's NEXT message as one combined call — file block first, user's words last; the payload is built at delivery time (`deliver_pending`) so intermediate saves collapse into one diff and meanwhile-deleted files drop out. Presence is deterministic: the status line counts `👁 watching · N changes noted`, reset on delivery, zero tokens (no LLM-generated ambient text, ever). Immediate feedback survives as an explicit user choice — `/watch live [on|off]` replaces `/watch polite`, and the approach file's `watch: live` line now selects timing (immediate + plain review frame) instead of frame: `watching`+`polite` collapse into one honest axis (companion = accumulate + `flow_frame` · live = immediate + `feedback_frame`). Prompt side (backup layer, not the primary defense): eyes-only rule (assignment artifacts are verified against the user's report, never narrated), one-sentence nudge instead of full question repeats, TEACHING.md no longer promises "saving triggers your review". `Cargo.lock` joins the watcher's ignore list. Pending changes are dropped silently on `/quit`/close (the closing flush already reads disk); a bulk batch never enters the pending set. Design: `docs/superpowers/specs/2026-08-28-watcher-turn-taking-design.md`. v0.28.0.
```

Aynı dosyada 2026-08-26 (v0.24.0, "Polite watcher") girdisinin SONUNA (aynı madde içinde, "v0.24.0." noktasından sonra) ekle:

```markdown
 **Historical note (2026-08-28):** the queue/backstop mechanism this entry describes was retired in v0.25.0 (flow companion, `83813f0`) — it never coexisted with later versions — and v0.28.0 replaced the frame-only `polite` flag with accumulate-and-ride-along turn-taking; see the 2026-08-28 entry above.
```

- [ ] **Step 3: `SPEC.md`.** §4.21 başlığını `## 4.21 Polite Watcher (v0.24, superseded by v0.25.0 and v0.28.0 below)` yap. v0.25.0 bloğunun kapanışındaki `Design detail: docs/superpowers/specs/2026-08-27-flow-companion-design.md.` satırından SONRA ekle:

```markdown
**v0.28.0 — watcher turn-taking (accumulate & ride along):** the watcher never initiates an LLM turn (spec K1, no exceptions — `exercises/` included). The default (**companion**) holds flushed batches in `PendingChanges` (paths only; the payload is built at delivery via the existing batch merger, so intermediate saves collapse into one diff) and delivers them WITH the user's next message in a single call — a one-line `pending_preamble`, the `flow_frame`-framed file block, then the user's own words last. The status line shows a deterministic counter (`👁 watching · N changes noted`), reset on delivery; live mode shows `👁 watching·live` and no counter. Immediate feedback is an explicit user choice: `/watch live [on|off]` (replacing `/watch polite`) session-only, or a `watch: live` approach line as the per-topic default — both now select TIMING, collapsing the old `watching`+`polite` pair into one honest axis (companion = accumulate + `flow_frame` · live = immediate + `feedback_frame`). Bulk and observe-only routes are unchanged and a bulk batch never enters the pending set; pending changes are dropped silently at `/quit`/close (the closing flush already reads the disk). `Cargo.lock` is ignored at the watcher. Prompt side (backup layer): `flow_frame` gains an eyes-only rule and a one-sentence-nudge rule; TEACHING.md's exercise flow now says the user reports when done instead of promising an automatic review on save. The flush policy lives in `polite::dispatch_flush`; ride-along delivery in `polite::attach_pending` + `file_feedback::deliver_pending` — `run.rs` keeps thin call sites (its 600-line budget). Design: `docs/superpowers/specs/2026-08-28-watcher-turn-taking-design.md`.
```

- [ ] **Step 4: `README.md`.** Üç düzeltme:

(a) "In-session commands:" satırındaki `/watch on|off` (file feedback)` parçasını şu yap: `` `/watch on|off` (file feedback) · `/watch live` (immediate review per save) ``.

(b) "Practice:" satırını şu yap:

```markdown
Practice: Usta assigns exercises into `exercises/` — write, save, and say you're done: the review comes with your message. No solutions handed over.
```

(c) `### Polite watching` bölümünü (başlık + paragraf) tamamen şununla değiştir:

```markdown
### Companion watching

Usta watches your files but never interrupts. By default, saves accumulate quietly and ride along with your next message — the mentor sees your changes and your words together, in one turn, and your words are the last word. The status line counts what's noted (`👁 watching · 2 changes noted`); the counter resets when delivered. Files saved together are merged into a single block, and repeated saves of the same file collapse into one diff at delivery time.

Want a review the moment you save? Turn live mode on: `/watch live` (toggle; `on`/`off` also work) for the session, or add a `watch: live` line to the topic's approach file (`.usta/approaches/<topic>.md`, project override first) to make it the topic's default — the session-only command never writes back to the file. Live feedback uses plain review framing. `/watch off` stops watching entirely.
```

- [ ] **Step 5: Final kapılar.** `cargo test` → tümü yeşil · `cargo clippy --all-targets` → 0 uyarı · `cargo fmt --check` → crate-genelinde temiz · `grep -c "" src/tui/run.rs` → ≤ 598 · `grep -rn "watch polite" src README.md SPEC.md --include="*.rs"` → yalnız tarihî SPEC §4.21/v0.24–v0.25 paragraflarında ve ROADMAP tarihî girdisinde geçmeli, canlı dokümantasyonda GEÇMEMELİ.
- [ ] **Step 6:** Commit (tag/push/install YOK — insan kararı):

```bash
git add Cargo.toml Cargo.lock docs/ROADMAP.md SPEC.md README.md
git commit -m "docs: watcher turn-taking in SPEC/README/ROADMAP; bump to v0.28.0"
```

---

## Spec'in açık bıraktığı yerlerde verilen kararlar (yürütücü ve insan için)

1. **Ride-along çerçeve kompozisyonu:** Spec hem "payload `pending_preamble` ile çerçevelenir" (Akış 2) hem "companion çerçevesi `flow_frame`" (K4 tablosu) diyor. Plan ikisini birleştirir: `PENDING_PREAMBLE` (tek satır) + `flow_frame(payload)` + boş satır + kullanıcı metni. `flow_frame` birebir yeniden kullanılır — K4 tablosu ve mevcut pin metinleri korunur.
2. **Ride-along tesliminde `cargo check` KOŞMAZ:** Spec teslim içeriğinde yalnız "notis kanalı korunur" der, check'ten söz etmez; kapsam-dışı bölümü `run_check`'in soğukta 60 sn maliyetini gerekçe gösterir. Kullanıcının Submit'ini 60 sn bekletmek ride-along'un amacını öldürür. Canlı yol (`process_batch` → `handle_batch_change`) check davranışını AYNEN korur.
3. **`/watch off` bekleyenleri düşürmez:** Spec yalnız "watching kapalıyken hiçbir şey birikmez" der. Birikmiş olan durur ve bir sonraki Submit'te teslim edilir (teslim Submit'e bağlı, watching yalnız biriktirmeyi kapılar). Sessizce veri silmemek için minimal yorum bu.
4. **Sayaç dilbilgisi:** `N == 1` için de `1 changes noted` — spec durum metnini `changes noted` olarak bağlıyor; deterministik tek biçim tercih edildi.
5. **`pending_preamble` → `PENDING_PREAMBLE`:** Rust sabit adlandırma kuralı (SCREAMING_SNAKE); clippy lowercase sabite uyarı verir. Spec'in bağlayıcı adının birebir Rust karşılığıdır.
6. **`build_batch_payload` private kalır:** Spec teslim anında `build_batch_payload` kullanılmasını ister; plan bunu aynı dosyada `deliver_pending` sarmalayıcısıyla yapar — görünürlük genişletmeden, üstelik birim-test edilebilir sınır kazanarak.
7. **Eski `polite_branch_selecting_flow_frame_is_pinned` pini silinir:** İğnesi (`handle_batch_change` içinde `flow_frame`) yeni tasarımda kasıtlı olarak yanlışlaşıyor. Yerine iki daha keskin koruma girer: saf `ride_along_turn` doğrudan testi (çerçeve seçimi artık test edilebilir) + `dispatch_flush_route_arms_are_pinned` kaynak pini.

## İlgili

- Spec (bağlayıcı): `docs/superpowers/specs/2026-08-28-watcher-turn-taking-design.md`
- v0.25.0 gerekçesi: `docs/superpowers/specs/2026-08-27-flow-companion-design.md`
- Yapı emsali: `docs/superpowers/plans/2026-08-28-entry-flow-13a.md`
