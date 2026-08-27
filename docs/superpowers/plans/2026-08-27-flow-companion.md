# Flow Companion Implementation Plan (v0.25.0)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** v0.24.7 üstüne. Spec: `docs/superpowers/specs/2026-08-27-flow-companion-design.md` — TAMAMINI oku ("İsimlendirme" bölümü bağlayıcı). v0.24.x kuyruk tarihçesi: `docs/superpowers/specs/2026-08-26-polite-watcher-design.md` (artık büyük ölçüde superseded — yeni davranışın referansı flow-companion spec'idir).

**Goal:** Kuyruk/bekletme modeli kalkar; dosya değişikliği anında + batch tek LLM turunda işlenir; polite=true akış-içi eşlik çerçevesi (`flow_frame`), polite=false düz inceleme. v0.25.0.

**Architecture:** Batch birleştirme `file_feedback.rs`'te (`handle_batch_change`), çerçeve metinleri `file_feedback.rs`'te (`flow_frame` + mevcut `feedback_frame`), kaldırmalar `polite.rs` + `run.rs`'te. run.rs küçülür (kuyruk/backstop/soru-takibi dalları silinir).

**Tech Stack:** Rust, tokio. Binary crate — `cargo test <filtre>`.

## Global Constraints

- TÜM yeni kod adları, string'ler, yorumlar, commit mesajları İNGİLİZCE.
- `run.rs` production ≤600 satır (küçülmesi beklenir — kaldırma işi).
- TDD; her task sonunda `cargo build && cargo test` yeşil, clippy yeni uyarı 0 (ölü kod uyarısı dahil), fmt dokunulan dosyalara, İngilizce commit + push.
- `watching=false` ve exercise/silent-skip sınıflandırmaları davranış değiştirmez.
- Görev sırası önemli: önce EKLE (Task 1-2), sonra KALDIR (Task 3) — ara commit'lerde geçici `#[allow(dead_code)]` YOK; gerekiyorsa Task 1-2'nin ürünleri Task 3'te bağlanana dek test-yalnız kullanım kabul (reviewer'a not düş).

---

### Task 1: `flow_frame` — akış-içi çerçeve (`src/file_feedback.rs`)

**Files:**
- Modify: `src/file_feedback.rs` (yeni fn + testler in-module)

**Interfaces (Produces):**
- `pub(crate) fn flow_frame(files_payload: &str, any_exercise: bool) -> String` — spec'in dört kuralını içeren İngilizce çerçeve; `files_payload` = Task 2'nin birleşik dosya bloğu. Exercise varsa mevcut "AS AN EXERCISE" kuralı çerçeveye eklenir (mevcut `feedback_frame`'deki metni YENİDEN KULLAN — kopyalama, ortak sabite çek).
- Mevcut `feedback_frame` DEĞİŞMEZ (live mod + plain yol kullanmaya devam eder).

- [ ] **Step 1: Failing testler**

```rust
#[test]
fn flow_frame_pins_the_four_lesson_rules() {
    let s = flow_frame("FILE: src/main.rs\n...", false);
    // (1) step check + advance, (2) keep open question alive,
    // (3) scaffold in one sentence, (4) answer then recall the task
    assert!(s.contains("part of the ongoing lesson"));
    assert!(s.contains("next step"));
    assert!(s.contains("unanswered question"));
    assert!(s.contains("scaffold"));
    assert!(s.contains("hand-written"));
    assert!(!s.to_lowercase().contains("standalone code review"));
}

#[test]
fn flow_frame_carries_exercise_rule_when_flagged() {
    assert!(flow_frame("x", true).contains("AS AN EXERCISE"));
    assert!(!flow_frame("x", false).contains("AS AN EXERCISE"));
}
```

(`standalone code review` yasağı: çerçeve "this is NOT a standalone code review" cümlesini pozitif kurar — assert'i gerçek cümleye göre uyarla; niyet: çerçevede bağımsız-inceleme MODUNU emreden dil olmaması.)

- [ ] **Step 2:** `cargo test flow_frame` → derleme hatası
- [ ] **Step 3:** Implement — spec "Davranış > Çerçeve" maddesindeki dört kuralı İngilizce, kısa ve emir kipinde yaz; exercise sabitini `feedback_frame` ile paylaş.
- [ ] **Step 4:** `cargo test file_feedback` → PASS (mevcut `feedback_frame` testleri dahil)
- [ ] **Step 5:** Commit + push: `feat: flow_frame — lesson-flow feedback framing`

---

### Task 2: `handle_batch_change` — N dosya, tek LLM turu (`src/file_feedback.rs`)

**Files:**
- Modify: `src/file_feedback.rs` (yeni fn + testler)

**Interfaces:**
- Consumes: `feedback::FileMemory::observe`, `ChangePayload{Skip,TooLarge,FirstSight,Diff}`, `is_exercise_path`, `is_silent_skip`, mevcut `handle_file_change` (yapı şablonu — cargo check bloğu, injected turn kurulumu, Recorder; koda bak).
- Produces: `pub(crate) async fn handle_batch_change(backend, session, files, project_root, paths: &[PathBuf], recorder, polite: bool) -> Result<FileFeedback>` — batch'teki her dosya: okunur, `observe` edilir; Skip → düşer, TooLarge → dosya-başına `Bildirim` toplanır (mevcut metin), silent-skip sınıfları sessiz düşer. Kalan dosyalar `FILE: <path>` başlıklı bloklar hâlinde tek payload'a birleşir; çerçeve `polite` bayrağına göre `flow_frame` / `feedback_frame`(çok-dosya uyarlaması — koda bak, feedback_frame tek dosya imzalıysa payload'ı tek blok olarak ver). Cargo check TUR BAŞINA bir kez, sonuç mevcut "Usta's eyes only" bloğuyla eklenir. Tüm dosyalar düştüyse LLM çağrısı YOK (`Sessiz`/`Bildirim` birleşimi döner). Tek injected user turn + tek LLM çağrısı; dönüş mevcut `FileFeedback::Yanit` şekli.

- [ ] **Step 1: Failing testler** (LLM çağrısız test edilebilir kısım: payload birleştirme + eleme mantığı — gerekiyorsa saf yardımcı `build_batch_payload(files, paths) -> (String, BatchMeta)` çıkar ve ONU test et; async LLM'li gövde ince kalır):

```rust
#[test]
fn batch_payload_merges_files_and_drops_skips() {
    // tmpdir: iki gerçek dosya (biri baseline'da aynı → Skip, biri değişmiş → Diff)
    // + bir binary dosya (silent skip sınıfı)
    // build_batch_payload → payload yalnız değişmiş dosyayı içerir, "FILE:" başlıklı;
    // meta.total_included == 1; skip edilen dosya payload'da GEÇMEZ.
}

#[test]
fn batch_payload_orders_files_deterministically() {
    // aynı batch iki kez → payload byte-identical (sıra: verilen paths sırası)
}
```

(Gövdeler mevcut file_feedback test desenleriyle — tmpdir + gerçek dosya + `FileMemory`. İmzayı koda göre kur.)

- [ ] **Step 2:** `cargo test batch_payload` → derleme hatası
- [ ] **Step 3:** Implement — `build_batch_payload` + `handle_batch_change`. `handle_file_change` bu aşamada DURUYOR (Task 3 kaldıracak veya tek-dosyalık sarmalayıcıya indirecek — plain.rs kullanıyorsa sarmalayıcı kalır, koda bak).
- [ ] **Step 4:** `cargo test file_feedback` → PASS
- [ ] **Step 5:** Commit + push: `feat: handle_batch_change — one LLM turn per debounce batch`

---

### Task 3: Kaldırma + yeniden bağlama (`run.rs`, `polite.rs`, `slash.rs` mesajları)

**Files:**
- Modify: `src/tui/run.rs` (watcher dalları, kuyruk/backstop/soru-takibi silinir), `src/tui/polite.rs` (kuyruk aygıtı + testleri silinir; `route` üç kollu olur; pin testleri güncellenir), `src/slash.rs` (yalnız `apply_polite` mesaj metinleri), `src/file_feedback.rs` (gerekirse `handle_file_change` → sarmalayıcı/kaldırma)

**Interfaces (Consumes):** Task 1-2 ürünleri; spec "İsimlendirme > Kaldırılanlar" listesi.

- [ ] **Step 1:** `route()` üç kollu: `Queue` varyantı ve `polite/question_open` parametreleri kalkar → `route(batch_len, max_batch, watching) -> Route{Bulk,ObserveOnly,Feedback}`; doğruluk tablosu testi güncellenir.
- [ ] **Step 2:** run.rs watcher dalı: Feedback kolu `handle_batch_change(..., &batch, ..., polite)` tek çağrı olur (dosya-başına döngü kalkar; `Yanit/Bildirim/Sessiz` kolları mevcut sunumla). Kuyruk push kolu, backstop `select!` dalı, `question_open` set/temizleme noktaları, cevap-sonrası flush bloğu, `last_key`'in backstop kullanımı SİLİNİR (`last_key` başka yerde kullanılmıyorsa değişkeni de kaldır).
- [ ] **Step 3:** polite.rs: spec listesindeki semboller + testleri silinir; `process_paths` artık yalnız... koda bak — `handle_batch_change` sunum katmanını (page_reply vb.) run.rs'te mi polite.rs'te mi bırakmak daha küçükse onu yap; pin testi needle listesi: kaldırılanlar çıkar, `handle_batch_change` + `flow_frame` + `polite::route(` girer.
- [ ] **Step 4:** slash.rs `apply_polite` mesajları: on → `"polite mode on — companion follows your lesson flow"`, off → `"polite mode off — plain review feedback"`; testler güncellenir. `/watch off` "(pending feedback dropped)" eki kalkar (kuyruk yok).
- [ ] **Step 5:** `cargo build && cargo test` → TÜMÜ PASS; `cargo clippy --all-targets` 0 uyarı (ölü kod dahil — kaldırma tam olmalı); `wc -l src/tui/run.rs` raporla (küçülmüş olmalı).
- [ ] **Step 6:** Commit + push: `feat: retire the polite queue — immediate batched lesson-flow feedback`

---

### Task 4: Docs + v0.25.0 release

**Files:** `README.md`, `SPEC.md`, `src/help.rs` (gerekirse), `Cargo.toml`, `Cargo.lock`, sürüm testi (grep `0.24.7` src/)

- [ ] **Step 1:** SPEC §4.21 sonuna **v0.25.0** paragrafı: kuyruk/backstop kaldırıldı; feedback anında + batch tek turda; polite = çerçeve anahtarı (flow vs plain review); `watch: live` ve `/watch polite` anlamı buna göre. §9'daki v0.24.1-4 satırlarına dokunma (tarihçe), ama §4.21 içinde superseded olduğu tek cümleyle işaretlenebilir.
- [ ] **Step 2:** README polite bölümü yeniden yazılır (İngilizce): lesson-flow companion davranışı, `/watch polite off` = plain review, 180s/backstop referansları SİLİNİR.
- [ ] **Step 3:** help.rs `/watch polite` satırı yeni anlama göre: `queue file feedback...` → `lesson-flow feedback framing (default: on)` benzeri — koda bak.
- [ ] **Step 4:** Cargo.toml `0.25.0`; sürüm testi güncelle.
- [ ] **Step 5:** Verify: `cargo build && cargo test` PASS · clippy 0 · `cargo install --path .`
- [ ] **Step 6:** Commit + push + tag:

```bash
git add -A
git commit -m "feat: flow companion — v0.25.0"
git push
git tag v0.25.0 && git push --tags
```

- [ ] **Step 7 (elle doğrulama — ATLA, Anil koşacak):** mentor adım verir → yap → ANINDA teyit + sıradaki adım · `cargo new` → TEK toplu yanıt, scaffold tek cümle · araya soru sor → cevap + göreve dönüş · `/watch polite off` → düz inceleme tonu · `/watch off` → sessizlik.
