# Tasarım — Deneme Sınavı Üretici (Roadmap #7)

**Tarih:** 2026-08-15
**Kapsam:** Hedefli (GOAL modlu) konuda oturum içi `/exam` komutu: müfredat haritasından deneme sınavı, sınav boyunca ipucu/öğretim askıda, sonunda skor + kırılım; skor kapanışta `## Hedef Durumu` ölçüm günlüğüne işlenir.
**Durum:** Onaylandı (Anil: "hepsini bitir") → writing-plans
**Bağımlılık:** Yok (ölçüm günlüğü closing_prompt'ta zaten tanımlı).

## Amaç

Roadmap #7: "GOAL modunda /exam: haritadan zamanlı deneme, skor `## Hedef Durumu`na işlenir." Bugün hedefli öğrenmede (sertifika/sınav) gerçek prova mekanizması yok. Sınavın kendisi LLM işi (soru üretimi, değerlendirme); kabuk yalnız tetikler ve kapıyı tutar ("ince kabuk").

Kararlar:
- **`/exam` = prompt-enjeksiyon komutu** (statik intercept değil): kabuk tanır, hedef kontrolü yapar, `[EXAM MODE]` turunu oturuma enjekte eder — sınav sohbetin içinde akar.
- **Kapı: hedef şart.** Konunun approach dosyasında `## Hedef` yoksa notice: "no goal set for this topic — /exam needs a goal (exam/certificate); set one in the introduction" — LLM'e gitmez.
- **Zamanlama yumuşak:** kabuk süre tutmaz (v1) — model hedef formatındaki süre bütçesini söyler, soru sayısını ona göre seçer. Sert zamanlayıcı kapsam dışı.
- **Sınav yürütümü kurallı:** tek seferde TEK soru; sınav sırasında ipucu merdiveni ve öğretim ASKIDA (gerçek prova hissi); kopya-önleme yok (kullanıcı kendine sınav oluyor). Bitince: skor (hedef eşiğine göre), harita-maddesi kırılımı (hangi konular zayıf), zayıf maddeler gap adayı.
- **Skor kalıcılığı:** closing_prompt'a tek ek cümle — bu oturumda deneme yapıldıysa sonuç ölçüm günlüğüne (`date | mock exam | score`) yazılır; zayıf çıkan maddeler `## Gap'ler`e işlenir. (Ölçüm günlüğü formatı zaten var.)
- **Kural evi: GOAL.md** (embedded, yalnız hedefli konularda yüklenir — doğru yer) — `## Mock Exams` bölümü.
- **Sürüm:** iş sonunda `0.16.0` + tag.

## Davranış

### 1. Komut tanıma (`src/main.rs`)

`pub(crate) fn is_exam_command(line: &str) -> bool` — `line.trim() == "/exam"` (help.rs `is_help_command` deseni).

### 2. Hedef kontrolü (`src/main.rs`)

`pub(crate) fn topic_has_goal(project_root: &Path, global: &Path, topic: &str) -> bool` — approach dosyası (proje override öncelikli, yoksa global; `brain.rs`'teki öncelik sırasının aynısı — `progress::approach_path` + global fallback) `## Hedef` içeriyor mu.

### 3. Sınav turu (`src/progress.rs`)

`pub fn exam_prompt(topic: &str) -> String`:

```text
[EXAM MODE — MOCK EXAM]
Topic: {topic}. Build a mock exam from your curriculum map, following the exam
format defined under `## Hedef` in your approach (format, question style, time
budget, passing threshold). Weight questions toward items not yet `oturdu` and
known gaps. State the number of questions and the time budget up front. Then:
ask ONE question at a time and wait for my answer; during the exam NO hints, NO
teaching, NO feedback between questions — the hint ladder is SUSPENDED until
the exam ends. After my last answer: score against the goal's threshold, give a
short per-map-item breakdown (strong/weak), name the weak items as gap
candidates, and remind me the result is recorded at session close. If I say
'stop the exam', end it early and score what was answered.
```

### 4. Döngü entegrasyonu (TUI `src/tui/run.rs` + plain `src/main.rs`)

`/watch`-`/help` intercept'lerinin yanında: `is_exam_command` ise →
- `topic_has_goal` false → notice (yukarıdaki metin), LLM'e gitmez, `continue`.
- true → `exam_prompt(&topic)` normal kullanıcı turu gibi enjekte edilir (`session.push_user` + recorder + ask akışı — opening drill enjeksiyonunun oturum-içi muadili; TUI'de `page_user_echo` ile `/exam` yankısı basılır, LLM'e giden metin exam_prompt'tur).

### 5. Kapanış kuralı (`closing_prompt` — Hedef Durumu cümlesine ek)

"If a mock exam (`/exam`) ran this session, append its result to the measurement log (`date | mock exam | score`) and record weak items as gaps."

### 6. GOAL.md — `## Mock Exams` bölümü

Sınav yürütüm kuralları (tek soru, askıya alınan ipucu merdiveni, eşiğe göre skor, kırılım, erken bitirme, kayıt hatırlatması) + pedagojik not: deneme = en güçlü retrieval practice; sınav SONRASI zayıf maddeler normal öğretim moduna döner.

### 7. Yardım + docs

`/help` oturum-içi komut listesine `/exam` satırı (`goal mode: timed mock exam from your map`). SPEC yeni § (v0.16) · README (İngilizce, Pedagogy veya ayrı satır) · ROADMAP #7 ✅ · Cargo `0.16.0` + tag.

## Test

- `is_exam_command`: `/exam`, ` /exam ` → true; `/exam now`, `exam`, `/examx` → false.
- `exam_prompt`: `EXAM MODE`, `ONE question at a time`, `SUSPENDED`, `measurement`/`recorded at session close`, `stop the exam` içerir; `{topic}` gömülür.
- `topic_has_goal`: tmpdir — proje override'da `## Hedef` var → true; yalnız global'de var → true; ikisinde de yok → false; override hedefsiz ama global hedefli → false (öncelik: override kazanır — brain.rs GOAL yükleme semantiğiyle AYNI; koda bak, oradaki öncelik neyse onu birebir uygula ve testle kilitle).
- `closing_prompt`: `mock exam` kural cümlesini içerir.
- help metni: `/exam` satırı (mevcut help testi güncellenir).
- Döngü kolları: kod incelemesi (mevcut desen).

## Kapsam dışı

- Sert zamanlayıcı / süre aşımı algısı.
- Sınav geçmişi ayrı dosyası (ölçüm günlüğü yeter).
- Soru bankası / tekrar eden sınav kalıpları.
- Hedefsiz konuda "genel quiz" modu (drill zaten var).

## Açık sorular

Yok.
