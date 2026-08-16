# Yarım Kalmış Oturum: Otomatik Salvage Flush Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

> **REVİZE:** Bu plan "onaylı silme" planının yerine geçti (Anil kararı: sormadan direkt flush). Önceki plandan Task 1 (`delete_unflushed`) implement edildiyse KALDIR — salvage rename kullanır, silme yardımcısına gerek yok.

**Ön-koşul:** Yok (v0.18.2 üstüne). Spec: `docs/superpowers/specs/2026-08-16-stale-session-cleanup-design.md` (revize hali) — önce oku.

**Goal:** Açılışta yarım kayıt → sormadan kurtarma: transcript parse → kapanış flush'ı o history ile koşar → dosyalar yazılır → kayıt `.done` rename. TTY-değilse eski warn davranışı. v0.18.3.

**Architecture:** `transcript.rs`'e okuma tarafı (`read_history`, konu çıkarımı, rename). `flush_progress` (main.rs) `(topic, history, system)` alan yeniden kullanılabilir çekirdeğe ayrılır — hem normal kapanış hem salvage onu çağırır. Salvage `main`'de backend seçiminden sonra, konu/oturum akışından önce.

## Global Constraints

- Salvage YALNIZ TTY'de (stdin+stdout `is_terminal` — sihirbaz koşulunun aynısı); pipe'ta mevcut warn birebir.
- Hata asla açılışı engellemez: her hata warn + kayıt yerinde kalır (bir sonraki açılışta tekrar denenir).
- Kullanıcıya soru YOK; kayıt başına tek `notice`.
- Binary crate — `cargo test <filtre>`. Her task commit (Türkçe) + push; sonda 0.18.3 + tag + `cargo install --path .`. clippy yeni uyarı 0.

---

### Task 1: Transcript okuma tarafı (`src/transcript.rs`)

**Files:** Modify: `src/transcript.rs` · testler in-module

**Interfaces (Produces):**
- `pub fn read_history(path: &Path) -> Result<Vec<Message>>` — Recorder'ın YAZDIĞI jsonl formatını okur (önce yazma formatına bak, birebir tersi; bozuk satır → Err).
- `pub fn topic_from_record(path: &Path) -> Option<String>` — `<topic>-<YYYYMMDD>-<HHMMSS>.jsonl` → `topic` (sondan timestamp deseni soyulur; konu adında tire OLABİLİR — sondan iki `-` bloğu sayısal+uzunluk kontrolüyle soyulur; uymuyorsa None).
- `pub fn mark_done(path: &Path) -> Result<PathBuf>` — `x.jsonl` → `x.done.jsonl` rename (mevcut done-işaret konvansiyonunun aynısı — koda bak, done nasıl işaretleniyorsa o mekanizmayı YENİDEN KULLAN; ayrı fonksiyon zaten varsa onu pub yap, yenisini yazma).

- [ ] **Step 1: Failing testler**

```rust
#[test]
fn read_history_roundtrips_recorder_output() {
    // tmpdir: Recorder ile 2 user + 1 assistant turu yaz → read_history aynı sırayla döndürür
}

#[test]
fn topic_from_record_strips_timestamp_keeps_hyphenated_topic() {
    assert_eq!(topic_from_record(Path::new("kaynak-ingest-20260814-153309.jsonl")).as_deref(), Some("kaynak-ingest"));
    assert_eq!(topic_from_record(Path::new("rust-20260807-1030.jsonl")).as_deref(), Some("rust"));
    assert!(topic_from_record(Path::new("garip.jsonl")).is_none());
}

#[test]
fn mark_done_renames_and_unflushed_no_longer_finds_it() {
    // tmpdir: işaretsiz dosya → mark_done → unflushed boş döner
}
```

(Gövdeler transcript.rs'nin gerçek yazma API'siyle doldurulur — Recorder imzasına bak. Timestamp deseni mevcut dosya-adı üretimindeki formatla birebir aynı olmalı; üretim kodundaki formatı sabit/fonksiyon olarak paylaş.)

- [ ] **Step 2:** `cargo test transcript` → derleme hatası
- [ ] **Step 3:** Implement — parse: her satır jsonl (`role` + `content` neyse), Message'a eşle; bilinmeyen role → Err. `topic_from_record`: dosya kökünden sondan `-\d{8}-\d+` benzeri iki bloğu soy (yazma formatıyla senkron).
- [ ] **Step 4:** `cargo test transcript` → PASS
- [ ] **Step 5:** Commit + push: `salvage: transcript okuma — read_history + topic_from_record + mark_done`

---

### Task 2: `flush_progress` çekirdeğini yeniden kullanılabilir yap

**Files:** Modify: `src/main.rs`

- [ ] **Step 1:** `flush_progress`'in gövdesini `(backend, topic: &str, system: &str, history: &[Message], project_root, record_history: bool)` alan `flush_core` benzeri fonksiyona çıkar; mevcut `flush_progress(backend, session, project_root, record_history)` onu `session.topic/session.system/session.history()` ile çağırır. DAVRANIŞ DEĞİŞMEZ — saf refactor, mevcut testler yeşil kalmalı.
- [ ] **Step 2:** `cargo test` → tümü PASS. Commit + push: `salvage: flush çekirdeği session'dan bağımsızlaştı (saf refactor)`

---

### Task 3: Açılış salvage akışı (`src/main.rs` unflushed tarama noktası)

**Files:** Modify: `src/main.rs` (mevcut warn döngüsünün yeri, ~satır 95)

- [ ] **Step 1: Implement** — mevcut döngünün yerine:

```rust
    let stale = /* mevcut unflushed çağrısı */;
    if !stale.is_empty() {
        use std::io::IsTerminal;
        let tty = std::io::stdin().is_terminal() && std::io::stdout().is_terminal();
        if !tty {
            for p in &stale { ui::warn(&format!("half-finished session record found (may not have been flushed): {}", p.display())); }
        } else {
            for p in &stale {
                let Some(topic) = transcript::topic_from_record(p) else {
                    ui::warn(&format!("unrecognized session record name, leaving as-is: {}", p.display()));
                    continue;
                };
                match transcript::read_history(p) {
                    Err(e) => ui::warn(&format!("could not read session record ({e}) — leaving as-is: {}", p.display())),
                    Ok(h) if h.iter().filter(|m| /* user turn */ true).count() == 0 => {
                        // kurtarılacak içerik yok — sessizce kapat (gürültü kalmasın)
                        let _ = transcript::mark_done(p);
                    }
                    Ok(history) => {
                        ui::notice(&format!("recovering unflushed session: {} — writing files…", p.display()));
                        let system = brain::load_system_prompt(&global, Some(&project_root), &topic, &today());
                        match flush_core(&mut backend, &topic, &system, &history, &project_root, true).await {
                            Ok(()) => {
                                let _ = transcript::mark_done(p);
                                ui::notice(&format!("recovered: {topic}"));
                            }
                            Err(e) => ui::warn(&format!("recovery failed ({e}) — record kept, will retry next start: {}", p.display())),
                        }
                    }
                }
            }
        }
    }
```

Uyum notları (koda bak): `global`/`project_root`/`backend` bu noktada gerçekte hangi sırayla hazır — salvage backend seçiminden SONRA durmalı, gerekirse tarama noktasını taşı; user-turn filtresi Message tipinin gerçek role alanıyla; boş-history eşiği "hiç user turn yok". `backend.reset_session()` gerekiyorsa flush çağrıları arasında (CLI session kirliliği — slug mini-session paritesine bak).

- [ ] **Step 2:** `cargo build && cargo test` → PASS. Commit + push: `salvage: açılışta yarım kayıt sormadan kurtarılır — flush + .done rename (TTY-only)`

---

### Task 4: SPEC + v0.18.3

- [ ] **Step 1:** SPEC'te half-finished maddesi güncellenir: otomatik salvage (TTY), pipe'ta warn, hata=bekle-tekrar-dene.
- [ ] **Step 2:** Cargo `0.18.3`; sürüm testi `"0.18.3"`; `cargo build`.
- [ ] **Step 3:** Verify: `cargo test` PASS · clippy yeni uyarı 0 · `cargo install --path .`.

```bash
git add SPEC.md Cargo.toml Cargo.lock src/
git commit -m "salvage: SPEC + v0.18.3"
git push
git tag v0.18.3 && git push --tags
```

- [ ] **Step 4 (elle doğrulama — ATLA, Anil koşacak):** stagit'te `usta` → "recovering unflushed session … recovered: kaynak-ingest" ×2 → progress dosyaları oluşmuş, kayıtlar `.done.jsonl`, bir sonraki açılış uyarısız; `echo | usta ...` pipe'ta yalnız warn.
