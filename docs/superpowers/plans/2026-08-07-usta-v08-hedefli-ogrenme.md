# Usta v0.8 — Hedefli Öğrenme Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Öğrenmenin iki modunu tek sistemde birleştir: **keşif** (Rust merakı) ve **hedef** (AWS sertifikası, PMP, Goethe B1 — tarih + geçme eşiği olan somut hedefler). Hedefliyse: harita resmi çerçeveden kurulur, geriye-doğru planlama + tempo bekçiliği yapılır ("8 hafta kaldı, haritanın %30'undasın — riskli"), drill hedefin sınav formatına uyar, ölçümler loglanır.

**Architecture:** Neredeyse tamamen davranış katmanı (USTA.md + prompt formatları) — hedef kavramı jenerik bir kayıttır: approach dosyasında `## Hedef` (ne / tarih / eşik / format — kararlı tanım), progress'te `## Hedef Durumu` (kalan süre / harita ilerlemesi / tempo / ölçüm logu — değişen durum). Tek gerçek kod ihtiyacı: **model bugünü güvenilir bilmez** — `load_system_prompt` `today` parametresi alır ve system prompt'a `===== BUGÜN =====` bölümü ekler; "kaç hafta kaldı" hesabı buna dayanır.

**Tech Stack:** v0.7 sonrası yığın. Yeni bağımlılık YOK (chrono zaten var — `today()` main'de mevcut, v0.4).

## Global Constraints

- **ÖN KOŞUL: v0.2–v0.7 planlarının TAMAMI uygulanmış ve commit'lenmiş olmalı** (`load_system_prompt` çağrı yerleri v0.7'nin `maybe_compact`'ını da içerir). Bitmemişse DUR ve bildir.
- Bu repo kendi git'ine sahip (`usta/` içinde çalış, headspace repo'suna commit atma).
- Tüm kod yorumları ve kullanıcıya görünen mesajlar **Türkçe**. Modül başları `//!` doc.
- Commit başlık deseni: `<scope>: kısa türkçe özet`.
- Her görev sonunda `cargo test`, `cargo build`, `cargo clippy` temiz.
- Test isimleri `snake_case`; mevcut testler imza değişiminde UYARLANIR, silinmez.
- Saf mantık test edilebilir fonksiyonda; IO/async kabukta.
- Commit mesajı sonuna ekle: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`

## Dosya Haritası

| Dosya | Sorumluluk | Değişim |
|---|---|---|
| `src/brain.rs` | `load_system_prompt`'a `today` parametresi + BUGÜN bölümü | güncellenir |
| `src/main.rs` | İki çağrı yerine `&today()` geçilir | güncellenir |
| `src/progress.rs` | `closing_prompt`'a Hedef kuralları, `onboarding_prompt`'a hedef sorusu | güncellenir |
| `USTA.md` | "Hedefli Öğrenme" bölümü | eklenir |
| `SPEC.md` | §4.10 + kararlar | güncellenir |

---

### Task 1: Tarih enjeksiyonu — `load_system_prompt(… , today)` (TDD)

**Files:**
- Modify: `src/brain.rs`
- Modify: `src/main.rs` (iki çağrı yeri)

**Interfaces:**
- Produces: `brain::load_system_prompt(global: &Path, project: Option<&Path>, topic: &str, today: &str) -> String` — İMZA DEĞİŞİR; system prompt'un İLK bölümü `===== BUGÜN =====\n<today>` olur (tarih hesapları için sabit referans).
- Consumes: `main::today()` (v0.4'te mevcut).
- v0.6 brain testleri 4. argümanla (`"2026-08-07"` gibi sabit) UYARLANIR.

- [x] **Step 1: Failing testi yaz**

`src/brain.rs` test modülüne ekle (mevcut tüm `load_system_prompt(...)` çağrılarına 4. argüman olarak `"2026-08-07"` ekle):

```rust
#[test]
fn system_prompt_starts_with_today_section() {
    let (global, _project) = temp_pair("today");
    fs::write(global.join("USTA.md"), "ÇEKIRDEK").unwrap();
    let sys = load_system_prompt(&global, None, "rust", "2026-08-07");
    assert!(sys.starts_with("===== BUGÜN =====\n2026-08-07"));
    let _ = fs::remove_dir_all(global.parent().unwrap());
}
```

Run: `cargo test brain`
Expected: FAIL (derleme — 4. parametre yok).

- [x] **Step 2: Implemente et**

`load_system_prompt` imzasına `today: &str` ekle; gövdede `let mut parts ...` satırından hemen sonra:

```rust
    // Model bugünü güvenilir bilmez — "sınava kaç hafta kaldı" gibi hesaplar
    // için sabit referans en başta verilir (USTA.md "Hedefli Öğrenme").
    parts.push(format!("===== BUGÜN =====\n{today}"));
```

NOT: `parts` artık asla boş olamaz — fonksiyon sonundaki `if parts.is_empty()` fallback kontrolünü şuna çevir: `if parts.len() == 1` (yalnız BUGÜN varsa brain dosyaları hiç bulunamamış demektir → `FALLBACK_SYSTEM`). `falls_back_when_no_files` testi geçmeye devam etmeli.

`src/main.rs` — iki çağrı yeri güncellenir:

1. Oturum açılışı: `brain::load_system_prompt(&global, Some(&project_root), &topic, &today())`
2. `maybe_compact` içindeki yeniden yükleme (v0.7): `brain::load_system_prompt(&global, Some(project_root), &session.topic, &today())`

- [x] **Step 3: Test + build**

Run: `cargo test && cargo build && cargo clippy`
Expected: yeni test dahil hepsi PASS (fallback testi dahil).

- [x] **Step 4: Commit**

```bash
git add src/brain.rs src/main.rs
git commit -m "brain: system prompt'a BUGÜN bölümü — tarih hesapları güvenilir

Model bugünü bilmez; 'sınava X hafta kaldı' tempo bekçiliğinin referansı
artık prompt'ta.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Prompt formatları — Hedef tanımı + Hedef Durumu (TDD)

**Files:**
- Modify: `src/progress.rs` (`closing_prompt` ve `onboarding_prompt` gövdeleri — imzalar AYNI)

**Interfaces:**
- `closing_prompt` kurallarına eklenir: approach'ta `## Hedef` (kararlı tanım), progress'te `## Hedef Durumu` (değişen durum; hedef yoksa bölüm yazılmaz).
- `onboarding_prompt`'a keşif-mi-hedef-mi sorusu eklenir.

- [ ] **Step 1: Failing testleri yaz**

`src/progress.rs` test modülüne:

```rust
#[test]
fn closing_prompt_defines_goal_sections() {
    let s = closing_prompt("almanca", None, None, None);
    assert!(s.contains("## Hedef Durumu"));
    assert!(s.contains("## Hedef"));
    assert!(s.contains("tempo"));
}

#[test]
fn onboarding_prompt_asks_exploration_or_goal() {
    let s = onboarding_prompt("almanca");
    assert!(s.contains("keşif mi"));
    assert!(s.contains("hedef"));
}
```

Run: `cargo test 'goal'` ve `cargo test onboarding`
Expected: FAIL.

- [ ] **Step 2: Implemente et**

`closing_prompt` format string'inde iki kural satırını değiştir/ekle:

1. `- \`approach\` yalnız ilk oturumda veya ...` satırının SONUNA ekle:

```
 Hedefli öğrenmede approach `## Hedef` bölümü içerir: ne (sertifika/seviye/çıktı), \
 sınav-değerlendirme tarihi (YYYY-MM-DD), geçme eşiği, sınav/değerlendirme formatı.
```

2. progress yapı listesine (`## İpucu merdiveni` maddesinden sonra) yeni madde:

```
 `## Hedef Durumu` — SADECE approach'ta `## Hedef` tanımlıysa yaz: kalan süre \
 (BUGÜN bölümünden hesapla), harita ilerlemesi (%), tempo değerlendirmesi \
 (yetişir / riskli / yetişmez + tek cümle gerekçe), ölçüm logu \
 (`tarih | ölçüm | skor` — deneme sınavı, yazma değerlendirmesi vb.). \
 Hedef yoksa bu bölümü hiç yazma.
```

`onboarding_prompt` format string'ine ("Kısa bir tanışma başlat: ..." cümlesinden sonra) ekle:

```
 Şunu mutlaka netleştir: bu keşif mi (merak, açık uçlu), yoksa somut bir hedef mi \
 (sertifika, seviye, tarihli çıktı — ör. AWS SAA, Goethe B1, PMP)? Hedefliyse: ne, \
 hangi tarihte, geçme eşiği ne, formatı ne — approach'un `## Hedef` bölümüne yazılacak. \
 Harita resmi çerçeveden kurulur (sınav müfredatı / exam guide / CEFR) — web'de araştır.
```

- [ ] **Step 3: Test + build**

Run: `cargo test && cargo build && cargo clippy`
Expected: yeni 2 test dahil hepsi PASS (v0.6/v0.7 prompt testleri bozulmadan).

- [ ] **Step 4: Commit**

```bash
git add src/progress.rs
git commit -m "progress: hedef kavramı — approach'ta tanım, progress'te durum

Keşif ve hedefli öğrenme aynı sistemin iki ayarı; tanışma hangisi
olduğunu netleştirir, tempo/ölçüm kapanışta loglanır.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: USTA.md — "Hedefli Öğrenme" kuralları

**Files:**
- Modify: `USTA.md`

- [ ] **Step 1: Bölümü ekle**

"Kapsam Bekçiliği" bölümünden sonra:

```markdown
## Hedefli Öğrenme — keşif ve hedef aynı sistem

Öğrenmenin iki modu var; tanışmada hangisi olduğunu öğren:
- **Keşif:** merak, açık uçlu (Rust'a bakmak). Normal akış.
- **Hedef:** somut sonuç + tarih + eşik (AWS sertifikası, PMP, Goethe B1, iş teslimi). Approach'ta `## Hedef` tanımlanır, aşağıdaki kurallar devreye girer.

Hedef kuralları:

1. **Harita resmi çerçeveden.** Sınav müfredatı / exam guide / CEFR seviye tanımı yayınlanmıştır — web'de araştır, haritayı ORADAN kur. Tahmin haritası hedefli öğrenmede kabul edilemez.
2. **Geriye-doğru planlama + tempo bekçiliği.** `===== BUGÜN =====` bölümünden kalan süreyi hesapla. Her açılışta tek satır: "X hafta kaldı · haritanın %Y'ındayız · tempo: yetişir/riskli/yetişmez". Riskliyse dürüst söyle ve planı revize et (hangi konular kısılır, neye odaklanılır) — yargı yok, panik yok, ADHD-aware: küçük parça, net sonraki adım.
3. **Format-uyumlu pratik.** Drill hedefin gerçek formatına uyar: AWS/PMP → senaryo çoktan-seçmeli (yanlış şıkkın NEDEN cazip olduğunu tartıştır), Goethe → Schreiben metni / Lesen sorusu, iş teslimi → gerçek çıktının provası. Serbest hatırlama + format pratiği karışık gider.
4. **Ölçüm logu.** Deneme sınavı / değerlendirme sonuçlarını progress `## Hedef Durumu`na işle (`tarih | ölçüm | skor`). Zayıf alanları haritada işaretle, drill'i oraya yönelt. Ölçümsüz hedef takibi olmaz — kullanıcı hiç deneme yapmıyorsa bunu nazikçe görünür kıl.
5. **Medium sınırı dürüstlüğü.** Terminalde çalışmayan modülleri (dinleme/konuşma, lab-donanım, sunum provası) haritada `dış kaynak gerekli` işaretle ve ne önerdiğini yaz (podcast, tandem partner, gerçek lab). Sahte tamlık yasak — kapsam bekçiliği "yapamadığımı da söylerim" demektir.
6. **Hedefe ulaşınca:** kutla (gerçekten — cesur işti), sonra sor: yeni hedef mi, keşfe geçiş mi? Progress arşivlenmez, seviye kaydı olarak kalır.
```

- [ ] **Step 2: Commit**

```bash
git add USTA.md
git commit -m "USTA: hedefli öğrenme — resmi çerçeve, tempo bekçiliği, format pratiği

Keşif ve hedef aynı sistemin iki ayarı; sertifika/seviye/teslim hepsi
jenerik hedef kaydı. Medium sınırı dürüstçe haritalanır.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: SPEC v0.8 güncellemesi

**Files:**
- Modify: `SPEC.md`

- [ ] **Step 1: §4.9'dan sonra yeni bölüm ekle**

```markdown
## 4.10 Hedefli Öğrenme (v0.8)

Öğrenmenin iki modu tek sistemde: **keşif** (açık uçlu merak) ve **hedef** (sertifika/seviye/teslim — tarih + eşik). Tanışma hangisi olduğunu netleştirir.

- **Hedef kaydı jenerik:** approach `## Hedef` (ne / tarih / eşik / format), progress `## Hedef Durumu` (kalan süre / harita % / tempo / ölçüm logu). AWS SAA da Goethe B1 de aynı kalıp.
- **Harita resmi çerçeveden:** sınav müfredatı / exam guide / CEFR — web araştırmalı, tahmin değil.
- **Tempo bekçiliği:** system prompt'un `===== BUGÜN =====` bölümü sayesinde kalan süre hesaplanır; her açılışta tek satır durum, riskliyse plan revizesi.
- **Format-uyumlu drill:** senaryo çoktan-seçmeli / yazma görevi / prova — hedefin gerçek sınav formatı.
- **Medium sınırı:** terminalde çalışmayan modüller haritada `dış kaynak gerekli` olarak işaretlenir — sahte tamlık yok.
```

- [ ] **Step 2: "Alınan Kararlar" bölümüne ekle**

```markdown
- **Hedefli öğrenme (v0.8):** hedef ayrı mod değil approach alanı; tarih referansı system prompt `BUGÜN` bölümünden (`load_system_prompt` `today` parametresi aldı — model saati güvenilmez). Tempo/ölçüm progress'te yaşar, kod tarafında hedef mantığı YOK (ince kabuk korundu).
```

- [ ] **Step 3: Commit + push**

```bash
git add SPEC.md
git commit -m "SPEC: v0.8 hedefli öğrenme — keşif/hedef, tempo, resmi çerçeve

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Bitiş Doğrulaması (tüm görevler sonrası)

- [ ] `cargo test` — tamamı PASS; `cargo build` + `cargo clippy` temiz
- [ ] **Migrasyon notu rapora:** USTA.md yine değişti — gerçek kurulumda `rm ~/.config/usta/USTA.md` + bir kez `usta` gerekli (rapora açıkça yaz).
- [ ] Sandbox duman (backend varsa):
  1. `usta start almanca` → tanışmada "keşif mi, hedef mi?" sorulsun; "Goethe B1, Aralık ortası" de → `/quit` → approach'ta `## Hedef` (tarih + eşik + format), curriculum'da dört modül + Hören/Sprechen'de `dış kaynak gerekli` işareti oluşsun.
  2. Yeniden `usta start almanca` → açılışta tempo satırı ("X hafta kaldı · %Y · tempo") gelsin — tarih hesabı BUGÜN bölümünden doğru olsun.
  3. Keşif regresyonu: hedefsiz konuda (`rust`) progress'te `## Hedef Durumu` bölümü YAZILMAMIŞ olsun.
