# Prompt Diyeti Implementation Plan (v0.19.0)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** v0.18.5 (300 test yeşil). Spec: `docs/superpowers/specs/2026-08-16-prompt-diet-design.md` — önce oku, bağlayıcı ilke orada.

**Goal:** Gamification/Material/Prediction brain bölümleri koşullu yüklenir; vadeli-soru seçimi kabuğa geçer; ölü KEEP cümlesi silinir. v0.19.0.

**Architecture:** GOAL.md koşullu-yükleme deseni üç yeni Code-owned dosyaya kopyalanır (`GAMIFICATION.md`, `MATERIAL.md`, `PREDICTION.md`); `due_questions` tek-kaynak (due_count onun len'i olur); opening_prompt talimat yerine seçilmiş maddeleri taşır.

## Global Constraints

- Brain içeriği DEĞİŞMEZ — yalnız taşınır (bölüm metinleri birebir yeni dosyalara; diff'te içerik kaybı olmadığı doğrulanır).
- Koşul false iken ilgili içerik prompt'ta HİÇ olmamalı (negatif assert'ler şart).
- Davranış regresyonu: koşul true iken bugünkü prompt içeriğiyle eşdeğer.
- Binary crate — `cargo test <filtre>`. Her task Türkçe commit + push; sonda 0.19.0 + sürüm testi + tag + `cargo install --path .`. clippy yeni uyarı 0.

---

### Task 1: `GAMIFICATION.md` ayrımı + koşullu yükleme + `/game on` gömme

**Files:** Create: `GAMIFICATION.md` · Modify: `TEACHING.md` (bölüm çıkar), `src/defaults.rs` (listeye ekle, Code-owned), `src/brain.rs` (koşullu yükleme), `/game on` enjeksiyon turunun kurulduğu yerler (src/tui/run.rs + src/main.rs) · testler ilgili modüllerde

**Adımlar (TDD):**
- [ ] Failing testler: (a) brain testi — USER.md'de `- gamification: on` satırı VARKEN prompt GAMIFICATION içeriğini içerir, YOKKEN o içerikten tek cümle bile yok (GOAL testi deseni, tmpdir); (b) defaults testi üç... bu task'ta bir yeni dosyayı kapsar; (c) `/game on` turunun metni GAMIFICATION.md içeriğini gömer (turu kuran fonksiyonu saf yap: `game_on_turn(rules: &str) -> String` + çağrı yerleri global'den okur).
- [ ] Implement: `## Gamification` bölümünü TEACHING.md'den KES → `GAMIFICATION.md` (başlık dahil, birebir). defaults.rs: `("GAMIFICATION.md", include_str!("../GAMIFICATION.md"), Ownership::Code)`. brain.rs: USER.md okuma noktasında içerikte `- gamification: on` satırı varsa `read_section(&global.join("GAMIFICATION.md"), ...)` (GOAL bloğunun yanına, aynı desen). `/game on` turu: `game_on_turn(&std::fs::read_to_string(global.join("GAMIFICATION.md")).unwrap_or_default())` — okunamazsa eski kısa metne düş (fallback, oturum kırılmaz).
- [ ] `cargo test` yeşil → commit + push: `diyet: gamification kuralları koşullu — oyun kapalıyken prompt'ta yok`

---

### Task 2: `MATERIAL.md` + `PREDICTION.md` ayrımı + koşulları

**Files:** Create: `MATERIAL.md`, `PREDICTION.md` · Modify: `TEACHING.md`, `src/defaults.rs`, `src/brain.rs`, `src/materials.rs` (`materials_present`) · testler

**Adımlar (TDD):**
- [ ] Failing testler: `materials_present` (md/txt/pdf → true; boş veya yalnız `.gitkeep` → false; klasör yok → false); brain koşullu ×2 (materials var/yok, Cargo.toml var/yok — pozitif+negatif assert'ler); defaults kapsamı.
- [ ] Implement: iki bölüm TEACHING'den birebir kesilir. `materials_present`: hafif taramalı varlık kontrolü (digest YOK — collect_files'ı yeniden kullan, uzantı filtresi md/txt/pdf). Cargo koşulu: check.rs'in cargo tespit mantığı neyse ONU paylaş (fonksiyonu pub yap / ortak yardımcı — kopya kural yazma). brain.rs: `load_system_prompt` proje parametresi üzerinden iki koşullu `read_section`.
- [ ] DİKKAT: `load_system_prompt` imzası proje kökünü zaten alıyor — koşullar orada hesaplanır; çağrı imzası değişmez (değişmesi gerekirse tüm call site'ları aynı task'ta güncelle).
- [ ] `cargo test` yeşil → commit + push: `diyet: material + prediction kuralları koşullu — gereksiz oturumda taşınmaz`

---

### Task 3: Vadeli soru seçimi kabuğa (`due_questions` + opening_prompt yeniden şekillendirme)

**Files:** Modify: `src/tui/welcome.rs` (`due_questions` + `due_count` tek-kaynaklaştırma), `src/progress.rs` (`opening_prompt`), çağrı yerleri (src/tui/run.rs + src/main.rs) · testler

**Adımlar (TDD):**
- [ ] Failing testler: `due_questions` (geçmiş+bugün+kuyruksuz seçilir; gelecek elenmez→elenir; en eski due önce; 3 tavanı; başka bölüm maddeleri dışarıda; `due_count(p,t) == due_questions(p,t).len()`); `opening_prompt` üç dal (spec Test bölümü — dolu liste maddeleri içerir + eski filtreleme talimatı YOK negatif assert'i; boş+soru-var → "no reviews due today" atlama cümlesi; hiç-soru-yok kuralı korunur).
- [ ] Implement: `due_questions` — mevcut `due_count` bölüm-tarama mantığını genelleştir (satır + due tarihi topla, kuyruksuz = bugün kabul et, tarihe göre stable sort, truncate 3); `due_count` = `due_questions(...).len()` (mevcut due_count testleri KIRILMAMALI). `opening_prompt` imzası: filtreleme talimatı yerine seçilmiş maddeleri (veya boş-durum bilgisini) alan parametre — spec'teki üç dal; `game_streak`/`project_known` parametreleri aynen. Çağrı yerleri progress içeriğini zaten okuyor — `due_questions` çağrısı + geçiş; `today` her iki yerde mevcut.
- [ ] `cargo test` yeşil → commit + push: `diyet: vadeli soru seçimi kabukta — drill listesi deterministik`

---

### Task 4: Ölü KEEP cümlesi

**Files:** Modify: `src/progress.rs` (closing_prompt) · test silme

- [ ] closing_prompt'tan "KEEP the '## Tercihler' ..." cümlesi silinir; `closing_prompt_protects_tercihler_section` testi silinir (restore_game_pref testleri güvenceyi zaten veriyor — kontrol et, varlar).
- [ ] `cargo test` yeşil → commit + push: `diyet: ölü Tercihler talimatı kaldırıldı — kabuk restore garantisi yeter`

---

### Task 5: Docs + v0.19.0

**Files:** `SPEC.md`, `README.md`, `Cargo.toml`(+lock), sürüm testi

- [ ] SPEC yeni §: koşullu brain tablosu (dosya | koşul: GOAL→## Hedef, GAMIFICATION→profil on, MATERIAL→materials/ dolu, PREDICTION→Cargo.toml) + due-seçimi kabukta + prompt-diyet ilkesi tek cümle.
- [ ] README: brain ağacına üç yeni dosya + koşul notu (İngilizce, kısa).
- [ ] Cargo `0.19.0`; sürüm testi; `cargo build`.
- [ ] Verify: `cargo test` PASS · clippy yeni uyarı 0 · `cargo install --path .`.
- [ ] Commit + push + `git tag v0.19.0 && git push --tags`: `diyet: SPEC + README — v0.19.0`
- [ ] (Elle doğrulama — ATLA, Anil koşacak): oyun kapalı oturumda transcript'te gamification kuralları görünmemeli; `/game on` turu kuralları getirmeli; Rust-dışı projede prediction yok; drill yalnız kabuk-seçimi soruları sormalı.
