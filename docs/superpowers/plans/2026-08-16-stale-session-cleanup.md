# Yarım Kalmış Oturum Temizliği Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Ön-koşul:** Yok (v0.18.2 üstüne). Spec: `docs/superpowers/specs/2026-08-16-stale-session-cleanup-design.md` — önce oku.

**Goal:** Açılıştaki "half-finished session record" uyarısının ardından TTY'de tek onaylı silme önerisi; pipe/red yolunda davranış birebir aynı. v0.18.3.

## Global Constraints

- Default HAYIR (boş Enter = silme). Kabul kümesi mevcut `confirm` konvansiyonu: `["e","evet","y","yes"]`.
- TTY-değil yol + red yolu: bugünkü davranış birebir (yalnız warn).
- Binary crate — `cargo test <filtre>`. Commit Türkçe + push; sonda 0.18.3 + tag + `cargo install --path .`.

---

### Task 1: `transcript::delete_unflushed` yardımcısı

**Files:** Modify: `src/transcript.rs` · test in-module

- [ ] **Step 1: Failing test**

```rust
#[test]
fn delete_unflushed_removes_only_given_files_reports_errors() {
    let base = std::env::temp_dir().join(format!("usta_transcript_del_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let sdir = base.join(".usta/sessions");
    std::fs::create_dir_all(&sdir).unwrap();
    std::fs::write(sdir.join("a-1.jsonl"), "x").unwrap();
    std::fs::write(sdir.join("b-2.jsonl"), "x").unwrap();
    std::fs::write(sdir.join("c-3.done.jsonl"), "x").unwrap();

    let files = vec![sdir.join("a-1.jsonl"), sdir.join("b-2.jsonl"), sdir.join("yok.jsonl")];
    let (deleted, errors) = delete_unflushed(&files);
    assert_eq!(deleted, 2);
    assert_eq!(errors.len(), 1);
    assert!(!sdir.join("a-1.jsonl").exists());
    assert!(sdir.join("c-3.done.jsonl").exists()); // .done'a dokunulmaz

    let _ = std::fs::remove_dir_all(&base);
}
```

- [ ] **Step 2:** Run: `cargo test delete_unflushed` → derleme hatası

- [ ] **Step 3: Implement**

```rust
/// Delete the given half-finished session records. Only ever called with the
/// list produced by `unflushed` — never touches `.done` files by construction.
/// Errors are collected, not fatal: a leftover record must never block startup.
pub fn delete_unflushed(files: &[PathBuf]) -> (usize, Vec<String>) {
    let mut deleted = 0;
    let mut errors = Vec::new();
    for f in files {
        match std::fs::remove_file(f) {
            Ok(()) => deleted += 1,
            Err(e) => errors.push(format!("{}: {e}", f.display())),
        }
    }
    (deleted, errors)
}
```

- [ ] **Step 4:** `cargo test delete_unflushed` → PASS
- [ ] **Step 5:** Commit + push: `temizlik: transcript::delete_unflushed — verilen kayıtları siler, hata toplar`

---

### Task 2: Açılış onay akışı (`src/main.rs:95` civarı)

**Files:** Modify: `src/main.rs` (uyarı döngüsü)

- [ ] **Step 1: Implement** — mevcut warn döngüsünü koru, ardına ekle (koda bak — `confirm` yardımcısının gerçek imzası + sihirbaz entegrasyonundaki TTY koşulu neyse aynısı):

```rust
    let stale = transcript::unflushed(&project_root); // mevcut çağrı neyse
    if !stale.is_empty() {
        for p in &stale {
            ui::warn(&format!("half-finished session record found (may not have been flushed): {}", p.display()));
        }
        use std::io::IsTerminal;
        if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
            let q = format!("delete {} half-finished session record(s)? [y/N] ", stale.len());
            if confirm(&q, &["e", "evet", "y", "yes"])? {
                let (deleted, errors) = transcript::delete_unflushed(&stale);
                for e in errors {
                    ui::warn(&format!("could not delete: {e}"));
                }
                ui::notice(&format!("deleted {deleted} record(s)"));
            }
        }
    }
```

DİKKAT: mevcut kodda uyarı nasıl üretiliyorsa (döngü/values) onu yeniden kullan; `confirm` reddinde ve TTY-değilken akış aynen devam etmeli (oturum açılışı engellenmez).

- [ ] **Step 2:** `cargo build && cargo test` → tümü PASS
- [ ] **Step 3:** Commit + push: `temizlik: açılışta yarım kayıtlar için tek onaylı silme önerisi (TTY-only, default hayır)`

---

### Task 3: SPEC + v0.18.3

**Files:** `SPEC.md`, `Cargo.toml`(+lock), sürüm testi

- [ ] **Step 1:** SPEC'te half-finished uyarısını anlatan maddeye tek cümle: TTY'de onaylı silme önerisi (default hayır), pipe'ta yalnız uyarı.
- [ ] **Step 2:** Cargo `0.18.3`; `version_aligned_with_spec` testi `"0.18.3"`; `cargo build`.
- [ ] **Step 3:** Verify: `cargo test` PASS · clippy yeni uyarı 0 · `cargo install --path .`.

```bash
git add SPEC.md Cargo.toml Cargo.lock src/
git commit -m "temizlik: SPEC + v0.18.3"
git push
git tag v0.18.3 && git push --tags
```

- [ ] **Step 4 (elle doğrulama — ATLA, Anil koşacak):** stagit'te `usta` → uyarılar + "delete 2 ...? [y/N]" → `y` → silinir, bir sonraki açılış temiz; pipe'ta (`echo | usta ...`) soru sorulmaz.
