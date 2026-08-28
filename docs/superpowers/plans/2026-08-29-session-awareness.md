# Session Awareness — Identity, Verdict Memory, Structure Signal, Audit, /context — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** `main` dalı, v0.28.0, temiz ağaç. `docs/superpowers/specs/2026-08-29-session-awareness-design.md` ağaçta OLMALI ve çelişkide o kazanır. Önce spec'i oku, özellikle "İsimlendirme (bağlayıcı)", "Kararlar" (A1, C1–C2, D1–D2, E1, F1) ve "Davranış (bağlayıcı detay)" bölümleri. İş dalını main'den aç (`git switch -c session-awareness`). Kanıt zemini `.superpowers/sdd/progress.md` "CANLI KULLANIM BULGULARI — v0.28.0" bölümüdür — görev gerekçelerindeki BULGU A/C/D/E atıfları oraya gider.

**Goal:** Beş iş, tek kök tema — Usta'nın dünya modeli yalnız dosya-içeriği kaydıyla tazeleniyor, bu tur eksik yarıları tamamlıyor: **(A1)** tanışma artık kullanıcının KİM olduğunu da öğreniyor (isim/arka plan/öğrenme tarzı — form değil, konuşma). **(C1/C2)** doğrulama kararnamesi HATIRLANIYOR: check bugünkü yerlerinde koşmaya devam eder (yeni koşum YOK), sonucu `check::VerifyMonitor`'da saklanır; kırmızıyken her teslim edilen tur tek satırlık `[build state: …]` notu taşır ve durum satırı dim `✗ check failing ` gösterir; doğrulayıcısı olmayan projede (Cargo-dışı her şey) özellik sessiz no-op. **(D1/D2)** yapısal değişiklikler görünür: yeni dizin, silinen bilinen dosya/dizin `STRUCTURE:` satırı olarak `PendingChanges`'te birikir ve ride-along ile gider — içerik asla, yalnız yol. **(E1)** `flow_frame` OKU/YAZ asimetrisini açık yazar ve teslim edilen değişikliğin ödeve karşı üç parçalı denetimini (ne yapılmış / ne eksik / tek sonraki adım) kural yapar. **(F1)** `/context` bağlam penceresini neyin doldurduğunu kesin byte ile döker (token rakamları tahmin etiketli). Hedef sürüm **v0.29.0**. K1 (gözcü asla tur açmaz) dokunulmaz.

**Architecture:** Her yeni mekanizma deterministik kabuk işi — modele yalnız payload + çerçeve + tek satırlık durum notu gider. Saf mantık dört yerde: `src/watcher.rs` (`StructureTracker`, tür-farkındalıklı `should_forward`), `src/tui/polite.rs` (`classify_flush`, notlu `PendingChanges`, `handle_watch_command`), `src/check.rs` (`Verdict`, `VerifyMonitor` — nötr isimler bilinçli: kavram "projenin doğrulama sinyali", cargo bugünkü tek implementasyon), `src/context_report.rs` (yeni; `brain::section_sizes` ile beslenir). `src/tui/run.rs` (596/600) yalnız ince çağrı yerleri tutar — /watch komut kolu `polite::handle_watch_command`'a taşınarak yer AÇILIR (Görev 6), eklenen tracker/monitör init + `/context` dalı o alana sığar; beklenen final ≈ 590.

**Tech Stack:** Rust, tokio `select!` (run.rs döngüsü), notify v6 (`EventKind` türleri Görev 3'te genişliyor), ratatui inline viewport. Binary crate — test filtreli `cargo test <filtre>`. `tokio::select!` gövdesini rustfmt biçimlemez — run.rs'in kol içi uzun tek-satır çağrıları bilinçlidir, bütçe bu sayede tutar.

## Global Constraints

- TÜM yeni tanımlayıcılar, kullanıcıya dönük string'ler, kod yorumları ve commit mesajları İNGİLİZCE (spec "İsimlendirme" bölümü birebir bağlayıcı: `StructureTracker`, `classify_flush`, `VerifyMonitor`, `Verdict`, `handle_watch_command`, `section_sizes`, `is_context_command`, `STRUCTURE: project tree changes`, `✗ check failing `, `[build state: …]`). Türkçe yalnız spec düzyazısında.
- Her task: TDD — önce failing test, doğru sebeple fail ettiği görülür, sonra minimal implementasyon; task sonunda `cargo test` TÜMÜ yeşil, sonra commit. **Push / tag / `cargo install` YOK** — bunlar manuel doğrulama sonrası insanın kararı; plan yalnız lokal commit üretir.
- `cargo clippy --all-targets` her task sonunda **0 uyarı**. Henüz tüketilmeyen aşamalı öğeler `#[allow(dead_code)]` alır (Görev 5) ve tüketen task'ta (Görev 6) SÖKÜLÜR.
- `cargo fmt` yalnız dokunulan dosyalara scoped (`cargo fmt -- <dosyalar>`); en sonda (Görev 8) `cargo fmt --check` crate-genelinde temiz.
- `src/tui/run.rs` 596 satırda, 600 hard bütçe — run.rs'e dokunan HER task sonunda `grep -c "" src/tui/run.rs` ≤ 600 doğrulanır. Görev 4 +1 (≈597), Görev 6 net −12 (≈585, /watch kolu `polite::handle_watch_command`'a taşınır), Görev 7 +6 (≈591). Sapma varsa yorum satırlarında fazlalık aranır, koddan kırpılmaz.
- `src/plain.rs` DEĞİŞMEZ (kaynak olarak da davranış olarak da: genişleyen watcher kanalından plain'e düşen dizin/silinme yolları `handle_file_change`'in mevcut silent-skip sınıflarıyla — NotFound/IsADirectory — sessizce yutulur; Görev 3 bunu ayrıca doğrular). Plain'de yapı takibi, monitör ve `/context` YOK.
- Davranış regresyonu yasak: `usta start <topic>`, resume, lock-çakışma onayı, katalog upsert, transcript kaydı, altı-dosyalık closing flush, `/watch on|off|live`, bulk-skip, ride-along sözleşmesi (dosya bloğu önce — kullanıcının sözü SON), K1 (gözcü hiçbir koşulda tur açmaz — `VerifyMonitor` ve `StructureTracker` dahil hiçbir yeni mekanizma tur BAŞLATAMAZ).
- **Check koşum noktaları DEĞİŞMEZ** (spec C2): ride-along teslimi (≥1 non-exercise içerik), live batch turu, plain tek-dosya. Yeni koşum eklemek — kullanıcı turunda, flush'ta, arkaplanda — YASAK; bulgu C'nin düzeltmesi hafızadır, sıklık değil.
- Prompt diet: ölçüm, önbellek, yapı sınıflandırması ve `/context` dökümü tamamen deterministik kabuk işi. Ambient panelde LLM üretimi metin YASAK (K3).
- Pin testleri gevşetilmez (`run_rs_wiring_call_sites_are_pinned`, `dispatch_flush_route_arms_are_pinned`, `ride_along_attaches_only_to_genuine_user_text`, `run_wires_intro_flow` — iki sessiz silme + iki vakum iğne emsali). İmzası değişen çağrı yerinin iğnesi hâlâ ISIRAN eşdeğeriyle değiştirilir ve her yeni/değişen iğne şu protokolle doğrulanır: **çağrıyı geçici olarak yorum satırı yap → ilgili pin testinin FAIL ettiğini gör → geri al** (derleme hatası da "ısırma" sayılır — mekanik pin). Bu doğrulama, ilgili adımda açıkça yapılır ve commit mesajına yazılmaz.

---

### Task 1: Tanışma kimliği de sorar (spec A1 — BULGU A)

**Files:**
- Modify: `src/progress.rs` (`introduction_prompt` gövdesine kimlik bloğu)
- Modify: `src/progress_tests.rs` (1 yeni test)

**Interfaces:**
- Consumes: `progress::introduction_prompt(project_known: bool, materials: Option<&str>) -> String` (mevcut, imza değişmiyor).
- Produces: aynı fonksiyon, gövdesinde kimlik bloğu. `MEET_BLOCK` (`progress.rs:48`) DEĞİŞMEZ — plain yolu ve `usta start <topic>` onu kullanmaya devam eder. Başka task buna bağımlı değil.

- [ ] **Step 1: Failing test.** `src/progress_tests.rs`'te `introduction_prompt_carries_rules_marker_and_role` testinin ALTINA ekle:

```rust
#[test]
fn introduction_prompt_asks_who_the_learner_is() {
    // Finding A (v0.28.0 live session): the introduction inferred role and
    // level but never asked WHO the learner is, so USER.md's Who section
    // stayed empty and every later session opened with a stranger — against
    // 13a's own rationale that the introduction fills the profile. The old
    // MEET_BLOCK asked these on the post-lock paths; the pre-lock intro must
    // ask them too, woven in, never as a form.
    let p = introduction_prompt(false, None);
    assert!(p.contains("their name"));
    assert!(p.contains("how they like to learn"));
    assert!(p.contains("Who section"));
    assert!(p.contains("never as a form"));
    // The identity block must justify itself under rule 1, not suspend it.
    assert!(p.contains("pass rule 1"));
    // The three conversation rules and the role inference must survive intact.
    assert!(p.contains("changes what you do next"));
    assert!(p.contains("honor it without argument"));
    assert!(p.contains("never ask as a menu"));
}
```

- [ ] **Step 2:** `cargo test introduction_prompt_asks_who` → Beklenen: **FAIL** (`their name` iğnesi yok).
- [ ] **Step 3: Minimal implementasyon.** `src/progress.rs` içinde `introduction_prompt`'un `format!` gövdesinde, şu mevcut satırların

```
         Never infer boredom or disinterest from short answers — a yes/no \
         question earns a yes/no; judge by what they say, not how much.\n\
```

hemen ALTINA (yani `Also infer — never ask as a menu` paragrafından önce) şu bloğu ekle:

```
         Meet them as a person, too: early on — woven into the conversation, \
         never as a form — learn their name, what they have done before that \
         touches this area, and how they like to learn (reading first, \
         building first, being quizzed). These questions pass rule 1: every \
         answer changes how you calibrate. At session close they fill the \
         profile's Who section — skip them and every later session starts \
         with a stranger.\n\
```

- [ ] **Step 4:** `cargo test introduction_prompt` → yeni test + `introduction_prompt_carries_rules_marker_and_role` + `introduction_prompt_embeds_material_digest` hepsi PASS. Ardından `cargo test` (tümü) → yeşil.
- [ ] **Step 5:** `cargo fmt -- src/progress.rs src/progress_tests.rs` · `cargo clippy --all-targets` → 0 uyarı.
- [ ] **Step 6:** Commit:

```bash
git add src/progress.rs src/progress_tests.rs
git commit -m "fix: first-run introduction asks who the learner is — name, background, learning style (finding A)"
```

---

### Task 2: flow_frame — OKU/YAZ asimetrisi + üç parçalı denetim (spec E1 — BULGU E)

**Files:**
- Modify: `src/file_feedback.rs` (`flow_frame` gövdesi + `flow_frame_pins_the_five_lesson_rules` testinin yenilenmesi)

**Interfaces:**
- Consumes: `file_feedback::flow_frame(files_payload: &str, any_exercise: bool) -> String` (mevcut, `pub(crate)`, imza değişmiyor); `EXERCISE_REVIEW_RULE` (değişmiyor).
- Produces: aynı imza, kural 1 ve 5 yeniden yazılmış; kural 2/3/4 ve `if any_exercise` kuyruğu BİREBİR aynı. Görev 4'ün structure-only teslim testi bu frame'i sarar — "part of the ongoing lesson" iğnesi korunmalı.

Not: v0.28.0'ın rule 5'i doğruydu ama tek yönlüydü — model "okuman söyleneni anlatma"yı "yazman söyleneni de değerlendirme" diye genelleyebiliyordu (BULGU E "EK RİSK"). Ayrım artık açık: OKU → rapor gelene kadar OFF-LIMITS; YAZ → denetle. Hard Rule 2 aynen: eksik söylenir, çözüm yazılmaz.

- [ ] **Step 1: Failing test.** `src/file_feedback.rs` test modülünde `flow_frame_pins_the_five_lesson_rules` testini SİL ve yerine koy:

```rust
    #[test]
    fn flow_frame_pins_the_audit_and_asymmetry_rules() {
        let s = flow_frame("FILE: src/main.rs\n...", false);
        // (1) WRITE-assignments are audited in three parts against the
        // assignment (finding E: the user's answering is already proof they
        // made the change; the reply must account for the change, not just
        // the words), (2) one-sentence nudge — never a full repeat,
        // (3) scaffold in one sentence, (4) answer then recall the task,
        // (5) the artifact's PURPOSE decides: READ → off-limits until the
        // report; WRITE → auditing it is the point.
        assert!(s.contains("part of the ongoing lesson"));
        assert!(s.contains("audit the delivered change against the assignment"));
        assert!(s.contains("three parts"));
        assert!(s.contains("what is missing or wrong"));
        assert!(s.contains("single next step"));
        assert!(s.contains("never write the fix"));
        assert!(s.contains("unanswered question"));
        assert!(s.contains("ONE short sentence"));
        assert!(s.contains("never repeat the full question"));
        assert!(s.contains("scaffold"));
        assert!(s.contains("hand-written"));
        assert!(s.contains("PURPOSE"));
        assert!(s.contains("READ, RUN, or DESCRIBE"));
        assert!(s.contains("OFF-LIMITS"));
        assert!(s.contains("only acknowledge that the step happened"));
        assert!(s.contains("say nothing about the artifact's content"));
        assert!(s.contains("verify it against"));
        assert!(s.contains("WRITE or CHANGE"));
        // The over-correction risk finding E named: rule 5 must never read
        // as a licence to stay silent about assigned WRITE work.
        assert!(s.contains("stay silent about work you assigned"));
        assert!(!s.contains("describe only the change"));
        assert!(!s.to_lowercase().contains("standalone code review"));
    }
```

- [ ] **Step 2:** `cargo test flow_frame_pins` → Beklenen: **FAIL** (`audit the delivered change against the assignment` iğnesi yok; eski test adı artık derlenmiyor olabilir — o da doğru sebeple fail'dir).
- [ ] **Step 3: Implementasyon.** `src/file_feedback.rs` içinde `flow_frame`'in `format!` bloğunu şu gövdeyle değiştir (kural 2, 3, 4 birebir aynı; kural 1 ve 5 yeni; `if any_exercise` kuyruğu değişmiyor):

```rust
    let mut frame = format!(
        "[Files changed]\n{files_payload}\n\n\
This change is part of the ongoing lesson — respond as the mentor guiding it, not as a reviewer opening a fresh audit. Apply these rules:\n\
1. If your last message asked the user to WRITE or CHANGE something and this change is their delivery: audit the delivered change against the assignment, and shape your reply in three parts — what they did (named concretely, with evidence from the change), what is missing or wrong (said plainly), and the single next step. Answering their words alone is not enough — hold the change against the assignment. Name the gaps; never write the fix yourself (Hard Rule 2 — the hint ladder still applies).\n\
2. If there's an unanswered question from you still pending: nudge it in ONE short sentence — never repeat the full question text.\n\
3. First-sight full-content files may be tool-generated scaffold (e.g. a `cargo new` template) — acknowledge scaffold in one sentence, don't review it line by line; focus on the user's hand-written change.\n\
4. If the user asks a question in the middle of this, answer it, then recall the task.\n\
5. The artifact's PURPOSE decides what you may say about it. Asked to READ, RUN, or DESCRIBE it: it is OFF-LIMITS until the user reports — do not quote, summarize, or explain it; only acknowledge that the step happened and say nothing about the artifact's content; when their report arrives, verify it against what you saw. Asked to WRITE or CHANGE it: the opposite — seeing and judging it is the point; audit it under rule 1. Never read this rule as a reason to stay silent about work you assigned them to produce."
    );
```

- [ ] **Step 4:** Doküman yorumunu güncelle: `flow_frame`'in üzerindeki doc comment'ta beş kuralı sayan cümleyi yeni kural 1/5'i yansıtacak şekilde düzelt (İngilizce; "(1) audit a WRITE-assignment delivery in three parts … (5) READ-artifacts are eyes-only until the report, WRITE-artifacts are audited"). `flow_frame_carries_exercise_rule_when_flagged` DEĞİŞMEZ.
- [ ] **Step 5:** `cargo test flow_frame` → PASS (özellikle `flow_frame_carries_exercise_rule_when_flagged` hâlâ geçmeli). `cargo test` (tümü) → yeşil — DİKKAT: `ride_along_turn_selects_flow_frame_and_keeps_user_words_last` "part of the ongoing lesson" iğnesini kullanıyor, korunmuş olmalı.
- [ ] **Step 6:** `cargo fmt -- src/file_feedback.rs` · `cargo clippy --all-targets` → 0 uyarı.
- [ ] **Step 7:** Commit:

```bash
git add src/file_feedback.rs
git commit -m "fix: flow_frame audits WRITE-deliveries in three parts and states the READ/WRITE asymmetry (finding E)"
```

---

### Task 3: Gözcü yapı olaylarını iletir — tür-farkındalıklı filtre + StructureTracker + FileMemory::knows (spec D1 mekaniği)

**Files:**
- Modify: `src/watcher.rs` (olay-türü genişletme, `should_forward(path, kind)`, `StructureTracker`, testler)
- Modify: `src/feedback.rs` (`FileMemory::knows` + 1 test)

**Interfaces:**
- Consumes: notify v6 `EventKind::{Modify, Create, Remove}`; `watcher::is_ignored` (değişmiyor).
- Produces: `watcher::should_forward(path: &Path, kind: &EventKind) -> bool` (İMZA DEĞİŞTİ — tek çağrıcısı `spawn`'ın thread'i, dışarıda kullanan yok); `watcher::StructureTracker` (`seed(root) -> Self` · `note_new_dir(&Path) -> bool` · `note_removed(&Path) -> bool`) — Görev 4'ün `classify_flush`'ı tüketir, bu task'ta `#[allow(dead_code)]` ile aşamalı; `feedback::FileMemory::knows(&Path) -> bool` — aynı şekilde Görev 4 tüketir (pub API'ye eklenen okuyucu, dead_code uyarısı üretmez çünkü `pub`).
- Plain yolu NOT: kanal artık dizin-Create/Remove ve silinmiş yolları da taşır; plain'in `handle_file_change`'i bunları NotFound/IsADirectory silent-skip'iyle yutar — `src/plain.rs` dokunulmaz, davranışı değişmez.

- [ ] **Step 1: Failing testler — `src/watcher.rs`.** Test modülünde `should_forward_filters_out_real_directory` ve `should_forward_allows_real_file` testlerini SİL, yerine ekle (test modülü başına `use notify::event::{CreateKind, ModifyKind, RemoveKind};` importu gerekir):

```rust
    #[test]
    fn should_forward_live_directory_only_on_structure_kinds() {
        // A directory APPEARING or DISAPPEARING is a structure signal
        // (spec D1: "boş dizin dosya olayı üretmiyor" was the invisible
        // half of the brands/marka-a assignment); a Modify on a live
        // directory is contents noise — the file inside gets its own event.
        let dir = std::env::temp_dir().join(format!(
            "usta_watcher_forward_kinds_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(should_forward(&dir, &EventKind::Create(CreateKind::Folder)));
        assert!(should_forward(&dir, &EventKind::Remove(RemoveKind::Folder)));
        assert!(!should_forward(&dir, &EventKind::Modify(ModifyKind::Any)));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn should_forward_files_and_vanished_paths_on_every_kind() {
        let file = std::env::temp_dir().join(format!(
            "usta_watcher_forward_file_{}.rs",
            std::process::id()
        ));
        std::fs::write(&file, b"fn main() {}").unwrap();
        assert!(should_forward(&file, &EventKind::Modify(ModifyKind::Any)));
        assert!(should_forward(&file, &EventKind::Create(CreateKind::File)));
        std::fs::remove_file(&file).unwrap();
        // A vanished path (deletion / rename source): is_dir() is false —
        // forward; flush-time classification decides what it means.
        assert!(should_forward(&file, &EventKind::Remove(RemoveKind::File)));
        assert!(should_forward(&file, &EventKind::Modify(ModifyKind::Any)));
        // Ignored stays ignored on every kind.
        assert!(!should_forward(
            Path::new("target/debug/x.rs"),
            &EventKind::Modify(ModifyKind::Any)
        ));
        assert!(!should_forward(
            Path::new(".git/HEAD"),
            &EventKind::Remove(RemoveKind::File)
        ));
    }

    #[test]
    fn structure_tracker_seeds_existing_dirs_and_flags_changes() {
        let base = std::env::temp_dir().join(format!(
            "usta_watcher_tracker_{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("src")).unwrap();
        let mut t = StructureTracker::seed(&base);
        // Pre-existing dir was seeded: sighting it is NOT "new".
        assert!(!t.note_new_dir(&base.join("src")));
        // A brand-new dir: first sighting is new, second is not.
        let fresh = base.join("brands");
        assert!(t.note_new_dir(&fresh));
        assert!(!t.note_new_dir(&fresh));
        // Removal: known dir → true exactly once; unknown → false.
        assert!(t.note_removed(&fresh));
        assert!(!t.note_removed(&fresh));
        assert!(!t.note_removed(&base.join("never-seen")));
        let _ = std::fs::remove_dir_all(&base);
    }
```

- [ ] **Step 2: Failing test — `src/feedback.rs`.** Test modülüne ekle:

```rust
    #[test]
    fn knows_reports_seeded_and_observed_paths() {
        // The flush-time structure classification (spec D1) asks this to
        // tell "a file the mentor knew was deleted" (structure note) from
        // "a transient temp path vanished" (silence, as today).
        let mut m = FileMemory::new();
        assert!(!m.knows(Path::new("a.rs")));
        m.seed(Path::new("a.rs"), "x".into());
        assert!(m.knows(Path::new("a.rs")));
        let _ = m.observe(Path::new("b.rs"), "y".into());
        assert!(m.knows(Path::new("b.rs")));
        assert!(!m.knows(Path::new("c.rs")));
    }
```

- [ ] **Step 3:** `cargo test should_forward_live_directory structure_tracker_seeds knows_reports` → Beklenen: **derlenmez** (`should_forward` tek parametreli, `StructureTracker`/`knows` yok). Doğru sebeple fail.
- [ ] **Step 4: Implementasyon — `src/watcher.rs`.**
  1. `spawn`'ın thread gövdesindeki olay filtresini değiştir:

```rust
            if matches!(
                event.kind,
                EventKind::Modify(_) | EventKind::Create(_) | EventKind::Remove(_)
            ) {
                for path in event.paths {
                    if !should_forward(&path, &event.kind) {
                        continue;
                    }
                    // End the thread if the REPL receiver has closed.
                    if out_tx.send(path).is_err() {
                        return;
                    }
                }
            }
```

  2. `should_forward`'ı şu tanımla değiştir (eski doc comment'ın yerine):

```rust
/// Decide whether a changed path is worth forwarding. Ignored paths never
/// are. A LIVE directory only matters when it appears or disappears — a
/// structure signal (spec D1) — so it forwards only on Create/Remove;
/// Modify on a directory is contents noise (the file inside gets its own
/// event). Files, and paths that no longer exist (deletions, rename
/// sources), forward on every kind: classification happens at flush time
/// (polite::classify_flush), where existence is probed exactly once.
pub fn should_forward(path: &Path, kind: &EventKind) -> bool {
    if is_ignored(path) {
        return false;
    }
    if path.is_dir() {
        return matches!(kind, EventKind::Create(_) | EventKind::Remove(_));
    }
    true
}
```

  3. `Debouncer`'ın ÜSTÜNE ekle:

```rust
/// Session-scoped directory inventory (spec D1): seeded from the project
/// tree at session start so an event on a PRE-EXISTING directory is never
/// misreported as "new". Deterministic shell state; classification asks it
/// two questions and never reads contents (directory contents are never
/// sent — the v0.24-era decision stands, only the EVENT is no longer
/// dropped).
#[allow(dead_code)] // staged: consumed by the structure ride-along task
pub struct StructureTracker {
    dirs: std::collections::BTreeSet<PathBuf>,
}

#[allow(dead_code)] // staged: consumed by the structure ride-along task
impl StructureTracker {
    /// Walk `root` and record every non-ignored directory that exists now.
    pub fn seed(root: &Path) -> Self {
        let mut tracker = StructureTracker {
            dirs: std::collections::BTreeSet::new(),
        };
        tracker.walk(root);
        tracker
    }

    fn walk(&mut self, dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && !is_ignored(&path) {
                self.dirs.insert(path.clone());
                self.walk(&path);
            }
        }
    }

    /// Record a directory sighting; true when it was previously unknown —
    /// the "new directory" signal.
    pub fn note_new_dir(&mut self, path: &Path) -> bool {
        self.dirs.insert(path.to_path_buf())
    }

    /// Record a disappearance; true when the path was a known directory —
    /// the "directory removed" signal.
    pub fn note_removed(&mut self, path: &Path) -> bool {
        self.dirs.remove(path)
    }
}
```

- [ ] **Step 5: Implementasyon — `src/feedback.rs`.** `seed`'in altına ekle:

```rust
    /// Whether this path has ever been seeded or observed. The flush-time
    /// structure classification uses it to tell "a known file was deleted"
    /// (worth a structure note, spec D1) from "a transient temp path
    /// vanished" (silence, as today).
    pub fn knows(&self, path: &Path) -> bool {
        self.seen.contains_key(path)
    }
```

- [ ] **Step 6:** `cargo test` (tümü) → yeşil (özellikle: mevcut `is_ignored_*` ve `debouncer_*` testleri değişmeden geçmeli). NOT: `StructureTracker` public olduğu için dead_code uyarısı beklenmez ama binary crate'te `pub` da ölü sayılabilir — Step 4'teki `#[allow(dead_code)]` bu yüzden şimdiden konuyor; `cargo clippy --all-targets` → **0 uyarı** bunun kanıtı.
- [ ] **Step 7:** `cargo fmt -- src/watcher.rs src/feedback.rs` · `cargo clippy --all-targets` → 0 uyarı.
- [ ] **Step 8:** Commit:

```bash
git add src/watcher.rs src/feedback.rs
git commit -m "feat: forward create/remove watcher events and stage StructureTracker + FileMemory::knows (finding D groundwork)"
```

---

### Task 4: Yapı notları biriktirilir ve ride-along ile teslim edilir (spec D2 — BULGU D)

**Files:**
- Modify: `src/tui/polite.rs` (`classify_flush`, notlu `PendingChanges`, `dispatch_flush`, `drop_pending_on_watch_off`, pin/test güncellemeleri)
- Modify: `src/file_feedback.rs` (`deliver_pending` notes parametresi + STRUCTURE bloğu; testler)
- Modify: `src/tui/run.rs` (tracker init — TEK satır; dispatch çağrısına `&mut tracker`)
- Modify: `src/watcher.rs`, `src/feedback.rs` (yalnız Görev 3'ün `#[allow(dead_code)]`'larının sökülmesi — `knows` zaten allow'suz)

**Interfaces:**
- Consumes: Görev 3'ün `StructureTracker` ve `FileMemory::knows`'u; mevcut `build_batch_payload`, `ride_along_turn`, `PENDING_PREAMBLE`.
- Produces: `polite::classify_flush(batch: Vec<PathBuf>, tracker: &mut watcher::StructureTracker, files: &FileMemory, project_root: &Path) -> (Vec<PathBuf>, Vec<String>)`; `PendingChanges::{hold_notes, len, is_empty, take}` — `take` artık `(Vec<PathBuf>, Vec<String>)` döner (İMZA DEĞİŞTİ); `polite::MAX_STRUCTURE_NOTES = 20`; `file_feedback::deliver_pending(files, project_root, paths, notes: &[String], user_text)` (İMZA DEĞİŞTİ); `dispatch_flush` `tracker` parametresi alır. Görev 6 `deliver_pending`'e monitör parametresi ekleyecek — bu task check davranışına DOKUNMAZ (inline check aynen).

- [ ] **Step 1: Failing testler — `src/tui/polite.rs`.** Test modülüne ekle; ayrıca `pending_changes_dedup_preserve_order_and_reset_on_take` içindeki `p.take()` beklentisini tuple'a çevir (`assert_eq!(p.take().0, vec![...])` ve sonuna `assert!(p.take().0.is_empty());`):

```rust
    #[test]
    fn pending_notes_dedup_cap_and_overflow_line() {
        let mut p = PendingChanges::new();
        p.hold_notes(vec![
            "+ a/ (new directory)".to_string(),
            "+ a/ (new directory)".to_string(),
        ]);
        assert_eq!(p.len(), 1, "exact repeats collapse");
        // A branch switch can delete hundreds of files — past the cap the
        // rest is counted and collapses into ONE overflow line at take().
        let many: Vec<String> = (0..30).map(|i| format!("- f{i}.rs (deleted)")).collect();
        p.hold_notes(many);
        assert_eq!(p.len(), 31, "the counter stays honest about suppressed notes");
        let (paths, notes) = p.take();
        assert!(paths.is_empty());
        assert_eq!(notes.len(), MAX_STRUCTURE_NOTES + 1);
        assert!(notes.last().unwrap().contains("11 more structural changes"));
        assert!(p.is_empty(), "take() drains notes and the overflow count too");
    }

    #[test]
    fn classify_flush_five_way_table() {
        let base = scratch("classify");
        std::fs::create_dir_all(base.join("known")).unwrap();
        let file = base.join("main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let mut tracker = crate::watcher::StructureTracker::seed(&base);
        let mut files = FileMemory::new();
        let deleted_known = base.join("old.rs");
        files.seed(&deleted_known, "gone\n".to_string());

        let new_dir = base.join("brands");
        std::fs::create_dir_all(&new_dir).unwrap();
        let vanished_unknown = base.join("_transient_tmp.rs");

        let (content, notes) = classify_flush(
            vec![
                file.clone(),
                new_dir.clone(),
                base.join("known"),
                deleted_known.clone(),
                vanished_unknown,
            ],
            &mut tracker,
            &files,
            &base,
        );
        // Existing file → content path (unchanged pipeline).
        assert_eq!(content, vec![file]);
        // New dir noted, pre-existing dir silent, deleted KNOWN file noted,
        // vanished unknown path silent (transient noise, as today).
        assert_eq!(
            notes,
            vec![
                "+ brands/ (new directory)".to_string(),
                "- old.rs (deleted)".to_string()
            ]
        );
        // Second sighting of the same dir is silent — the tracker learned it.
        let (_, notes2) = classify_flush(vec![new_dir], &mut tracker, &files, &base);
        assert!(notes2.is_empty());
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn classify_flush_reports_removed_known_directory() {
        let base = scratch("classify-rmdir");
        let dir = base.join("assets");
        std::fs::create_dir_all(&dir).unwrap();
        let mut tracker = crate::watcher::StructureTracker::seed(&base);
        std::fs::remove_dir_all(&dir).unwrap();
        let files = FileMemory::new();
        let (content, notes) = classify_flush(vec![dir], &mut tracker, &files, &base);
        assert!(content.is_empty());
        assert_eq!(notes, vec!["- assets/ (directory removed)".to_string()]);
        std::fs::remove_dir_all(&base).ok();
    }
```

- [ ] **Step 2: Failing testler — `src/file_feedback.rs`.** Test modülüne ekle (mevcut `deliver_pending_*` testlerinin çağrılarına Step 5'te `&[]` argümanı eklenecek — önce yenileri yaz):

```rust
    #[tokio::test]
    async fn deliver_pending_ships_structure_only_batches_in_the_lesson_frame() {
        // "Did you create those folders?" must never be asked again
        // (finding D): a structure-only delivery is a real delivery — the
        // note line IS the evidence the audit rule (flow_frame rule 1)
        // verifies. Content is never sent for directories; only the line.
        let dir = scratch_dir("deliver-structure-only");
        let notes = vec!["+ brands/marka-a/ (new directory)".to_string()];
        let mut files = feedback::FileMemory::new();
        let (notices, outgoing) =
            deliver_pending(&mut files, &dir, &[], &notes, "done".to_string()).await;
        assert!(notices.is_empty());
        assert!(outgoing.starts_with(PENDING_PREAMBLE));
        assert!(outgoing.contains("STRUCTURE: project tree changes"));
        assert!(outgoing.contains("+ brands/marka-a/ (new directory)"));
        assert!(outgoing.contains("part of the ongoing lesson"));
        assert!(outgoing.trim_end().ends_with("done"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn deliver_pending_structure_block_precedes_file_blocks() {
        let dir = scratch_dir("deliver-structure-order");
        let file = dir.join("main.rs");
        std::fs::write(&file, "fn main() {}\n").unwrap();
        let notes = vec!["- src/old.rs (deleted)".to_string()];
        let mut files = feedback::FileMemory::new();
        let (_, outgoing) = deliver_pending(
            &mut files,
            &dir,
            &[file],
            &notes,
            "take a look".to_string(),
        )
        .await;
        let pos_structure = outgoing.find("STRUCTURE: project tree changes").unwrap();
        let pos_file = outgoing.find("FILE:").unwrap();
        let pos_user = outgoing.rfind("take a look").unwrap();
        assert!(pos_structure < pos_file, "structure line leads the payload");
        assert!(pos_file < pos_user, "the user's words stay last (spec K2)");
        std::fs::remove_dir_all(&dir).ok();
    }
```

- [ ] **Step 3:** `cargo test pending_notes_dedup classify_flush deliver_pending_ships_structure` → Beklenen: **derlenmez** (`hold_notes`/`classify_flush` yok; `deliver_pending` 4 parametreli). Doğru sebeple fail.
- [ ] **Step 4: Implementasyon — `src/tui/polite.rs`.**
  1. `PendingChanges`'i şu tanımla değiştir (mevcut `hold`/`new` aynen kalır; `len`/`is_empty`/`take` güncellenir, `hold_notes` ve sabit eklenir):

```rust
/// Cap on held structure notes — a branch switch can delete hundreds of
/// files; past this the rest collapses into one overflow line at take().
pub(crate) const MAX_STRUCTURE_NOTES: usize = 20;

/// Accumulated-but-undelivered watcher batches (spec K2) plus structure
/// notes (spec D2): only PATHS and one-line notes are held — file payloads
/// are built at delivery time via `file_feedback::deliver_pending`, so
/// intermediate saves collapse into one diff, and directory CONTENTS are
/// never sent at all. Order preserved, repeats collapsed. `len` feeds the
/// status line's deterministic counter (spec K3) and counts paths, notes
/// and suppressed-overflow alike; `take` drains everything, which is also
/// the counter reset.
#[derive(Default)]
pub(crate) struct PendingChanges {
    paths: Vec<PathBuf>,
    notes: Vec<String>,
    suppressed: usize,
}
```

  ve metodlar:

```rust
    /// Accumulate structure notes — exact repeats collapse; overflow past
    /// the cap is counted and rendered as one honest line at take().
    pub(crate) fn hold_notes(&mut self, notes: Vec<String>) {
        for n in notes {
            if self.notes.contains(&n) {
                continue;
            }
            if self.notes.len() >= MAX_STRUCTURE_NOTES {
                self.suppressed += 1;
            } else {
                self.notes.push(n);
            }
        }
    }

    pub(crate) fn len(&self) -> usize {
        self.paths.len() + self.notes.len() + self.suppressed
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.paths.is_empty() && self.notes.is_empty() && self.suppressed == 0
    }

    /// Drain for delivery — resets the counter (spec K3). The overflow
    /// count collapses into its one line here.
    pub(crate) fn take(&mut self) -> (Vec<PathBuf>, Vec<String>) {
        let mut notes = std::mem::take(&mut self.notes);
        if self.suppressed > 0 {
            notes.push(format!(
                "… and {} more structural changes",
                self.suppressed
            ));
            self.suppressed = 0;
        }
        (std::mem::take(&mut self.paths), notes)
    }
```

  2. `classify_flush`'ı `route`'un altına ekle:

```rust
/// Flush-time split of a debounced batch into CONTENT paths (existing
/// files — the payload pipeline as before) and STRUCTURE notes (spec D1):
/// new directories, deleted known files, removed known directories. Paths
/// that vanished without ever being known stay silent — the transient-temp
/// class is_silent_skip swallows today. Deterministic; existence is probed
/// HERE, once, so classification can't rot between event and delivery. The
/// tracker updates even while watching is off, so re-enabling never
/// misreports old directories as new.
pub(crate) fn classify_flush(
    batch: Vec<PathBuf>,
    tracker: &mut crate::watcher::StructureTracker,
    files: &FileMemory,
    project_root: &Path,
) -> (Vec<PathBuf>, Vec<String>) {
    let mut content = Vec::new();
    let mut notes = Vec::new();
    for path in batch {
        let rel = path
            .strip_prefix(project_root)
            .unwrap_or(&path)
            .display()
            .to_string();
        if path.is_dir() {
            if tracker.note_new_dir(&path) {
                notes.push(format!("+ {rel}/ (new directory)"));
            }
        } else if path.is_file() {
            content.push(path);
        } else if tracker.note_removed(&path) {
            notes.push(format!("- {rel}/ (directory removed)"));
        } else if files.knows(&path) {
            notes.push(format!("- {rel} (deleted)"));
        }
        // else: a vanished path nobody ever knew — transient noise, silent.
    }
    (content, notes)
}
```

  3. `dispatch_flush`'ın imzasına `tracker: &mut crate::watcher::StructureTracker` parametresi ekle (son parametre olarak) ve gövdesini şu yapıya getir — DİKKAT: pin testleri güncellenecek (Step 6):

```rust
    // Flush-time classification (spec D1): existence is probed NOW; the
    // tracker updates even with watching off, so re-enabling stays honest.
    let (content, notes) = classify_flush(batch, tracker, files, project_root);
    if watching && !notes.is_empty() {
        // Structure notes never open a turn in ANY mode — they ride the
        // user's next submit (spec D2), so K1 holds for mkdir too.
        pending.hold_notes(notes);
    }
    let picked = route(content.len(), max_batch, watching, live);
    match picked {
        Route::Bulk => bulk_skip(tui, files, content)?,
        Route::ObserveOnly => sync_baseline(files, content),
        // Live mode — the user's explicit timing choice: immediate turn.
        Route::Feedback => {
            process_batch(
                tui,
                editor,
                events,
                backend,
                session,
                files,
                recorder,
                project_root,
                topic,
                last_tokens,
                &content,
            )
            .await?
        }
        // Companion default: accumulate; delivery rides the next submit (K2).
        Route::Hold => pending.hold(content),
    }
    Ok(())
```

  4. `drain_and_deliver`'ın gövdesindeki drain'i tuple'a çevir:

```rust
    let (paths, notes) = pending.take();
    crate::file_feedback::deliver_pending(files, project_root, &paths, &notes, user_text).await
```

  5. `drop_pending_on_watch_off`'un gövdesini `take().len()` yerine `len()` üzerinden kur (notlar da sayılır):

```rust
    let dropped = pending.len();
    let _ = pending.take();
    Some(format!(
        "{dropped} noted change(s) dropped — they will not be sent"
    ))
```

- [ ] **Step 5: Implementasyon — `src/file_feedback.rs`.** `deliver_pending`'in imzasına `notes: &[String]` ekle (paths'ten sonra) ve gövdesini şuna getir (inline check bu task'ta AYNEN kalır — spec C2 "check koşum noktaları değişmez"):

```rust
pub(crate) async fn deliver_pending(
    files: &mut feedback::FileMemory,
    project_root: &Path,
    paths: &[PathBuf],
    notes: &[String],
    user_text: String,
) -> (Vec<String>, String) {
    let (mut payload, meta) = build_batch_payload(files, project_root, paths);
    if !notes.is_empty() {
        // Structure lines lead the payload (spec D2): paths only, never
        // contents — the directory-contents decision stands; what was wrong
        // was dropping the EVENT entirely.
        let block = format!("STRUCTURE: project tree changes\n{}", notes.join("\n"));
        payload = if payload.is_empty() {
            block
        } else {
            format!("{block}\n\n{payload}")
        };
    }
    if meta.total_included == 0 && notes.is_empty() {
        return (meta.notices, user_text);
    }
    let check = if meta.any_non_exercise {
        check::run_check(project_root)
            .await
            .map(|r| check_result_block(&r))
    } else {
        None
    };
    let turn = ride_along_turn(&payload, meta.any_exercise, check.as_deref(), &user_text);
    (meta.notices, turn)
}
```

  Doc comment'ı güncelle (notes parametresi + structure-only teslimin check koşturmadığı — `any_non_exercise` yalnız İÇERİKTEN türediği için kapı kendiliğinden doğru). Mevcut `deliver_pending_*` testlerinin TÜM çağrılarına `&[]` notes argümanını ekle (paths'ten sonra).
- [ ] **Step 6: Pin ve mevcut test güncellemeleri — `src/tui/polite.rs`.**
  1. `dispatch_flush_route_arms_are_pinned` iğne listesinde `"match route(batch.len()"` iğnesini şu İKİ iğneyle değiştir (gerekçe yorumuna bir satır ekle: "v0.29.0: the batch is classified first — the route sees CONTENT only, structure notes are held before routing"):

```rust
            "let picked = route(content.len()",
            "pending.hold_notes(notes)",
```

  2. Isırma doğrulaması: `dispatch_flush` gövdesindeki `pending.hold_notes(notes);` satırını geçici yorum yap → `cargo test dispatch_flush_route_arms_are_pinned` FAIL → geri al.
  3. `watch_off_drops_the_queued_changes_and_says_so` testine notlu senaryo ekle: `pending.hold_notes(vec!["+ x/ (new directory)".into()]);` satırını iki `hold`'dan sonra ekle ve `notice.contains('3')` bekle (2 yol + 1 not).
- [ ] **Step 7: Implementasyon — `src/tui/run.rs`** (iki nokta):
  1. `crate::file_feedback::seed_mentor_baseline(&mut files, project_root);` satırının hemen ALTINA ekle:

```rust
    let mut tracker = watcher::StructureTracker::seed(project_root);
```

  2. Deadline kolundaki `dispatch_flush` çağrısının argüman listesinin SONUNA `, &mut tracker` ekle (aynı satır — satır sayısı değişmez).
- [ ] **Step 8: Aşamalı allow'ları sök.** `src/watcher.rs`'te `StructureTracker` üzerindeki iki `#[allow(dead_code)]` satırını SİL (artık tüketiliyor).
- [ ] **Step 9:** Doğrula: `grep -c "" src/tui/run.rs` → Beklenen: **597** (≤ 600). `cargo test` → TÜMÜ yeşil (özellikle: `pending_changes_dedup_preserve_order_and_reset_on_take` (tuple), `drain_and_deliver_*` üçlüsü, `deliver_pending_*` hepsi, `run_rs_wiring_call_sites_are_pinned` — mevcut yedi iğnesi hâlâ yerinde, `ride_along_attaches_only_to_genuine_user_text` — attach imzası değişmedi).
- [ ] **Step 10:** `cargo fmt -- src/tui/polite.rs src/file_feedback.rs src/tui/run.rs src/watcher.rs` · `cargo clippy --all-targets` → 0 uyarı · `grep -c "" src/tui/run.rs` tekrar ≤ 600.
- [ ] **Step 11:** Commit:

```bash
git add src/tui/polite.rs src/file_feedback.rs src/tui/run.rs src/watcher.rs src/feedback.rs
git commit -m "feat: structural changes ride along — new dirs and deletions accumulate as STRUCTURE lines (finding D)"
```

---

### Task 5: Doğrulama hafızası çekirdeği — `Verdict` + `VerifyMonitor` (spec C1/C2, aşamalı)

**Files:**
- Modify: `src/check.rs` (`Verdict`, `verdict_of`, `error_summary`, `VerifyMonitor` + testler)

**Interfaces:**
- Consumes: `check::is_cargo_project` (mevcut — dikişin kendisi: doğrulayıcı kapısı zaten burada), `check::truncate_output` (mevcut).
- Produces: `check::Verdict::{Pass, Fail{summary}}` · `check::verdict_of(&str) -> Verdict` · `check::error_summary(&str) -> String` · `check::VerifyMonitor` (`new(&Path)` · `record(&str)` · `is_failing() -> bool` · `note() -> Option<String>`). Hepsi bu task'ta `#[allow(dead_code)]` ile aşamalı; Görev 6 tüketir ve söker.
- İsimler bilinçli NÖTR (spec C1): kavram "projenin doğrulama sinyali, proje birine sahipse"; cargo bugünkü TEK implementasyondur ve öyle kalır — kayıt/registry/dil-algılama YASAK (YAGNI). Kullanıcıya/modele dönük metin somut "cargo check" der, çünkü yalnız Cargo projesinde görünür.

- [ ] **Step 1: Failing testler.** `src/check.rs` test modülüne ekle:

```rust
    #[test]
    fn verdict_of_classifies_clean_and_failing_output() {
        // run_check's own clean sentence is the Pass pin.
        assert_eq!(
            verdict_of("CLEAN — cargo check passed with no errors."),
            Verdict::Pass
        );
        let raw = "src/main.rs:3:18: error[E0308]: mismatched types: expected `i32`, found `()`\nwarning: unused";
        match verdict_of(raw) {
            Verdict::Fail { summary } => {
                assert!(summary.contains("error[E0308]"));
                assert!(summary.contains("mismatched types"));
            }
            Verdict::Pass => panic!("failing output must classify as Fail"),
        }
    }

    #[test]
    fn error_summary_picks_first_error_line_and_caps_it() {
        let raw = "warning: something\nsrc/main.rs:3:18: error[E0308]: mismatched types\nsrc/x.rs:9:1: error[E0425]: not found";
        let s = error_summary(raw);
        assert!(s.contains("error[E0308]"));
        assert!(!s.contains("E0425"), "one line, the FIRST error");
        // No error-looking line at all → first non-empty line, still capped.
        let long = format!("prelude {}", "x".repeat(400));
        let capped = error_summary(&long);
        assert!(capped.len() < 250);
        assert!(capped.contains("truncated"));
        assert_eq!(error_summary(""), "(no output)");
    }

    /// Scratch dirs for monitor tests — with/without a Cargo manifest.
    fn monitor_scratch(tag: &str, cargo: bool) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("usta_verify_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        if cargo {
            std::fs::write(dir.join("Cargo.toml"), "[package]").unwrap();
        }
        dir
    }

    #[test]
    fn verify_monitor_remembers_the_last_verdict() {
        // Finding C's fix in one test: the check already produces the truth;
        // the bug was forgetting it the moment a delivery ended. Level-
        // triggered memory, zero new executions.
        let dir = monitor_scratch("remember", true);
        let mut m = VerifyMonitor::new(&dir);
        assert!(!m.is_failing());
        assert!(m.note().is_none(), "no verdict yet — silence");
        m.record("src/main.rs:3:18: error[E0308]: mismatched types");
        assert!(m.is_failing());
        let note = m.note().expect("red state must produce the note");
        assert!(note.starts_with("[build state:"));
        assert!(note.contains("error[E0308]"));
        assert!(note.contains("do not treat the current step as complete"));
        // Superseded only by the next REAL check (spec C2).
        m.record("CLEAN — cargo check passed with no errors.");
        assert!(!m.is_failing());
        assert!(m.note().is_none(), "green state never nags");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn verify_monitor_is_a_silent_no_op_without_a_verifier() {
        // Spec C1 lock: Usta is domain-agnostic — for any non-Cargo project
        // (and every non-software domain) the WHOLE feature must be a
        // silent no-op: no marker, no note, zero behavior change.
        let dir = monitor_scratch("noop", false);
        let mut m = VerifyMonitor::new(&dir);
        m.record("src/main.rs:1:1: error[E0308]: mismatched types");
        assert!(!m.is_failing());
        assert!(m.note().is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
```

- [ ] **Step 2:** `cargo test verdict_of_classifies verify_monitor` → Beklenen: **derlenmez** (`Verdict`/`VerifyMonitor` yok). Doğru sebeple fail.
- [ ] **Step 3: Minimal implementasyon.** `src/check.rs`'te `run_check`'in ALTINA ekle:

```rust
/// One verification verdict, parsed from raw checker output. The names are
/// deliberately NEUTRAL (spec C1): the concept is "the project's own
/// verification signal, when it has one" — the Cargo check is merely
/// today's only implementation, and adding another later is a new arm, not
/// a redesign. No verifier registry, no per-language detection (YAGNI).
#[allow(dead_code)] // staged: consumed by the verification-wiring task
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    Pass,
    Fail { summary: String },
}

/// Classify raw `run_check` output. The clean case is pinned to
/// run_check's own sentence; anything else fails, with a one-line summary.
#[allow(dead_code)] // staged: consumed by the verification-wiring task
pub fn verdict_of(raw: &str) -> Verdict {
    if raw.starts_with("CLEAN") {
        Verdict::Pass
    } else {
        Verdict::Fail {
            summary: error_summary(raw),
        }
    }
}

/// First error-carrying line of a failing check, capped for a one-line note.
#[allow(dead_code)] // staged: consumed by the verification-wiring task
pub fn error_summary(raw: &str) -> String {
    let line = raw
        .lines()
        .find(|l| l.contains("error"))
        .or_else(|| raw.lines().find(|l| !l.trim().is_empty()))
        .unwrap_or("(no output)")
        .trim();
    truncate_output(line, 200)
}

/// Shell memory of the project's verification signal (spec C2): the check
/// keeps running exactly where it runs today — at deliveries that carry a
/// non-exercise file, and on the live path — and this REMEMBERS the verdict
/// so the mentor can no longer forget a red project between deliveries
/// (finding C: turns 14–15 declared a step finished while the project did
/// not compile). Level-triggered where v0.28.0 was edge-triggered; zero new
/// check executions. For a project with no verifier the monitor is disabled
/// and every method is a silent no-op — zero behavior change (spec C1).
/// Never opens a turn (K1): everything it produces is a status-line pixel
/// or a line attached to a turn the user already opened.
#[allow(dead_code)] // staged: consumed by the verification-wiring task
pub struct VerifyMonitor {
    enabled: bool,
    verdict: Option<Verdict>,
}

#[allow(dead_code)] // staged: consumed by the verification-wiring task
impl VerifyMonitor {
    pub fn new(project_root: &Path) -> Self {
        VerifyMonitor {
            enabled: is_cargo_project(project_root),
            verdict: None,
        }
    }

    /// Remember the verdict of a check that actually ran. Disabled → no-op
    /// (belt: run_check already returns None for non-Cargo roots).
    pub fn record(&mut self, raw: &str) {
        if self.enabled {
            self.verdict = Some(verdict_of(raw));
        }
    }

    /// Last KNOWN state is failing — drives the dim status marker.
    pub fn is_failing(&self) -> bool {
        matches!(self.verdict, Some(Verdict::Fail { .. }))
    }

    /// One-line state note while the last known verdict is red; None
    /// otherwise. Carried on EVERY delivered turn until a later check comes
    /// back clean. The instruction rides INSIDE the note, so flow_frame
    /// needs no extra rule (prompt diet). Honest about staleness: "as of
    /// the last delivered change".
    pub fn note(&self) -> Option<String> {
        match &self.verdict {
            Some(Verdict::Fail { summary }) if self.enabled => Some(format!(
                "[build state: the last cargo check was still failing — first error: {summary}. \
The project did not compile as of the last delivered change; do not treat the \
current step as complete until a later check comes back clean.]"
            )),
            _ => None,
        }
    }
}
```

- [ ] **Step 4:** `cargo test verdict_of error_summary verify_monitor` → hepsi PASS. `cargo test` (tümü) → yeşil.
- [ ] **Step 5:** `cargo fmt -- src/check.rs` · `cargo clippy --all-targets` → **0 uyarı** (dead_code uyarısı kalırsa attribute eksik demektir — yukarıdaki allow'lar yerinde mi kontrol et).
- [ ] **Step 6:** Commit:

```bash
git add src/check.rs
git commit -m "feat: stage Verdict + VerifyMonitor — neutral verdict memory for the project's verification signal (finding C)"
```

---

### Task 6: Doğrulama hafızası kablolaması — kayıt, not, durum işareti, run.rs yerinden etme (spec C2 yürürlükte)

**Files:**
- Modify: `src/file_feedback.rs` (`deliver_pending` + `handle_batch_change` monitör parametresi; testler)
- Modify: `src/tui/polite.rs` (`drain_and_deliver`/`attach_pending` monitör geçişi; `process_batch`; `dispatch_flush`; **`handle_watch_command`** yerinden etme; pin güncellemeleri)
- Modify: `src/tui/run.rs` (monitör init; attach/dispatch/draw çağrıları; /watch bloğunun sökülmesi)
- Modify: `src/tui/status.rs` (`render_status` `verify_failing: bool` parametresi + işaret + testler)
- Modify: `src/tui/page.rs` (`draw` parametre geçişi)
- Modify: `src/tui/ask.rs`, `src/tui/intro.rs`, `src/tui/entry.rs` (draw çağrılarına `false`)
- Modify: `src/check.rs` (yalnız Görev 5 `#[allow(dead_code)]`'larının sökülmesi)

**Interfaces:**
- Consumes: Görev 5'in `VerifyMonitor`'u; Görev 4'ün notlu teslim zinciri.
- Produces: `file_feedback::deliver_pending(files, project_root, paths, notes, monitor: &mut check::VerifyMonitor, user_text)` (İMZA DEĞİŞTİ); `file_feedback::handle_batch_change(..., monitor: &mut check::VerifyMonitor)` (İMZA DEĞİŞTİ — plain'in kullandığı `handle_file_change` DEĞİŞMEZ); `polite::drain_and_deliver`/`attach_pending` monitör parametreli; `polite::handle_watch_command(cmd, &mut watching, &mut live, &mut pending) -> Vec<String>`; `page::draw(..., verify_failing: bool)`; `status::render_status(..., verify_failing: bool)`.

- [ ] **Step 1: Failing testler — `src/tui/status.rs`.** Test modülüne ekle (mevcut tüm `render_status` çağrılarına Step 5'te `, false` eklenecek):

```rust
    #[test]
    fn verify_failing_marker_shows_only_when_flagged() {
        // Finding C's visible half: a dim, deterministic marker while the
        // last known check verdict is red — presence costs zero tokens and
        // never a turn (the navigator's raised eyebrow).
        let on = text(&render_status(
            &Status::Idle,
            None,
            1_000_000,
            Some((true, false, 0)),
            true,
        ));
        assert!(on.contains("✗ check failing"));
        let off = text(&render_status(
            &Status::Idle,
            None,
            1_000_000,
            Some((true, false, 0)),
            false,
        ));
        assert!(!off.contains("check failing"));
    }
```

- [ ] **Step 2: Failing testler — `src/file_feedback.rs`.** Mevcut `deliver_pending_runs_the_check_for_non_exercise_files` ve `deliver_pending_skips_the_check_for_exercise_only_batches` testlerine monitör argümanı Step 4'te eklenecek; önce yenileri yaz:

```rust
    #[tokio::test]
    async fn deliver_pending_records_the_verdict_and_red_note_rides_later_turns() {
        // Finding C end to end at the delivery layer: a delivery whose
        // inline check fails RECORDS the verdict; the next delivery that
        // runs no check (exercise-only) still carries the one-line note, so
        // the mentor cannot declare a step finished on a red project.
        let dir = scratch_cargo_project("deliver-verdict-memory");
        // A deliberately broken main.rs — the same E0308 class the live
        // session exercised.
        std::fs::write(
            dir.join("src").join("main.rs"),
            "fn main() { let _x: i32 = (); }\n",
        )
        .unwrap();
        let mut monitor = check::VerifyMonitor::new(&dir);
        let mut files = feedback::FileMemory::new();
        let (_, first) = deliver_pending(
            &mut files,
            &dir,
            &[dir.join("src").join("main.rs")],
            &[],
            &mut monitor,
            "wrote it".to_string(),
        )
        .await;
        assert!(
            first.contains("[cargo check result — FOR YOUR EYES ONLY"),
            "the fresh eyes-only block still rides the checked delivery"
        );
        assert!(monitor.is_failing(), "the verdict must be remembered");
        // Exercise-only delivery: no check runs (v0.28.0 gate stands), but
        // the remembered red state rides as one line.
        let ex = dir.join("exercises").join("a.md");
        std::fs::create_dir_all(ex.parent().unwrap()).unwrap();
        std::fs::write(&ex, "answer\n").unwrap();
        let (_, second) = deliver_pending(
            &mut files,
            &dir,
            &[ex],
            &[],
            &mut monitor,
            "done".to_string(),
        )
        .await;
        assert!(!second.contains("FOR YOUR EYES ONLY"));
        assert!(second.contains("[build state: the last cargo check was still failing"));
        assert!(second.trim_end().ends_with("done"));
        std::fs::remove_dir_all(&dir).ok();
    }
```

  ve `src/tui/polite.rs` test modülüne:

```rust
    #[tokio::test]
    async fn drain_and_deliver_attaches_red_note_to_bare_messages() {
        // The core of finding C: turns WITHOUT any pending delivery are
        // exactly where the mentor used to forget the project was broken
        // (transcript turns 14–15). While the last verdict is red, every
        // outgoing turn carries the one-line note — remembering IS the fix.
        let dir = scratch("drain-red-note");
        std::fs::write(dir.join("Cargo.toml"), "[package]").unwrap();
        let mut monitor = crate::check::VerifyMonitor::new(&dir);
        monitor.record("src/main.rs:3:18: error[E0308]: mismatched types");
        let mut files = FileMemory::new();
        let mut pending = PendingChanges::new();
        let (notices, out) = drain_and_deliver(
            true,
            &mut pending,
            &mut files,
            &dir,
            &mut monitor,
            "so is this part finished?".to_string(),
        )
        .await;
        assert!(notices.is_empty());
        assert!(out.starts_with("[build state:"));
        assert!(out.trim_end().ends_with("so is this part finished?"));
        // Green (or no) verdict: the text passes through untouched.
        monitor.record("CLEAN — cargo check passed with no errors.");
        let (_, quiet) = drain_and_deliver(
            true,
            &mut pending,
            &mut files,
            &dir,
            &mut monitor,
            "and now?".to_string(),
        )
        .await;
        assert_eq!(quiet, "and now?");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn handle_watch_command_applies_toggles_and_drops_queue_on_off() {
        // Displaced from run.rs's submit arm (600-line budget): same
        // behavior, now directly testable — toggle applied, notices in
        // print order (drop notice BEFORE the toggle message, as before).
        use crate::slash::WatchCmd;
        let (mut watching, mut live) = (true, false);
        let mut pending = PendingChanges::new();
        pending.hold(vec![PathBuf::from("/tmp/a.rs")]);
        let notices = handle_watch_command(WatchCmd::Off, &mut watching, &mut live, &mut pending);
        assert!(!watching);
        assert_eq!(notices.len(), 2);
        assert!(notices[0].contains("dropped"), "drop notice prints first");
        assert!(notices[1].contains("off"));
        assert!(pending.is_empty());
        // Live toggle touches only the live axis.
        let notices = handle_watch_command(WatchCmd::LiveOn, &mut watching, &mut live, &mut pending);
        assert!(live);
        assert!(!watching, "live toggle must not touch watching");
        assert_eq!(notices.len(), 1);
    }
```

- [ ] **Step 3:** `cargo test verify_failing_marker deliver_pending_records drain_and_deliver_attaches_red handle_watch_command_applies` → Beklenen: **derlenmez** (imzalar eski). Doğru sebeple fail.
- [ ] **Step 4: Implementasyon — `src/file_feedback.rs`.**
  1. `deliver_pending` imzasına `monitor: &mut check::VerifyMonitor` ekle (`notes`'tan sonra) ve check bloğunu şuna çevir:

```rust
    // The check runs exactly where it ran before — a delivery carrying at
    // least one non-exercise file (spec C2: finding C adds MEMORY, not
    // executions). When it runs, the verdict is recorded; when it doesn't
    // (exercise-only, structure-only, run_check unavailable), a remembered
    // red verdict rides as the one-line note instead.
    let check_block = if meta.any_non_exercise {
        match check::run_check(project_root).await {
            Some(raw) => {
                monitor.record(&raw);
                Some(check_result_block(&raw))
            }
            None => monitor.note().map(|n| format!("\n\n{n}")),
        }
    } else {
        monitor.note().map(|n| format!("\n\n{n}"))
    };
    let turn = ride_along_turn(&payload, meta.any_exercise, check_block.as_deref(), &user_text);
    (meta.notices, turn)
```

  2. `handle_batch_change` imzasına `monitor: &mut check::VerifyMonitor` ekle (son parametre) ve check bloğunu şuna çevir (live yolu — inline check korunur, sonucu artık hatırlanır):

```rust
    if meta.any_non_exercise {
        if let Some(check_result) = check::run_check(project_root).await {
            monitor.record(&check_result);
            injected.push_str(&check_result_block(&check_result));
        }
    }
```

  3. Mevcut `deliver_pending_*` testlerinin hepsine `&mut check::VerifyMonitor::new(&dir)` argümanını ekle (ya da test başında `let mut monitor = check::VerifyMonitor::new(&dir);` — Cargo.toml'suz scratch dizinlerde monitör devre dışıdır, eski beklentiler değişmez; `scratch_cargo_project` kullananlarda check yine koşar, eski beklentiler değişmez). `batch_change_skips_llm_call_when_everything_drops` çağrısına da monitör ekle.
- [ ] **Step 5: Implementasyon — `src/tui/polite.rs`.**
  1. `attach_pending` ve `drain_and_deliver` imzalarına `monitor: &mut crate::check::VerifyMonitor` ekle (`project_root`'tan sonra); `attach_pending` gövdesindeki çağrıyı `drain_and_deliver(watching, pending, files, project_root, monitor, user_text).await` yap ve `drain_and_deliver` gövdesini şuna getir:

```rust
    if !watching {
        return (Vec::new(), user_text);
    }
    if pending.is_empty() {
        // No delivery — but a red verdict still rides as one line (spec
        // C2): the bare-message turn is exactly where the mentor used to
        // forget the project was broken. Green/unknown stays silent.
        let outgoing = match monitor.note() {
            Some(n) => format!("{n}\n\n{user_text}"),
            None => user_text,
        };
        return (Vec::new(), outgoing);
    }
    let (paths, notes) = pending.take();
    crate::file_feedback::deliver_pending(files, project_root, &paths, &notes, monitor, user_text)
        .await
```

  2. `process_batch` imzasına `monitor: &mut crate::check::VerifyMonitor` ekle ve `handle_batch_change` çağrısına geçir.
  2b. Mevcut üç `drain_and_deliver_*` testine (`…_empties_the_queue_after_one_delivery`, `…_passes_text_through_when_nothing_is_queued`, `…_is_a_no_op_while_watching_is_off`) monitör argümanı ekle: test başına `let mut monitor = crate::check::VerifyMonitor::new(&dir);` (scratch dizinlerde Cargo.toml yok → monitör devre dışı → eski beklentiler DEĞİŞMEZ) ve çağrıya `&mut monitor` geçir.
  3. `dispatch_flush` imzasına `monitor: &mut crate::check::VerifyMonitor` ekle (tracker'dan sonra) ve `Route::Feedback` kolundaki `process_batch` çağrısına geçir.
  4. `drop_pending_on_watch_off`'un ALTINA `handle_watch_command`'ı ekle:

```rust
/// `/watch` family handling, displaced from run.rs's submit arm (600-line
/// budget): applies the toggle and returns the notices to print, in order —
/// the queue-drop notice BEFORE the toggle message, exactly as the inline
/// block printed them. Watching off drops what is already queued (spec K2),
/// structure notes included.
pub(crate) fn handle_watch_command(
    cmd: crate::slash::WatchCmd,
    watching: &mut bool,
    live: &mut bool,
    pending: &mut PendingChanges,
) -> Vec<String> {
    use crate::slash::WatchCmd::*;
    let mut notices = Vec::new();
    match cmd {
        LiveOn | LiveOff | LiveToggle => {
            // Session-only timing choice (spec K4).
            let (next, m) = crate::slash::apply_live(cmd, *live);
            *live = next;
            notices.push(m.to_string());
        }
        On | Off | Toggle => {
            let (next, m) = crate::slash::apply_watch(cmd, *watching);
            *watching = next;
            if let Some(n) = drop_pending_on_watch_off(*watching, pending) {
                notices.push(n);
            }
            notices.push(m.to_string());
        }
    }
    notices
}
```

- [ ] **Step 6: Implementasyon — `src/tui/status.rs` + `src/tui/page.rs`.** `render_status` imzasına `verify_failing: bool` ekle (son parametre) ve watch bloğunun ALTINA ekle:

```rust
    // Dim verification marker (spec C2): deterministic presence, zero
    // tokens, never a turn — only ever true for a project that HAS a
    // verifier (spec C1 gates it at VerifyMonitor).
    if verify_failing {
        spans.push(Span::styled("✗ check failing ".to_string(), theme::info()));
    }
```

  `page::draw` imzasına `verify_failing: bool` ekle ve `render_status` çağrısına geçir. Mevcut `render_status` testlerinin tüm çağrılarına `, false` ekle.
- [ ] **Step 7: Implementasyon — `src/tui/run.rs`** (dört nokta; sonda satır sayısı kontrolü):
  1. Tracker init satırının ALTINA ekle:

```rust
    let mut monitor = crate::check::VerifyMonitor::new(project_root);
```

  2. Ana döngüdeki `draw` çağrısının `Some((watching, live, pending.len())),` satırının ALTINA yeni argüman satırı ekle: `watching && monitor.is_failing(),` (kapalı watch işareti gizler — sayaçla tutarlı).
  3. Submit kolundaki `/watch` bloğunu (23 satır: `if let Some(cmd) = crate::slash::parse_watch_command(&line) {`'dan kapanışına kadar) şu 6 satırla DEĞİŞTİR:

```rust
                        if let Some(cmd) = crate::slash::parse_watch_command(&line) {
                            crate::tui::page::page_user_echo(&mut tui, &line)?;
                            // Toggle handling + queue drop live in polite::handle_watch_command (600-line budget displacement).
                            for n in crate::tui::polite::handle_watch_command(cmd, &mut watching, &mut live, &mut pending) { crate::tui::page::page_notice(&mut tui, &n)?; }
                            continue;
                        }
```

  4. `attach_pending` çağrısını şuna çevir (tek satır): `crate::tui::polite::attach_pending(&mut tui, watching, &mut pending, &mut files, project_root, &mut monitor, line.clone()).await?` — ve deadline kolundaki `dispatch_flush` çağrısının sonuna `, &mut monitor` ekle (tracker'dan sonra).
- [ ] **Step 8: Diğer draw çağrıları.** `grep -rn "page::draw(" src/` → dört harici çağrı: `src/tui/ask.rs:41` (çok satırlı — `None,` argümanından sonra `false,`), `src/tui/ask.rs:109`, `src/tui/intro.rs:99`, `src/tui/entry.rs:284` (üçü tek satır — `None)` → `None, false)`). Hepsine `false` ekle (oturum-öncesi yüzeylerde monitör yok).
- [ ] **Step 9: Pin güncellemeleri — hepsi ısırma-doğrulamalı.**
  1. `src/tui/polite.rs` `run_rs_wiring_call_sites_are_pinned` iğne listesi: `"polite::drop_pending_on_watch_off("` iğnesini `"polite::handle_watch_command("` ile DEĞİŞTİR; listeye `"watching && monitor.is_failing()"` iğnesini EKLE (gerekçe yorumuna: "the marker feed is the same mutation class as the counter literal — a reviewer replacing it with `false` kills the marker with the suite green"). Isırma doğrulaması: run.rs'te draw'daki `watching && monitor.is_failing(),` satırını geçici `false,` yap → pin FAIL → geri al.
  2. `ride_along_attaches_only_to_genuine_user_text` içindeki tam-çağrı iğnesini yeni imzayla değiştir: `"attach_pending(&mut tui, watching, &mut pending, &mut files, project_root, &mut monitor, line.clone())"`. Isırma doğrulaması: iğnedeki `&mut monitor` kısmını testte yanlış yaz (`&mut monitorX`) → FAIL → düzelt.
  3. `run_wires_intro_flow` (`src/tui/intro.rs`) DEĞİŞMEZ — iğneleri /watch bloğuna değmiyor; yine de `cargo test run_wires_intro_flow` koş, yeşil gör.
- [ ] **Step 10: Aşamalı allow'ları sök.** `src/check.rs`'teki altı `#[allow(dead_code)] // staged:` satırını SİL.
- [ ] **Step 11:** Doğrula: `grep -c "" src/tui/run.rs` → Beklenen: **≈585** ve KESİNLİKLE ≤ 600. `cargo test` → TÜMÜ yeşil (özellikle: `verify_monitor_*`, `deliver_pending_records_the_verdict*`, `drain_and_deliver_attaches_red*`, `handle_watch_command_applies*`, `run_rs_wiring_call_sites_are_pinned`, `ride_along_attaches_only_to_genuine_user_text`, `watch_indicator_live_and_companion_states`; REGRESYON: `apply_watch_transitions`, `apply_live_transitions`, closing/transcript/lock testleri).
- [ ] **Step 12:** `cargo fmt -- src/file_feedback.rs src/tui/polite.rs src/tui/run.rs src/tui/status.rs src/tui/page.rs src/tui/ask.rs src/tui/intro.rs src/tui/entry.rs src/check.rs` · `cargo clippy --all-targets` → 0 uyarı (staged allow'lar söküldü, yenisi YOK) · `grep -c "" src/tui/run.rs` tekrar ≤ 600.
- [ ] **Step 13:** Commit:

```bash
git add src/file_feedback.rs src/tui/polite.rs src/tui/run.rs src/tui/status.rs src/tui/page.rs src/tui/ask.rs src/tui/intro.rs src/tui/entry.rs src/check.rs
git commit -m "feat: remember the verification verdict — red-state note on every delivered turn + dim status marker (finding C)"
```

---

### Task 7: `/context` — bağlam penceresi dökümü (spec F1)

**Files:**
- Create: `src/context_report.rs`
- Modify: `src/main.rs` (`mod context_report;` — `mod config;` satırının altına)
- Modify: `src/brain.rs` (`section_sizes` + `section_label` + 2 test)
- Modify: `src/file_feedback.rs` (`is_delivery_turn` + 1 test)
- Modify: `src/slash.rs` (`is_context_command` + testler + kapı pini)
- Modify: `src/help.rs` (1 satır + test iğnesi)
- Modify: `src/tui/run.rs` (`/context` dalı + konu-girişi kapısı)
- Modify: `src/tui/intro.rs` (tanışma kapısı)

**Interfaces:**
- Consumes: `session.system` (assembly'nin gerçek çıktısı — YENİDEN HESAPLAMA YOK, spec F1 tek-kaynaklılık), `session.history()`, `last_tokens`, `backend.context_window()` (run.rs'te `window` olarak zaten var).
- Produces: `context_report::build(system: &str, history: &[anthropic::Message], last_reported: Option<u64>, window: u64) -> String` (pub(crate)); `brain::section_sizes(system: &str) -> Vec<(String, usize)>` (pub); `file_feedback::is_delivery_turn(text: &str) -> bool` (pub(crate)); `slash::is_context_command(line: &str) -> bool` (pub(crate)).
- `src/plain.rs` DEĞİŞMEZ → `/context` plain'de normal metin olarak modele gider (spec kenar kararı, kabul edilen boşluk).

- [ ] **Step 1: Failing testler.**
  1. `src/brain.rs` test modülüne:

```rust
    #[test]
    fn section_sizes_roundtrips_the_assembled_prompt() {
        // Single-sourcing lock (spec F1): the report parses the REAL string
        // the model receives, using the assembly's own divider format — so
        // the breakdown can never drift from load_system_prompt.
        let (global, _project) = temp_pair("sizes");
        fs::write(global.join("SOUL.md"), "CORE").unwrap();
        fs::write(global.join("USER.md"), "PROFILE-BODY").unwrap();
        let sys = load_system_prompt(&global, None, "rust", "2026-08-29");
        let sizes = section_sizes(&sys);
        let labels: Vec<&str> = sizes.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(labels, vec!["TODAY", "SOUL.md", "USER.md"]);
        // Body bytes: each line plus its newline ("CORE" → 5).
        assert_eq!(sizes[1].1, "CORE".len() + 1);
        assert_eq!(sizes[2].1, "PROFILE-BODY".len() + 1);
        let _ = fs::remove_dir_all(global.parent().unwrap());
    }

    #[test]
    fn section_sizes_labels_the_fallback_prompt() {
        let sizes = section_sizes(FALLBACK_SYSTEM);
        assert_eq!(sizes.len(), 1);
        assert_eq!(sizes[0].0, "(fallback)");
        assert!(sizes[0].1 > 0);
    }
```

  2. `src/slash.rs` test modülüne:

```rust
    #[test]
    fn is_context_command_matches_only_bare_context() {
        assert!(is_context_command("/context"));
        assert!(is_context_command("  /CONTEXT  "));
        assert!(!is_context_command("/context now"));
        assert!(!is_context_command("context"));
    }

    #[test]
    fn context_command_is_gated_before_a_session_exists() {
        // At topic entry and during the introduction /context can't run (no
        // session yet) — both gates must point the user onward instead of
        // slugging "/context" into a topic name or sending it to the model.
        // Crude source pin, same class as run_rs_wiring_call_sites_are_pinned:
        // removing a gate arm produces no warning and no test failure
        // anywhere else.
        for src in [include_str!("tui/run.rs"), include_str!("tui/intro.rs")] {
            assert!(
                src.contains("crate::slash::is_context_command(&raw)"),
                "a pre-session gate lost its /context arm"
            );
        }
    }
```

  3. `src/file_feedback.rs` test modülüne:

```rust
    #[test]
    fn is_delivery_turn_matches_the_injected_file_frames() {
        assert!(is_delivery_turn(PENDING_PREAMBLE));
        assert!(is_delivery_turn("[Files changed]\nFILE: x"));
        assert!(is_delivery_turn("[File saved: src/main.rs]\n..."));
        assert!(is_delivery_turn("[File changed: src/main.rs]\n..."));
        assert!(is_delivery_turn("[Exercise submission saved: e.md]\n..."));
        assert!(!is_delivery_turn("[EXAM MODE — MOCK EXAM]\n..."));
        assert!(!is_delivery_turn("just a user message"));
    }
```

  4. `src/context_report.rs` (yeni dosya — test modülüyle birlikte Step 3'te yazılacak; failing aşaması modülün var olmamasıdır).
- [ ] **Step 2:** `cargo test section_sizes is_context_command is_delivery_turn` → Beklenen: **derlenmez**. Doğru sebeple fail.
- [ ] **Step 3: Implementasyon.**
  1. `src/brain.rs` — `FALLBACK_SYSTEM`'in ÜSTÜNE ekle:

```rust
/// Split an assembled system prompt back into (label, body-bytes) pairs —
/// the exact inverse of the `===== label =====` sections joined above.
/// Single-sourced from the same format on purpose: the /context report
/// measures the REAL string the model receives (including any drift after
/// mid-session file edits), never a recomputation. Text before the first
/// divider — the embedded fallback prompt — is labeled "(fallback)". Body
/// bytes count each line plus one newline; header lines and the blank join
/// lines are not attributed to any section.
pub fn section_sizes(system: &str) -> Vec<(String, usize)> {
    let mut out: Vec<(String, usize)> = Vec::new();
    let mut current: Option<String> = None;
    let mut size = 0usize;
    for line in system.lines() {
        if let Some(label) = section_label(line) {
            if let Some(l) = current.take() {
                out.push((l, size));
            }
            current = Some(label.to_string());
            size = 0;
        } else if current.is_some() {
            size += line.len() + 1;
        } else if !line.trim().is_empty() {
            current = Some("(fallback)".to_string());
            size = line.len() + 1;
        }
    }
    if let Some(l) = current {
        out.push((l, size));
    }
    out
}

/// `===== X =====` → `Some("X")`.
fn section_label(line: &str) -> Option<&str> {
    line.strip_prefix("===== ")?.strip_suffix(" =====")
}
```

  2. `src/file_feedback.rs` — `PENDING_PREAMBLE`'ın ALTINA ekle:

```rust
/// A history turn that is a shell-injected file delivery (single save,
/// live batch, or ride-along) — the /context report's classification hook,
/// single-sourced next to the frames it matches.
pub(crate) fn is_delivery_turn(text: &str) -> bool {
    text.starts_with(PENDING_PREAMBLE)
        || text.starts_with("[Files changed]")
        || text.starts_with("[File saved")
        || text.starts_with("[File changed")
        || text.starts_with("[Exercise submission")
}
```

  3. `src/slash.rs` — `is_exam_command`'ın yanına ekle:

```rust
/// True when the line is exactly `/context` (trimmed, case-insensitive) —
/// the deterministic context-window breakdown (spec F1). TUI-only surface;
/// the plain path never parses it (plain.rs is frozen).
pub(crate) fn is_context_command(line: &str) -> bool {
    line.trim().eq_ignore_ascii_case("/context")
}
```

  4. `src/context_report.rs` (yeni dosya, tamamı):

```rust
//! `/context` — deterministic context-window breakdown (spec F1). Exact
//! bytes from the LIVE session: the system prompt is parsed back through
//! brain::section_sizes (the assembly's own divider format — single-sourced,
//! never recomputed) and the history is classified by role and injection
//! prefix. Token figures are estimates and say so; the backend's last
//! reported usage is shown alongside because the gap between the two —
//! backend overhead, caching — is itself diagnostic. No LLM call anywhere
//! in this module.

use crate::anthropic::Message;

/// Rough tokens-from-bytes estimate; always labeled as such in the output.
fn est_tokens(bytes: usize) -> usize {
    bytes / 4
}

fn message_text(m: &Message) -> String {
    match &m.content {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Build the full report — pure string work over the live session state.
pub(crate) fn build(
    system: &str,
    history: &[Message],
    last_reported: Option<u64>,
    window: u64,
) -> String {
    let mut out = String::from(
        "context breakdown — exact bytes; token figures are estimates (bytes / 4)\n\n",
    );
    out.push_str(&format!(
        "system prompt: {} bytes (~{} tokens)\n",
        system.len(),
        est_tokens(system.len())
    ));
    for (label, bytes) in crate::brain::section_sizes(system) {
        out.push_str(&format!("  {label:<34} {bytes} bytes\n"));
    }
    // History buckets: the user's own words, the mentor's replies, injected
    // file deliveries, and other shell-injected directives.
    let mut buckets: [(&str, usize, usize); 4] = [
        ("your messages", 0, 0),
        ("usta's replies", 0, 0),
        ("file deliveries", 0, 0),
        ("injected directives", 0, 0),
    ];
    let mut history_bytes = 0usize;
    for m in history {
        let text = message_text(m);
        history_bytes += text.len();
        let idx = if m.role == "assistant" {
            1
        } else if crate::file_feedback::is_delivery_turn(&text) {
            2
        } else if text.starts_with('[') {
            3
        } else {
            0
        };
        buckets[idx].1 += text.len();
        buckets[idx].2 += 1;
    }
    out.push_str(&format!(
        "history: {} bytes (~{} tokens) across {} turns\n",
        history_bytes,
        est_tokens(history_bytes),
        history.len()
    ));
    for (name, bytes, count) in &buckets {
        out.push_str(&format!("  {name:<34} {bytes} bytes ({count} turns)\n"));
    }
    let total = system.len() + history_bytes;
    out.push_str(&format!(
        "total: {} bytes (~{} tokens) — window: {}k tokens\n",
        total,
        est_tokens(total),
        window / 1000
    ));
    match last_reported {
        Some(t) => out.push_str(&format!(
            "last call reported: {t} tokens (API side — the gap vs the estimate \
includes backend overhead and caching, and is itself diagnostic)"
        )),
        None => out.push_str(
            "last call reported: nothing yet — no usage on record for this session",
        ),
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sys() -> String {
        "===== TODAY =====\n2026-08-29\n\n===== SOUL.md =====\nCORE RULE".to_string()
    }

    #[test]
    fn report_lists_sections_buckets_and_labels_the_estimate() {
        let history = vec![
            Message::user("hello there"),
            Message::assistant_raw(serde_json::Value::String("hi".into())),
            Message::user("[Files changed]\nFILE: src/main.rs (full contents)\nfn main() {}"),
            Message::user("[EXAM MODE — MOCK EXAM]\nTopic: rust."),
        ];
        let r = build(&sys(), &history, Some(131_072), 200_000);
        assert!(r.contains("estimates"));
        assert!(r.contains("TODAY"));
        assert!(r.contains("SOUL.md"));
        assert!(r.contains("your messages"));
        assert!(r.contains("usta's replies"));
        assert!(r.contains("file deliveries"));
        assert!(r.contains("injected directives"));
        assert!(r.contains("(1 turns)"), "each bucket counted exactly one turn");
        assert!(r.contains("last call reported: 131072 tokens"));
        assert!(r.contains("across 4 turns"));
        assert!(r.contains("bytes"));
    }

    #[test]
    fn report_says_so_when_no_usage_was_reported() {
        // The introduction path drops context_tokens (known M11 gap) — the
        // report must say "nothing yet" instead of inventing a number.
        let r = build(&sys(), &[], None, 200_000);
        assert!(r.contains("nothing yet"));
    }
}
```

  5. `src/main.rs` — `mod config;` satırının ALTINA `mod context_report;` ekle.
  6. `src/help.rs` — `/game on|off` satırının ALTINA ekle:

```
     \x20\x20/context         what fills the context window (exact bytes; token counts are estimates)\n\
```

  ve `help_text_lists_shortcuts_commands_and_cli` iğne listesine `"/context"` ekle.
  7. `src/tui/run.rs` — `/help` bloğunun (`if crate::help::is_help_command(&line) { … }`) hemen ALTINA ekle (+6 satır):

```rust
                        if crate::slash::is_context_command(&line) {
                            crate::tui::page::page_user_echo(&mut tui, &line)?;
                            // Deterministic breakdown — shell-only, no LLM call (spec F1).
                            crate::tui::page::page_notice(&mut tui, &crate::context_report::build(&session.system, session.history(), last_tokens, window))?;
                            continue;
                        }
```

  8. Kapılar: `src/tui/run.rs` konu-girişi döngüsündeki ve `src/tui/intro.rs` `run_intro` içindeki mevcut

```rust
            if crate::visual::parse_show_command(&raw).is_some()
                || crate::slash::parse_watch_command(&raw).is_some()
```

  koşullarının HER İKİSİNE üçüncü kol ekle: `|| crate::slash::is_context_command(&raw)` (+1'er satır; mevcut notis metinleri değişmez).
- [ ] **Step 4: Isırma doğrulaması.** run.rs'teki kapı satırından `|| crate::slash::is_context_command(&raw)` kısmını geçici sil → `cargo test context_command_is_gated` FAIL → geri al. `/context` dalını geçici yorum yap → `cargo clippy --all-targets`'ın `context_report::build` için dead_code uyarısı verdiğini gör (mekanik pin) → geri al.
- [ ] **Step 5:** Doğrula: `grep -c "" src/tui/run.rs` → Beklenen: **≈592** ve ≤ 600. `cargo test` → TÜMÜ yeşil (yeni: `section_sizes_*`, `is_context_command_*`, `context_command_is_gated*`, `is_delivery_turn_*`, `report_*`; REGRESYON: `help_text_lists_shortcuts_commands_and_cli`, `run_wires_intro_flow`).
- [ ] **Step 6:** `cargo fmt -- src/context_report.rs src/brain.rs src/slash.rs src/help.rs src/file_feedback.rs src/tui/run.rs src/tui/intro.rs src/main.rs` · `cargo clippy --all-targets` → 0 uyarı.
- [ ] **Step 7:** Commit:

```bash
git add src/context_report.rs src/brain.rs src/slash.rs src/help.rs src/file_feedback.rs src/tui/run.rs src/tui/intro.rs src/main.rs
git commit -m "feat: /context — deterministic context-window breakdown, exact bytes, estimates labeled (finding F)"
```

---

### Task 8: Dokümantasyon + sürüm — v0.29.0

**Files:**
- Modify: `Cargo.toml` (version)
- Modify: `docs/ROADMAP.md` (Completed girdisi)
- Modify: `SPEC.md` (§4.21 v0.28.0 bloğuna ek cümle; yeni §4.23; §4.6 satır 65 kontrolü)
- Modify: `README.md` ("Companion watching" bölümü + Interface/Highlights'a `/context`)
- Modify: `PREDICTION.md` (`[build state:]` satırı tanımı)

**Interfaces:** yalnız dokümantasyon — kod değişikliği YOK. `PREDICTION.md` embed'li dağıtım dosyasıdır (`write_global_defaults` senkronu mevcut mekanizmayla dağıtır — ekstra iş yok); değişikliği Görev 6'nın not metniyle TUTARLI tut.

- [ ] **Step 1: `Cargo.toml`.** `version = "0.29.0"` yap; `cargo check` çalıştır (Cargo.lock'taki `usta` sürümünü yeniler).
- [ ] **Step 2: `PREDICTION.md`.** Mevcut son maddeden ("If the block never arrives…") ÖNCE yeni madde ekle:

```
- You may also get a one-line `[build state: the last cargo check was still failing — …]` note instead of the full block. It means no new check ran this turn and the project has NOT compiled since the last one that did. Treat it as ground truth: do not declare the current step complete, and do not invent a fresher compile result than the note states.
```

- [ ] **Step 3: `SPEC.md`.**
  1. §4.6'daki 4. madde (satır ~65, "Prediction protocol") sonuna ekle: ` The verdict of the last check that ran is remembered by the shell (v0.29.0): while it is red, every delivered turn carries a one-line `[build state: …]` note and the status line shows a dim `✗ check failing` marker — memory, not extra executions.`
  2. §4.22'den SONRA yeni bölüm ekle:

```
## 4.23 Session Awareness (v0.29)

Four fixes from the first live v0.28.0 session, one root: the mentor's world model was refreshed only by file-content saves. (1) **Identity in the introduction** — the pre-lock introduction now also learns the user's name, background and learning style (woven in, never as a form; rule 1 licenses the questions), so USER.md's Who section fills and later sessions know the learner; MEET_BLOCK still serves the plain path and `usta start <topic>`. (2) **Verification verdict memory** — the shell remembers the last `cargo check` verdict (`check::VerifyMonitor`, neutral name on purpose: the concept is "the project's own verification signal, when it has one"; Cargo is today's only implementation and projects without a verifier get a complete, tested no-op). Checks run exactly where they ran before — deliveries carrying a non-exercise file, and the live path; while the last verdict is red, every delivered turn carries a one-line `[build state: …]` note and the status line shows a dim `✗ check failing` marker. Honest staleness: after a fixing save the cache stays red until the message that delivers it — a one-turn, self-closing window. A background-check-at-flush design was evaluated and rejected (complexity buys only cold-start latency; "no compiles during silent accumulation" stands). (3) **Structure signal** — the watcher forwards Create/Remove events; at flush the shell classifies deterministically: new directories, deleted known files and removed known directories become one-line `STRUCTURE:` notes that accumulate in `PendingChanges` (capped at 20, overflow collapses to one line, counter counts them) and ride along with the user's next message in BOTH modes — never a turn (K1), never contents (the directory-contents decision stands), `/watch off` drops them too. Renames appear as a deletion note plus the new file's content; no correlation. (4) **READ/WRITE asymmetry + audit** — `flow_frame` now states that an artifact the learner was asked to READ/run/describe stays eyes-only until their report, while an artifact they were asked to WRITE/change must be AUDITED: three parts — what was done (with evidence), what is missing or wrong, the single next step; Hard Rule 2 unchanged. Plus `/context`: a deterministic, shell-only breakdown of what fills the context window — exact bytes (system prompt parsed back through the assembly's own `===== label =====` dividers via `brain::section_sizes`, history in four buckets), token figures explicitly labeled estimates, the backend's last reported usage alongside. TUI-only; the plain path is unchanged. Design: `docs/superpowers/specs/2026-08-29-session-awareness-design.md`.
```

- [ ] **Step 4: `README.md`.** "Companion watching" bölümünün ilk paragrafının sonuna ekle: ` Structural changes ride along too — a new directory or a deleted file shows up as a one-line note, never its contents. In a Cargo project, Usta also remembers the last `cargo check` verdict: while it's red, the status line shows a dim `✗ check failing` and the mentor won't call a step done.` — ve "Interface" (veya Highlights) bölümündeki komut listesine `/context` satırı ekle: `` `/context` — what fills the context window (exact bytes; token counts are estimates)``.
- [ ] **Step 5: `docs/ROADMAP.md`.** "## Completed" başlığının hemen altına yeni girdi:

```
- 2026-08-29: Session awareness — four live-session findings (ledger: "CANLI KULLANIM BULGULARI — v0.28.0") plus /context. Introduction asks who the learner is (name/background/learning style, woven in — finding A closes the 13a regression). The last verification verdict is REMEMBERED (`check::VerifyMonitor`, neutral seam gated on `is_cargo_project`; complete no-op without a verifier): red state = one-line `[build state:]` note on every delivered turn + dim `✗ check failing` marker; check execution points unchanged — memory, not frequency (finding C; background-at-flush evaluated and rejected). Structure signal: Create/Remove events forwarded, flush-time classification, `STRUCTURE:` one-liners (new dir / deleted known file / removed known dir, capped 20) accumulate in PendingChanges and ride along — never a turn, never contents (finding D). flow_frame states the READ/WRITE asymmetry and audits WRITE-deliveries in three parts: done-with-evidence / missing / single next step (finding E, undoing v0.28.0's over-correction risk). `/context`: deterministic byte-exact context breakdown, estimates labeled, last reported usage alongside (finding F; SPEND tracking explicitly out of scope). run.rs's /watch arm displaced to `polite::handle_watch_command` (budget). Design: `docs/superpowers/specs/2026-08-29-session-awareness-design.md`. v0.29.0.
```

- [ ] **Step 6: Final kapılar.** `cargo test` → tümü yeşil · `cargo clippy --all-targets` → 0 uyarı · `cargo fmt --check` → crate-genelinde temiz · `grep -c "" src/tui/run.rs` → ≤ 600 · `grep -rn "build state" src PREDICTION.md SPEC.md | grep -v test` → not metni üç yerde tutarlı (check.rs üretici, PREDICTION.md/SPEC.md tanımlayıcı) · `grep -n "watch polite" src -r` → boş.
- [ ] **Step 7: Elle duman testi (10 dk, LLM anahtarı olan ortamda; yoksa atla ve commit mesajına `smoke: skipped` yaz).** Taze bir Cargo deneme projesinde `usta start rust`: (1) `src/main.rs`'e kasıtlı tip hatası yaz + kaydet + "bak bakalım" mesajı → eyes-only check bloklu tur, ardından durum satırında `✗ check failing`; (2) kaydetmeden iki mesaj daha → her turda Usta'nın kırmızı durumdan haberdar kaldığını gör (adımı "bitti" ilan etmemeli); (3) hatayı düzelt + kaydet + mesaj → işaret söner; (4) `mkdir brands/marka-a` → sayaç artar, sonraki mesajda Usta klasörü sormadan bilir; (5) `/context` → döküm basılır, tur açılmaz; (6) `/watch off` → düşürme notisi not sayısını da içerir. Cargo-DIŞI bir dizinde kısa oturum: hiçbir `✗`/`[build state:]` görünmez.
- [ ] **Step 8:** Commit (tag/push/install YOK — insan kararı):

```bash
git add Cargo.toml Cargo.lock docs/ROADMAP.md SPEC.md README.md PREDICTION.md
git commit -m "docs: session awareness — SPEC 4.23, README, ROADMAP, PREDICTION build-state note; bump to v0.29.0"
```

---

## Spec'in açık bıraktığı yerlerde verilen kararlar (yürütücü ve insan için)

1. **`error_summary` kapağı 200 byte** (`truncate_output` yeniden kullanılır) — spec "kısa özet" der, sayı vermez; tek satırlık not için yeterli, mevcut kırpıcı tek kaynak.
2. **Yapı notu yolu gösterimi:** `strip_prefix(project_root)` başarısızsa mutlak yol basılır (watcher'ın mutlak yolları — `is_exercise_path` fallback emsali). Notlar batch sırasını korur.
3. **`/context` bölüm hizası 34 karakter** — en uzun beklenen etiket (`learner/curriculum/<topic>.md`) sığar; taşan etiket hizayı bozar, kırpılmaz (dürüstlük > kozmetik).
4. **`handle_watch_command` notis sırası** eski inline bloğun BASIM sırasını korur (drop notisi önce, toggle mesajı sonra) — test bunu pinler.
5. **Monitör `record` çağrısı `run_check`'in None döndüğü turda yapılmaz** — kararname korunur; spec'in "timeout kararnameyi bozmaz" kenarı buradan çıkar.
6. **Draw çağrılarındaki `false`** oturum-öncesi yüzeylerde (ask/intro/entry) sabittir — monitör oturumla doğar; tanışma sırasında işaret göstermek mümkün değil ve gereksiz.
7. **`section_sizes` gövde-byte tanımı** "satır + newline" — başlık satırları ve join boşlukları hiçbir bölüme yazılmaz; toplam sistem byte'ı ayrıca `system.len()` ile RAPORLANIR, yani bölüm toplamı ile tepe rakam arasındaki küçük fark başlık ek yüküdür (bilinçli, dökümde açıklanmaz — sayılar zaten "exact bytes of the real string" sözünü tutar).

## İlgili

- `docs/superpowers/specs/2026-08-29-session-awareness-design.md` — bağlayıcı tasarım (çelişkide o kazanır)
- `docs/superpowers/specs/2026-08-28-watcher-turn-taking-design.md` — K1–K6 zemini
- `docs/superpowers/plans/2026-08-28-watcher-turn-taking.md` — yapı emsali (pin protokolü, bütçe disiplini)
- `.superpowers/sdd/progress.md` "CANLI KULLANIM BULGULARI — v0.28.0" — BULGU A/C/D/E kanıtları
