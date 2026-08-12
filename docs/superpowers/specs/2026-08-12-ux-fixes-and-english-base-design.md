# Tasarım — UX düzeltmeleri + İngilizce ana dil

**Tarih:** 2026-08-12
**Kapsam:** Usta TUI'de dört davranış düzeltmesi + uygulamanın ana dilinin İngilizceye çevrilmesi.
**Durum:** Onay bekliyor → writing-plans

---

## Amaç

Usta kullanılabilirlik boşlukları ve dil politikası. Dört kullanıcı isteği:

1. Girdi kutusunda **yeni satır** atılamıyor — Enter anında gönderiyor.
2. LLM yanıtı **durdurulamıyor** (mevcut çift-Ctrl-C keşfedilebilir değil / yetersiz).
3. Uygulamanın **ana dili İngilizce** olmalı; kullanıcı hangi dilde yazarsa Usta o dilde cevap verir (zorunluluk yok). Arka plandaki kod yorumları da İngilizce.
4. Dosya-izleme (**companion**) proaktif feedback'i sürekli takip etmemeli — açıp kapatılabilmeli.

Karar verilmiş yönler (brainstorm çıktısı):
- Newline: modern terminalde Shift+Enter (kitty keyboard protocol) + **her terminalde Ctrl+J** evrensel fallback.
- Durdurma: **tek Esc** anında iptal; çift-Ctrl-C çıkış için kalır.
- Dil: base İngilizce, kullanıcı dilini yansıt (per-mesaj), zorunluluk yok. **Beyin dosyaları da İngilizceye çevrilir.**
- Companion: **açık başlar**, `/watch off` ile kapatılır (toggle).
- Kod yorumu çevirisi: **ayrı faz, en sona.**

## Kapsam dışı

- Plain modda (pipe/`NO_COLOR`, rustyline) tek-Esc iptal — orada canlı select döngüsü yok, çift-Ctrl-C kalır.
- Companion durumunun kalıcılaştırılması (her oturum açık başlar; progress'e yazılmaz).
- Yeni LLM özelliği, streaming, model değişikliği.

---

## Mimari bağlam

- İki oturum döngüsü var, paritede tutulmalı:
  - **TUI**: `src/tui/run.rs` — `tokio::select!` (tuş + watcher + LLM), `ask_live` canlı bekleme.
  - **Plain**: `src/main.rs::run_plain_loop` — rustyline, non-interaktif/pipe yolu.
- Girdi editörü: `src/tui/editor.rs` — `handle_key` tuşları `Action::{None,Submit,Exit}`'e çevirir; `\n` zaten desteklenir (`insert_str`, `wrap_visual` `⏎` çizer, submit tüm bloğu gönderir).
- Terminal yaşam döngüsü: `src/tui/term.rs` — raw mode + bracketed paste, `setup`/`restore`.
- Durum satırı: `src/tui/status.rs` — spinner + bağlam göstergesi.
- Watcher: `src/watcher.rs` (debounce) + `run.rs`/`main.rs` select kolları; `crate::handle_file_change` (main.rs) feedback üretir.
- Persona/davranış: `SOUL.md`, `RULES.md`, `TEACHING.md`, `GOAL.md`, `approaches/*.md` — `brain.rs` sırayla yükler, sistem promptu olur.
- Prompt üreticiler (Türkçe metin, davranışa etki eder): `progress::opening_prompt`, `progress::onboarding_prompt`, `main::slug_system`, `main::new_topic_confirm_msg`.

---

## İş birimleri

Beş bağımsız birim. 1–4 davranış; 5 mekanik çeviri (en sona).

### Birim 1 — Yeni satır tuşu (çok-satırlı girdi)

**Ne:** Enter gönderir (değişmez). Shift+Enter / Alt+Enter / Ctrl+J → `\n` ekler.

- `term.rs::setup`: `crossterm::terminal::supports_keyboard_enhancement()` true ise
  `PushKeyboardEnhancementFlags(DISAMBIGUATE_ESCAPE_CODES)` gönder; `restore`'da `PopKeyboardEnhancementFlags`. Desteklemeyen terminalde sessizce geç (bracketed paste ile aynı desen).
- `editor.rs::handle_key`:
  - Bare `Enter` (modifier yok) → mevcut Submit davranışı.
  - `Enter` + SHIFT **veya** `Enter` + ALT **veya** Ctrl+J → `self.input.handle(InsertChar('\n'))`, `Action::None`. Recall cursor sıfırlama mantığı normal edit gibi.
- **Uygulama notu:** Ctrl+J ve Shift+Enter'ın crossterm'de tam olarak hangi `KeyEvent`'e düştüğü terminale/protokole bağlı. Uygulayan, geliştirme sırasında geçici bir debug print ile bu ortamdaki gerçek `KeyEvent`'leri doğrulasın ve newline'ı hepsine bağlasın. Ctrl+J (LF, 0x0A) evrensel fallback olduğundan **en az o çalışmalı.**

**Test:** `editor.rs` unit — Shift+Enter/Ctrl+J sonrası `value()` `\n` içerir ve `Action::None`; bare Enter hâlâ `Submit`. Mevcut `insert_str`/`wrap_visual` testleri değişmez.

**Risk:** Düşük — çok-satırlı altyapı zaten var, sadece giriş tuşu ekleniyor.

### Birim 2 — Esc = anında durdur

**Ne:** Yanıt beklenirken tek Esc iptal eder.

- `run.rs::ask_live` select döngüsünde tuş kolunda: `KeyCode::Esc` → `return Ok(AskOutcome::Cancelled)`. `fut` düşer → `kill_on_drop` çocuk süreci öldürür (backend.rs mevcut davranış).
- `classify_locked_key` veya döngü içi kontrol: Esc, CancelRequest'ten ayrı ele alınır — **tek basış** yeter (çift değil).
- Çift-Ctrl-C iptal mantığı **kalır** (geri uyum + çıkış refleksi).
- `status.rs`: Thinking ipucu Esc'i de anmalı (bkz. Birim 4 İngilizce metin: örn. `(esc to stop)`).
- Kapsam: `ask_live` çağrılan tüm canlı turlar (açılış, ana döngü, slug mini-oturum).

**Test:** `run.rs` — mevcut `classify_locked_key` testleri korunur; Esc'in Cancelled ürettiğini doğrulayan saf bir yardımcı varsa test edilir (select döngüsü doğrudan test edilemez, mantık saf fonksiyona çekilebilir).

**Risk:** Düşük.

### Birim 3 — Companion toggle

**Ne:** Dosya-izleme feedback'i açıp kapatılabilir; açık başlar.

- Çalışma-zamanı bayrağı `watching: bool = true`. TUI'de `run` içinde yerel `let mut watching = true;`. Plain'de `run_plain_loop` içinde aynı.
- Slash komutları (her iki döngüde, Submit işleyicisinde `/quit`'in yanında):
  - `/watch off` → `watching = false`, notice `"companion paused — file feedback off"`.
  - `/watch on` → `watching = true`, notice `"companion on — watching files"`.
  - `/watch` (argümansız) → toggle, duruma göre notice.
  - Bu satırlar LLM'e **gönderilmez** (echo + notice, `session.push_user` yok).
- Debounce flush kolu:
  - `watching == false` iken: batch'i **sessizce senkronla** (`files.observe` ile diff baseline güncelle), `handle_file_change` **çağırma**. Böylece watch tekrar açılınca birikmiş dev diff patlamaz.
  - `watching == true` iken: mevcut davranış.
- `status.rs`: Idle durumda küçük gösterge — `👁 watching` (açık) / `watch off` (kapalı). `render_status`'a `watching: bool` parametresi eklenir; çağrılar güncellenir.

**Test:** slash ayrıştırma saf fonksiyona çekilirse (`parse_watch_command(&str) -> Option<WatchCmd>`) unit test; `status.rs` render testi watch göstergesini doğrular.

**Risk:** Orta — iki döngüde paritede tutulmalı; watch kapalıyken baseline senkronu atlanırsa açılışta feedback patlaması olur (spec'te açıkça senkron şart).

### Birim 4 — Dil: İngilizce base + kullanıcı dilini yansıt

**4a — UI/prompt stringleri (Rust) İngilizceye:**
Tüm kullanıcı-yönelik ve modele giden Türkçe sabit metinler İngilizceye. En az:
- `run.rs`: `"Ne öğrenmek istiyorsun?…"`, `"devam: {t}"`, `"konu: … detayı sohbette anlatırsın"`, `"vazgeçildi…"`, `"yanıt iptal edildi…"`, `"açılış turu iptal edildi/atlandı"`, `"toplu değişiklik…"`, `"Bu konuda başka oturum açık…"`, `"dosya feedback atlandı…"` vb.
- `status.rs`: `"Usta düşünüyor…"`, `"(iptal: tekrar Ctrl-C)"`, `"bağlam Xk/Yk"` → İngilizce; Esc ipucu eklenir.
- `welcome.rs`: hoş-geldin/öğrenme-durumu kutusu metinleri.
- `progress.rs`: `opening_prompt`, `onboarding_prompt` — modele giden talimatlar İngilizce.
- `main.rs`: `slug_system`, `new_topic_confirm_msg`, plain yol notice'ları, `handle_file_change` içi metinler.
- Türkçe substring'e bağlı **testler güncellenir** (örn. status "düşünüyor" → "thinking", "bağlam" → "context").

**4b — Davranış politikası (SOUL.md):**
`Kullanıcıyla **Türkçe** konuşursun.` satırı →
> **Operate in English by default.** Mirror the user's language: if the user writes in Turkish, reply in Turkish; if in English, reply in English. This is a soft preference, not a hard rule — follow the user's lead.

**Test:** `progress.rs` mevcut prompt testleri İngilizce assertlere güncellenir; davranış assertleri (soru sayısı, jargon yasağı, konu gömme) korunur.

**Risk:** Orta — prompt metni değişince model davranışı incelikle kayabilir; testler davranış-değişmezlerini (behavior invariants) korumalı, sadece dil string'i değişmeli.

### Birim 5 — Beyin + kod yorumu çevirisi (mekanik, en sona)

**5a — Beyin dosyaları İngilizceye:** `SOUL.md`, `RULES.md`, `TEACHING.md`, `GOAL.md`, `USTA.md`, `approaches/*.md`. İçerik birebir korunur, sadece dil. Davranış aynı kalmalı. (SOUL dil politikası satırı Birim 4b'de zaten güncellendi — çakışmasın.)
- **Not:** `USTA.md` insan haritası + `approaches` bazıları davranış içerir; çeviri anlamı değiştirmemeli. Şüpheli pedagoji ifadelerinde İngilizce karşılık birebir tutulur.
- Gömülü default'lar: `defaults.rs` bu `.md` dosyalarını `include_str!` ile paketler — yani dosyaları çevirmek gömülü şablonları da otomatik günceller, `defaults.rs`'de ayrı Türkçe string yok. `learner/index.md`, `TEACHING.md` dahil tüm include edilen `.md`'ler çevrilmeli.

**5b — Kod yorumları İngilizceye:** ~22 kaynak dosyadaki tüm Türkçe `//!`/`//` yorumları. Mekanik; kod/mantık **değişmez**. Dosya bazında subagent'lara bölünebilir.

**Doğrulama:** `grep -rE '[çğşöüıİĞŞÖÜÇ]' src` → yalnız test verisi/string literali kalmalı (yorum kalmamalı). `cargo build` + `cargo test` yeşil.

**Risk:** Düşük-orta — hacim yüksek; kod anlamını bozmamak için diff dikkatli olmalı (sadece yorum satırları).

---

## Faz sırası

1. **Faz A — Davranış (paralel subagent'lara uygun):** Birim 1 (newline), Birim 2 (Esc), Birim 3 (watch toggle), Birim 4 (dil string + SOUL politikası). Her biri izole dosya kümesi; 3 ve 4 test güncellemesi içerir.
2. **Faz B — Mekanik çeviri:** Birim 5 (beyin + yorumlar). Faz A'dan sonra ki değişen dosyalar da İngilizce yorumla teslim edilsin.

Her faz sonunda: `cargo build && cargo test` yeşil. Faz A sonunda manuel duman testi (newline, Esc, /watch, dil).

## Uygulama iş akışı (proje kuralı)

Bu repoda kod doğrudan yazılmaz: **spec → plan (writing-plans) → subagent-driven session promptu** üretilir, Anil koşturur. Bu belge onaylanınca writing-plans ile plan çıkarılır.

## Açık sorular

Yok — brainstorm'da dört karar netleşti (newline fallback, Esc, dil yansıtma, watch default) + iki kapsam kararı (beyin çevirisi evet, çeviri ayrı faz).
