# Proje-Farkında Başlangıç Önerisi Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** `2026-08-14-mentor-context-layer` planı implement edilmiş olmalı (edildi — `progress::project_md_path` mevcut). Spec: `docs/superpowers/specs/2026-08-14-project-aware-start-design.md` — önce oku.

**Goal:** İlk oturumda `mentor/PROJECT.md` varsa, konu girişinde boş Enter → Usta PROJECT.md'den başlangıç önerir (slug + gerekçe + ilk adım), kullanıcı onaylayınca oturum o konuyla açılır.

**Architecture:** `interpret_topic_input`'a `Suggest` varyantı (saf mantık). Öneri, slug mini-session deseniyle tek LLM çağrısı (`start_suggest_system` + `parse_start_suggestion`), sonrasında `backend.reset_session()`. TUI döngüsüne bir kol; welcome + prompt satırına koşullu ipucu. Plain/pipe yolu değişmez.

**Tech Stack:** Rust (mevcut crate, yeni bağımlılık YOK). Test: in-module `#[cfg(test)]`.

## Global Constraints

- Kod yorumları ve prompt/UI metinleri İngilizce; kullanıcıya dönen ÖNERİ içeriği LLM'den oturum dilinde gelir.
- Boş Enter + `local` dolu = resume — mevcut davranışa DOKUNMA (resume öneriden önceliklidir).
- Mini-çağrı sonrası HER durumda (başarı/iptal/hata) `backend.reset_session()` — slug mini-session paritesi (spec B1).
- Her task sonunda commit (Türkçe mesaj) + `cargo test` yeşil; signature kıran task tüm call site'ları aynı task'ta günceller.

---

### Task 1: `TopicChoice::Suggest` + `interpret_topic_input` üçüncü parametre

**Files:**
- Modify: `src/main.rs:766` (`TopicChoice` enum), `src/main.rs:776-800+` (`interpret_topic_input`)
- Modify: `src/tui/run.rs:471` (çağrı yeri) + `src/main.rs` içindeki mevcut `interpret_topic_input` test çağrıları
- Test: `src/main.rs` in-module tests

**Interfaces:**
- Produces: `TopicChoice::Suggest` varyantı; yeni imza `interpret_topic_input(raw: &str, local: &[String], project_known: bool) -> Option<TopicChoice>`. Task 3 `Suggest` kolunu işler.

- [ ] **Step 1: Write the failing tests**

`src/main.rs` test modülüne:

```rust
#[test]
fn empty_enter_suggests_when_no_local_topics_and_project_known() {
    assert!(matches!(
        interpret_topic_input("", &[], true),
        Some(TopicChoice::Suggest)
    ));
    assert!(matches!(interpret_topic_input("  ", &[], true), Some(TopicChoice::Suggest)));
}

#[test]
fn empty_enter_resume_beats_suggest_when_local_exists() {
    let local = vec!["rust".to_string()];
    assert!(matches!(
        interpret_topic_input("", &local, true),
        Some(TopicChoice::Resume(t)) if t == "rust"
    ));
}

#[test]
fn empty_enter_without_project_stays_none() {
    assert!(interpret_topic_input("", &[], false).is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test empty_enter`
Expected: derleme hatası — imza 2 parametre / `Suggest` yok

- [ ] **Step 3: Implement**

`TopicChoice` enum'una varyant ekle:

```rust
    /// Empty Enter with no resumable topic but a filled mentor/PROJECT.md —
    /// Usta proposes where to start (spec: project-aware start).
    Suggest,
```

`interpret_topic_input` imzasına `project_known: bool` ekle; boş-input bloğunu güncelle — eski:

```rust
    if raw.is_empty() {
        return local.first().map(|t| TopicChoice::Resume(t.clone()));
    }
```

yeni:

```rust
    if raw.is_empty() {
        return match local.first() {
            Some(t) => Some(TopicChoice::Resume(t.clone())), // resume wins over suggest
            None if project_known => Some(TopicChoice::Suggest),
            None => None,
        };
    }
```

- [ ] **Step 4: Update all call sites**

- `src/tui/run.rs:471` — döngüden ÖNCE (local/other hesaplandığı yerde) ekle:
```rust
            let project_known = std::fs::read_to_string(progress::project_md_path(project_root))
                .map(|s| !s.trim().is_empty())
                .unwrap_or(false);
```
çağrıyı `crate::interpret_topic_input(&raw, &local, project_known)` yap. Yeni `Some(crate::TopicChoice::Suggest) => {}` kolu ŞİMDİLİK boş — "swallow, ask again" (Task 3 dolduracak; derleme için exhaustive match şart).
- `src/main.rs` test modülündeki TÜM mevcut `interpret_topic_input(...)` çağrılarına üçüncü argüman olarak `false` ekle (davranışları değişmez).

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test`
Expected: tümü PASS

- [ ] **Step 6: Commit**

```bash
git add src/main.rs src/tui/run.rs
git commit -m "start-öneri: boş Enter + PROJECT.md → TopicChoice::Suggest (resume öncelikli)"
```

---

### Task 2: Öneri prompt'u + parser (`src/main.rs`)

**Files:**
- Modify: `src/main.rs` (`slug_system`/`finalize_slug` bloğunun yanı, ~satır 733-760)
- Test: `src/main.rs` in-module tests

**Interfaces:**
- Produces: `pub(crate) fn start_suggest_system() -> String`; `pub(crate) fn parse_start_suggestion(reply: &str) -> Option<(String, String)>` → `(slug, öneri_metni)`. Task 3 ikisini de kullanır.

- [ ] **Step 1: Write the failing tests**

```rust
#[test]
fn start_suggest_system_defines_konu_contract() {
    let s = start_suggest_system();
    assert!(s.contains("KONU:"));
    assert!(s.contains("first step"));
}

#[test]
fn parse_start_suggestion_splits_slug_and_text() {
    let reply = "KONU: rust-temelleri\nStart with Rust because the backend is Rust.\nFirst step: cargo new.";
    let (slug, text) = parse_start_suggestion(reply).unwrap();
    assert_eq!(slug, "rust-temelleri");
    assert!(text.contains("First step"));
    assert!(!text.contains("KONU:"));
}

#[test]
fn parse_start_suggestion_normalizes_messy_slug_line() {
    let (slug, _) = parse_start_suggestion("KONU: Rust Temelleri!\ngerekçe").unwrap();
    assert_eq!(slug, "rust-temelleri");
}

#[test]
fn parse_start_suggestion_tolerates_missing_text_rejects_missing_konu() {
    let (slug, text) = parse_start_suggestion("KONU: rust").unwrap();
    assert_eq!(slug, "rust");
    assert_eq!(text, "");
    assert!(parse_start_suggestion("just prose, no marker").is_none());
    assert!(parse_start_suggestion("KONU:   \ntext").is_none());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test start_suggest`
Expected: derleme hatası — fonksiyonlar yok

- [ ] **Step 3: Implement**

`src/main.rs`, `slug_system`'in yanına:

```rust
/// System prompt for the one-shot start suggestion (spec: project-aware start).
/// Mirrors the slug mini-session: single call, session reset afterwards.
pub(crate) fn start_suggest_system() -> String {
    "You are Usta, a Socratic engineering mentor. The user has a project \
     definition (given in the user message) but does NOT know where to start \
     learning. Propose the single best starting topic. Reply in the language \
     of the project file. FIRST line must be exactly `KONU: <topic-slug>` \
     (lowercase, hyphenated, 1-3 words). Then 2-4 sentences: why this topic \
     first, and ONE concrete first step small enough to start today. No \
     greeting, no markdown headings, nothing after the suggestion."
        .to_string()
}

/// Parse the suggestion reply: first `KONU:` line → slug (normalized through
/// slugify_topic), remaining lines → suggestion text shown to the user.
/// No `KONU:` marker or empty slug → None (caller falls back to manual entry).
pub(crate) fn parse_start_suggestion(reply: &str) -> Option<(String, String)> {
    let mut lines = reply.trim().lines();
    let first = lines.next()?.trim();
    let rest_raw = first.strip_prefix("KONU:")?;
    let slug = slugify_topic(rest_raw);
    if slug.is_empty() {
        return None;
    }
    let text = lines.collect::<Vec<_>>().join("\n").trim().to_string();
    Some((slug, text))
}
```

Not: `slugify_topic("   ")` boş string döndürmüyorsa (koda bak) `rest_raw.trim().is_empty()` kontrolünü slugify'dan ÖNCE yap — `KONU:` boş satırı `None` dönmeli (test 4 bunu kilitler).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test start_suggest && cargo test parse_start`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "start-öneri: start_suggest_system + parse_start_suggestion (KONU: sözleşmesi)"
```

---

### Task 3: TUI akışı — Suggest kolu (`src/tui/run.rs`)

**Files:**
- Modify: `src/tui/run.rs` (konu-giriş döngüsü, Task 1'in boş bıraktığı `Suggest` kolu, ~satır 476)

**Interfaces:**
- Consumes: Task 1 `TopicChoice::Suggest` + `project_known`; Task 2 `start_suggest_system`/`parse_start_suggestion`; mevcut `ask_live`, `page_notice`, `tui_confirm`, `backend.reset_session()`, `progress::project_md_path`.
- Produces: onaylanan öneri → `break slug` + `intro = Some(...)`; her diğer sonuç → döngü başına (giriş sorusu).

- [ ] **Step 1: Implement the Suggest arm**

Task 1'deki boş `Suggest` kolunu doldur (slug mini-session kolundaki, ~satır 486-503, mekanikle birebir):

```rust
                    Some(crate::TopicChoice::Suggest) => {
                        // One-shot suggestion from mentor/PROJECT.md (spec:
                        // project-aware start). Same mechanics as the slug
                        // mini-session: single call, then ALWAYS reset.
                        let project_md = read(progress::project_md_path(project_root))
                            .unwrap_or_default();
                        let outcome = ask_live(
                            &mut tui,
                            &mut editor,
                            &mut events,
                            backend,
                            &crate::start_suggest_system(),
                            &[Message::user(project_md.as_str())],
                            None,
                        )
                        .await;
                        backend.reset_session(); // suggestion chat must not leak into the session
                        let parsed = match outcome {
                            Ok(AskOutcome::Reply(reply)) => crate::parse_start_suggestion(&reply.text),
                            Ok(AskOutcome::Cancelled) | Err(_) => None,
                        };
                        let Some((slug, text)) = parsed else {
                            page_notice(&mut tui, "suggestion failed — type a topic")?;
                            continue;
                        };
                        if !text.is_empty() {
                            page_notice(&mut tui, &text)?;
                        }
                        if tui_confirm(
                            &mut tui,
                            &editor,
                            &mut events,
                            &format!("start with '{slug}'? [E/h]"),
                        )
                        .await?
                        {
                            page_notice(&mut tui, &format!("topic: {slug}"))?;
                            intro = Some(format!(
                                "Usta's own opening suggestion (already shown to the user, \
                                 they accepted it — continue from its first step, don't repeat it):\n{text}"
                            ));
                            break slug;
                        }
                        page_notice(&mut tui, "cancelled — type a topic")?;
                    }
```

Uyum notları (koda bak, birebir kopyalama yerine mevcut yardımcılara uy):
- `read` closure'ı bu scope'ta `PathBuf` alıyor (`run` başındaki tanım) — imza farklıysa `std::fs::read_to_string(...).ok()` kullan.
- `tui_confirm` çağrı imzası `new_topic_confirm_msg` kullanımıyla aynı olsun (satır ~515).
- `AskOutcome`/`Message` import'ları dosyada zaten var.

- [ ] **Step 2: Compile + full test suite**

Run: `cargo build && cargo test`
Expected: derleme temiz, tüm testler PASS (bu task saf-TUI — yeni unit test yok; mantık Task 1-2 testlerinde)

- [ ] **Step 3: Commit**

```bash
git add src/tui/run.rs
git commit -m "start-öneri: TUI kolu — mini-çağrı, onay, intro taşıma, her yolda session reset"
```

---

### Task 4: Welcome + prompt satırı ipucu

**Files:**
- Modify: `src/tui/welcome.rs` (`render_welcome_identity`, ~satır 208-223) — imzaya `project_known: bool`
- Modify: `src/tui/run.rs` (`ask_topic` ~satır 309 — imzaya `project_known: bool`; prompt satırı ~satır 329; `ask_topic` çağrısı ~satır 435)
- Test: `src/tui/welcome.rs` in-module tests

**Interfaces:**
- Consumes: Task 1'de hesaplanan `project_known` (run.rs scope'unda mevcut — `ask_topic` çağrısına parametre olarak geçir; hesaplamayı çağrıdan ÖNCEYE taşımak gerekirse taşı).
- Produces: koşullu iki UI metni.

- [ ] **Step 1: Write the failing test**

`src/tui/welcome.rs` test modülüne (mevcut `First session` testinin desenini kopyala — render fonksiyonunu aynı argümanlarla çağıran testlere `project_known` ekle):

```rust
#[test]
fn first_session_hint_becomes_suggest_hint_when_project_known() {
    // Call render_welcome_identity twice with empty `local`, flipping only
    // project_known; join the right-column lines as the existing tests do.
    // project_known=false → "First session — type a topic."
    // project_known=true  → contains "PROJECT.md found" and "Enter"
}
```

(Gövdeyi mevcut test yardımcılarıyla doldur — welcome testleri render çıktısını `joined` string'e topluyor, aynı yolu izle.)

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib welcome`
Expected: derleme hatası (parametre) veya assert FAIL

- [ ] **Step 3: Implement**

- `render_welcome_identity` imzasına `project_known: bool`; satır ~223 eski:
```rust
        right.push((fit("First session — type a topic.", right_w), Style::default()));
```
yeni:
```rust
        let first_line = if project_known {
            "PROJECT.md found — press Enter, Usta suggests where to start."
        } else {
            "First session — type a topic."
        };
        right.push((fit(first_line, right_w), Style::default()));
```
- `ask_topic` imzasına `project_known: bool`, `render_welcome_identity` çağrısına geçir; prompt satırı (~329) eski:
```rust
        page_notice(tui, "What do you want to learn? (a word, or describe it in a sentence)")?;
```
yeni:
```rust
        let prompt_line = if project_known {
            "What do you want to learn? (Enter = Usta suggests from PROJECT.md; or type a topic)"
        } else {
            "What do you want to learn? (a word, or describe it in a sentence)"
        };
        page_notice(tui, prompt_line)?;
```
- `run` içindeki `ask_topic` çağrısına (~435) `project_known` argümanını ekle. `project_known` hesabı (Task 1) çağrıdan önce durmalı.
- welcome.rs'deki DİĞER mevcut `render_welcome_identity` test çağrılarına `false` ekle.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test`
Expected: tümü PASS

- [ ] **Step 5: Commit**

```bash
git add src/tui/welcome.rs src/tui/run.rs
git commit -m "start-öneri: welcome + prompt satırı proje-farkında ipucu"
```

---

### Task 5: SPEC.md + final doğrulama

**Files:**
- Modify: `SPEC.md` (madde 52-55 civarı — konu giriş akışı)

- [ ] **Step 1: SPEC.md güncelle**

Konu-giriş maddesine ekle: "İlk oturumda (`local` boş) `mentor/PROJECT.md` doluysa boş Enter = başlangıç önerisi: Usta PROJECT.md'den konu + gerekçe + ilk adım önerir (tek mini-çağrı, sonrası session reset), kullanıcı onaylarsa oturum o konuyla açılır; öneri metni intro olarak onboarding'e taşınır. `local` doluysa boş Enter = resume (öncelikli, değişmedi). Plain/pipe yolunda öneri yok."

- [ ] **Step 2: Full verify**

Run: `cargo test`
Expected: tümü PASS, 0 failed
Run: `cargo clippy --all-targets 2>&1 | grep -c "^warning" || true`
Expected: baseline'dan (2 pre-existing lint: too_many_arguments, bool_assert_comparison) FAZLA yeni uyarı yok. Not: `ask_topic` zaten `#[allow(clippy::too_many_arguments)]` taşıyor — parametre ekleyince lint patlamaz.

- [ ] **Step 3: Commit + install**

```bash
git add SPEC.md
git commit -m "start-öneri: SPEC — proje-farkında başlangıç akışı belgelendi"
cargo install --path .
```

- [ ] **Step 4: Elle doğrulama (Anil ile — ATLA, rapor et)**

`~/Documents/Work/Practice/stagit` içinde `usta` → welcome "PROJECT.md found" demeli → boş Enter → öneri + onay → oturum önerilen konuyla, öneriyi bilerek açılmalı.
