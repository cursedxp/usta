# TERMINOLOGY LOCK — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** `main` dalı, temiz ağaç. **Bu plan `2026-08-31-resize-ghost-frames.md`'den SONRA koşulur** (o v0.29.1'e, bu v0.29.2'ye çıkar); aynı oturumda sırayla yürütülürse resize planı bitip commit'lendikten sonra başlanır. Çakışan dosya yok (o `src/tui/`, bu `SOUL.md` + `src/defaults.rs`). `docs/superpowers/specs/2026-08-31-terminology-lock-design.md` ağaçta OLMALI ve çelişkide o kazanır — özellikle **"Karar" (K1–K4)** ve **"Davranış"taki bağlayıcı blok metni** (birebir yazılır). İş dalını main'den aç (`git switch -c terminology-lock`).

**Goal:** Usta kullanıcının dilinde konuşurken alanın yerleşik terimlerini uyduruk sade karşılıklarla değiştirmeyi bıraksın. Alan-bağımsız: doğru terim "İngilizce olan" değil, **o alanın uygulayıcısının kullanıcının dilinde kullandığı** kelime. Ayrıca terim istikrarı (bir kavram = bir kelime) ve "yanlış cümle kuran sadeleştirme = Rule 2 ihlali" kuralı. Hedef sürüm **v0.29.2**.

**Architecture:** Tek davranış değişikliği `SOUL.md`'de (shipped prompt, `Ownership::Code`). Kod tarafında yalnız pin testleri (`src/defaults.rs`). Kabuk mantığı, parser, token DEĞİŞMEZ — bu bir prompt kuralı, deterministik kabuk işi değil.

**Tech Stack:** Rust (yalnız test modülü + `include_str!`), Markdown prompt dosyaları. Binary crate — filtreli `cargo test <filtre>`.

## Global Constraints

- `SOUL.md`'ye giren blok, spec'in "Davranış" bölümündeki metnin **BİREBİR kendisidir** — yeniden yazılmaz, kısaltılmaz, örnekleri değiştirilmez. Örnekler bilinçli olarak çok-alanlıdır (`index`/`commit`/`deadlift`, `kalp yetmezliği`, `exposure`/`remission`/`consideration`); teknolojiye daraltmak bu planın amacını yok eder.
- Blok LANGUAGE LOCK paragrafının HEMEN ALTINA girer. Voice bölümündeki Jargon rule DEĞİŞMEZ.
- `RULES.md`, `TEACHING.md`, `GOAL.md`, `USER.md`, `src/tokens.rs` DEĞİŞMEZ.
- Her task: TDD — önce failing test, DOĞRU sebeple fail ettiği görülür, sonra minimal değişiklik; task sonunda `cargo test` TÜMÜ yeşil, sonra commit.
- `cargo clippy --all-targets` 0 uyarı; `cargo fmt` yalnız dokunulan dosyaya scoped, sonda `cargo fmt --check` crate-geneli temiz.
- **Push / tag / `cargo install` YOK** — manuel doğrulama sonrası insanın kararı.
- Prompt bütçesi: blok ~1.4KB. `SOUL.md` her turda yükleniyor; bu bilinçli maliyet, prompt diet (v0.19.0) ile çelişmiyor — kabuğun deterministik çözebileceği bir şey değil. Başka bir yerden karşılık kısıntı ARANMAZ.

---

### Task 1: `SOUL.md` — TERMINOLOGY LOCK bloku + pin

**Files:**
- Modify: `SOUL.md` (LANGUAGE LOCK'un hemen altına blok)
- Modify: `src/defaults.rs` (test modülü: 2 yeni test)

**Interfaces:** Kod arayüzü yok. `defaults::global_defaults()` zaten `include_str!("../SOUL.md")` taşıyor — dağıtım kendiliğinden çalışır, yeni kayıt/wiring GEREKMEZ.

- [ ] **Step 1: Failing test.** `src/defaults.rs` test modülüne, mevcut `teaching_promise_matches_ride_along_watcher`'ın yanına ekle:

```rust
#[test]
fn soul_carries_the_terminology_lock() {
    // Live sessions (2026-08-31): mirroring the user's language turned into
    // translating the field's vocabulary — "arguments" became "kelimeler", the
    // same word named index 0 and index 1 four sentences apart, and one
    // simplification stated outright that cargo compiles the code (rustc does).
    // The rule that stops this is prompt text, so the only thing that can guard
    // it is a pin: nobody gets to quietly tidy it away while simplifying Voice.
    let soul = include_str!("../SOUL.md");
    for needle in [
        "TERMINOLOGY LOCK",
        "Simplify the explanation, never the name",
        "practitioners of that field actually use",
        "not protected by looking exotic",
        "One concept, one word",
        "Precision outranks simplicity",
        "never compose in another language and translate",
    ] {
        assert!(soul.contains(needle), "SOUL.md lost the terminology rule: {needle}");
    }
}

#[test]
fn terminology_lock_follows_the_language_lock() {
    // Order carries meaning: which language to write in first, then how to write
    // its vocabulary. Reversed, the terminology rule reads as free-standing
    // style advice instead of the boundary on the language mirror.
    let soul = include_str!("../SOUL.md");
    let language = soul.find("LANGUAGE LOCK").expect("SOUL.md lost the language lock");
    let terminology = soul.find("TERMINOLOGY LOCK").expect("SOUL.md lost the terminology lock");
    assert!(language < terminology);
}
```

- [ ] **Step 2:** `cargo test soul_carries` ve `cargo test terminology_lock_follows` → FAIL (blok yok). Doğru sebep.
- [ ] **Step 3:** Bloğu `SOUL.md`'ye yaz — spec "Davranış" bölümündeki metin BİREBİR, LANGUAGE LOCK paragrafının hemen altına, `## Persona` başlığından önce.
- [ ] **Step 4:** `cargo test` TÜMÜ yeşil (özellikle `defaults` testleri — `returns_all_nonempty_files`, `core_behavior_is_code_owned_learner_is_user_owned` bozulmamalı), `cargo clippy --all-targets`, `cargo fmt -- src/defaults.rs`. Commit: `feat: SOUL.md TERMINOLOGY LOCK — mirror the language, don't translate the field's vocabulary`

---

### Task 2: Belgeleme + sürüm

**Files:**
- Modify: `SPEC.md` (§3 Persona — yeni madde)
- Modify: `docs/ROADMAP.md` (`## Completed` listesinin BAŞINA tarihli kayıt)
- Modify: `Cargo.toml` (`version = "0.29.2"`), `Cargo.lock` (`cargo check` ile tazelenir)

- [ ] **Step 1:** `SPEC.md` §3 Persona'ya İngilizce madde: dili aynalamak alanın sözlüğünü çevirmek DEĞİLDİR · doğru terim uygulayıcının kullanıcının dilinde kullandığı kelimedir (ödünç ya da yerli — alan-bağımsız) · bir kavram bir kelime · yanlış cümle kuran sadeleştirme Rule 2 ihlalidir · `SOUL.md` kaynağı + tasarım dosyası yolu.
- [ ] **Step 2:** `docs/ROADMAP.md` `## Completed` listesinin en başına tek paragraflık kayıt (mevcut biçim: `- 2026-08-31: <başlık> — <ne değişti, neden, kanıt>. Design: <spec yolu>. v0.29.2.`). Kanıt olarak iki canlı oturum bulgusunu adıyla an: "kelime"/argüman, "birinci" ikili anlamı, dosya↔binary, çevirme↔derleme, cargo≠derleyici.
- [ ] **Step 3:** `Cargo.toml` sürüm 0.29.2, `cargo check` ile lock tazele.
- [ ] **Step 4:** `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`. Commit: `docs: SPEC 3 + ROADMAP — terminology lock; bump to v0.29.2`

---

## Manuel Doğrulama (Anil ile, plan bittikten sonra)

`cargo install --path .` sonrası (mevcut global kurulum `Ownership::Code` olduğu için `SOUL.md` üzerine yazılır — doğrula: `grep -c "TERMINOLOGY LOCK" ~/.config/usta/SOUL.md` — global kök `~/.config/usta`, `~/.usta` DEĞİL (`src/config.rs::global_root`); ayrıca `usta init` gerekmez, `ensure_scaffold` her açılışta code-owned dosyaları tazeler):

1. Aynı `std::env::args()` sorusunu tekrar sor → **"argüman"** geçmeli, "kelime" GEÇMEMELİ; index 0 ve index 1 tek ve ayrı adlarla anılmalı.
2. Aynı cargo/stagit sorusunu sor → derleyenin `rustc` olduğu, cargo'nun onu sürdüğü doğru kurulmalı; "dosya"/"binary" ve "çevirme"/"derleme" karışmamalı.
3. **Teknoloji dışı bir alan dene** (sağlık, psikoloji veya hukuk konusu) → uydurma sade karşılık yerine alanın gerçek terimi + tek cümlelik açıklama. Kuralın alan-bağımsız çalıştığının asıl testi bu.
4. İngilizce bir oturum aç → blok İngilizce yanıtta gereksiz gürültü yaratmamalı (terim zaten yerinde), üslup değişmemeli.
