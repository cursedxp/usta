# Usta v0.5 — Arayüz Cilası Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Konuşan tarafları görsel olarak ayır (● Usta turuncu blok, ■ kullanıcı promptu), Usta yanıtlarını terminalde gerçek markdown olarak çiz (termimad), LLM beklerken spinner göster, konu sorusunu tek-kelime kuralıyla düzelt.

**Architecture:** Tüm stil tek modülde toplanır: yeni `src/ui.rs` — renk sabitleri, `is_plain()` kapısı (TTY değil veya `NO_COLOR` → düz çıktı; pipe/test bozulmaz), `print_usta_reply` (termimad render), `notice`/`warn` (soluk bildirimler), `banner`, `Spinner` (tokio task, `\r` + satır temizleme). `main.rs` çıktı üretmez, ui'yi çağırır. Davranış/prompt içeriği DEĞİŞMEZ — bu sürüm salt sunum katmanı (+ konu sorusu döngüsü).

**Tech Stack:** v0.4 sonrası yığın + `termimad = "0.30"` (yeni — terminal markdown render).

## Global Constraints

- **ÖN KOŞUL: v0.2, v0.3 VE v0.4 planları TAMAMEN uygulanmış ve commit'lenmiş olmalı** (select-loop, drill, flush, `&mut Backend` çağrı yerleri bu planın dokunduğu noktalar). Bitmemişse DUR ve bildir.
- Bu repo kendi git'ine sahip (`usta/` içinde çalış, headspace repo'suna commit atma).
- Tüm kod yorumları ve kullanıcıya görünen mesajlar **Türkçe**. Modül başları `//!` doc.
- Commit başlık deseni: `<scope>: kısa türkçe özet`.
- Her görev sonunda `cargo test` ve `cargo build` temiz (uyarı çıkarsa düzelt).
- Saf mantık test edilebilir fonksiyonda; IO kabukta. (ui.rs ağırlıkla IO kabuğudur — orada test beklenmez, duman testi yeterli.)
- termimad API'si sürüme göre küçük fark gösterebilir: plan kodu hedefi tanımlar; `set_headers_fg` vb. birebir yoksa sürümün eş değer çağrısını kullan. Hedef görsel sabit: **başlık+bold turuncu (256-renk 208), inline code yeşil (114), bildirimler soluk (dim)**.
- Commit mesajı sonuna ekle: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`

## Dosya Haritası

| Dosya | Sorumluluk | Değişim |
|---|---|---|
| `src/ui.rs` | **YENİ** — stil kapısı, markdown render, bildirimler, banner, Spinner | oluşturulur |
| `src/main.rs` | Çıktı çağrıları ui'ye taşınır, `ask_usta` sarmalayıcı, konu döngüsü, prompt `"■ "` | güncellenir |
| `Cargo.toml` | `termimad` | güncellenir |
| `SPEC.md` | v0.5 kararları | güncellenir |

Hedef görünüm:

```
● Usta — konu: rust · kod yaz, kaydet; izliyorum · /quit ile çık        (banner, soluk)

■ merhaba, todo app yapacağım                                          (kullanıcı promptu)

●                                                                      (turuncu)
  İyi seçim. Todo CLI Rust için tam boy:                               (termimad render)
  • ownership, Vec, struct doğal gelir
  • fazla büyük değil, felç yapmaz

■ _
```

---

### Task 1: `ui.rs` — stil katmanı + markdown render + bildirim taşıma

**Files:**
- Create: `src/ui.rs`
- Modify: `src/main.rs` (`mod ui;`, `print_reply` gövdesi, bildirim call-site'ları, banner, `input::spawn("■ ", ...)`)
- Modify: `Cargo.toml` (`termimad = "0.30"`)

**Interfaces:**
- Produces:
  - `ui::is_plain() -> bool` — stdout TTY değil VEYA `NO_COLOR` set → düz mod
  - `ui::print_usta_reply(reply: &str, web: bool)`
  - `ui::notice(msg: &str)` / `ui::warn(msg: &str)` — soluk bildirim (stdout/stderr)
  - `ui::banner(topic: &str)`
- Consumes: yok.
- Sözleşme: düz modda çıktı eski davranışa yakın kalır (`Usta> ...`, `(msg)`) — pipe edilen kullanım ve testler bozulmaz.

- [ ] **Step 1: Cargo.toml'a termimad ekle**

```toml
termimad = "0.30"
```

- [ ] **Step 2: `src/ui.rs`'i yaz**

```rust
//! Sunum katmanı: kim konuşuyor tek bakışta belli olsun. ● (turuncu) = Usta,
//! ■ = kullanıcı promptu, soluk `·` = sistem bildirimi. Usta yanıtları
//! termimad ile gerçek markdown olarak çizilir. `is_plain` kapısı: TTY yoksa
//! veya NO_COLOR set'liyse ANSI'siz düz çıktı — pipe/test bozulmaz.
//! Davranış burada yaşamaz — sadece görünüm.

use std::io::IsTerminal;

use termimad::MadSkin;

pub const ORANGE: &str = "\x1b[38;5;208m";
pub const DIM: &str = "\x1b[2m";
pub const RESET: &str = "\x1b[0m";

/// Düz mod: stdout TTY değil veya kullanıcı NO_COLOR istemiş.
pub fn is_plain() -> bool {
    !std::io::stdout().is_terminal() || std::env::var_os("NO_COLOR").is_some()
}

/// Usta yanıt bloğu: boş satır + turuncu ● + markdown render + boş satır.
pub fn print_usta_reply(reply: &str, web: bool) {
    if is_plain() {
        println!("Usta> {reply}");
        if web {
            println!("(🔎 web araştırıldı)");
        }
        return;
    }
    println!("\n{ORANGE}●{RESET}");
    skin().print_text(reply);
    if web {
        println!("{DIM}  🔎 web araştırıldı{RESET}");
    }
    println!();
}

/// Soluk bilgi satırı (stdout) — ana akıştan görsel olarak ayrılır.
pub fn notice(msg: &str) {
    if is_plain() {
        println!("({msg})");
    } else {
        println!("{DIM}· {msg}{RESET}");
    }
}

/// Soluk uyarı satırı (stderr).
pub fn warn(msg: &str) {
    if is_plain() {
        eprintln!("({msg})");
    } else {
        eprintln!("{DIM}! {msg}{RESET}");
    }
}

/// Oturum açılış satırı.
pub fn banner(topic: &str) {
    if is_plain() {
        println!("Usta hazır — konu: {topic}. Kod yaz, kaydet; ben izlerim. (/quit ile çık)");
        return;
    }
    println!("{ORANGE}● Usta{RESET} {DIM}— konu: {topic} · kod yaz, kaydet; izliyorum · /quit ile çık{RESET}");
}

/// Usta yanıtlarının markdown teni: başlık+bold turuncu, inline code yeşil.
/// (termimad API'si sürüme göre değişebilir — hedef renkler Global
/// Constraints'te; eş değer çağrıyı kullan.)
fn skin() -> MadSkin {
    use termimad::crossterm::style::Color;
    let mut skin = MadSkin::default();
    skin.set_headers_fg(Color::AnsiValue(208));
    skin.bold.set_fg(Color::AnsiValue(208));
    skin.inline_code.set_fg(Color::AnsiValue(114));
    skin
}
```

- [ ] **Step 3: `main.rs` call-site'larını taşı**

`mod ui;` ekle, sonra:

1. `print_reply` gövdesi:

```rust
/// Usta yanıtını sunum katmanına devret.
fn print_reply(reply: &str, web: bool) {
    ui::print_usta_reply(reply, web);
}
```

2. Girdi promptu: `input::spawn("sen> ", ready_rx)` → `input::spawn("■ ", ready_rx)`.

3. Banner: `println!("Usta hazır — konu: {topic}. ...")` satırı → `ui::banner(&topic);`

4. Bildirimleri taşı (parantezleri mesajdan at — stil katmanı ekliyor):
   - `println!("(.usta/ kuruldu)")` → `ui::notice(".usta/ kuruldu")`
   - `println!("(büyük dosya izleme dışı: {} — {len} bayt)", ...)` → `ui::notice(&format!("büyük dosya izleme dışı: {} — {len} bayt", path.display()))`
   - `eprintln!("(dosya feedback atlandı: {}: {e})", ...)` → `ui::warn(&format!("dosya feedback atlandı: {}: {e}", path.display()))`
   - `eprintln!("(hata: {e})")` → `ui::warn(&format!("hata: {e}"))`
   - `eprintln!("(açılış drilli atlandı: {e})")` → `ui::warn(&format!("açılış drilli atlandı: {e}"))`
   - `println!("(oturum özetleniyor — progress yazılıyor…)")` → `ui::notice("oturum özetleniyor — progress yazılıyor…")`
   - `println!("(progress güncellendi: {})", ...)` → `ui::notice(&format!("progress güncellendi: {}", path.display()))`
   - `eprintln!("(progress güncellenemedi: {e})")` → `ui::warn(&format!("progress güncellenemedi: {e}"))`
   - `eprintln!("(katalog güncellenemedi: {e})")` → `ui::warn(&format!("katalog güncellenemedi: {e}"))`
   - Kapanış `println!("Görüşürüz — suya girmeye devam et.")` → `ui::notice("Görüşürüz — suya girmeye devam et.")`
   - `run_init`/`run_topics`/`run_reset_*` çıktıları OLDUĞU GİBİ kalır (komut modu, sohbet değil).

- [ ] **Step 4: Test + build + duman**

Run: `cargo test && cargo build`
Expected: hepsi PASS (davranış değişmedi, sadece sunum).

Duman (backend gerekmez): `echo "" | cargo run -- start deneme` → düz mod çıktısı ANSI'siz olmalı (pipe kapısı çalışıyor).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml Cargo.lock src/ui.rs src/main.rs
git commit -m "ui: sunum katmanı — ● Usta bloğu, ■ prompt, termimad render, soluk bildirimler

Kim konuşuyor tek bakışta belli; markdown terminalde gerçekten çizilir.
TTY değilse/NO_COLOR'da düz çıktı — pipe ve testler bozulmaz.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Spinner — "Usta düşünüyor…"

**Files:**
- Modify: `src/ui.rs` (`Spinner`)
- Modify: `src/main.rs` (`ask_usta` sarmalayıcı + 4 çağrı yeri)

**Interfaces:**
- Produces:
  - `ui::Spinner::start(msg: &'static str) -> Spinner` — düz modda no-op
  - `Spinner::stop(self)` (async) — task'i durdurur, satırı temizler
  - `main::ask_usta(&mut Backend, &str, &[Message]) -> Result<(String, bool)>` (async)
- Sözleşme: LLM çağrısı sürerken tek satır animasyon; yanıt basılmadan önce satır tamamen silinir (`\r` + `\x1b[2K`).

- [ ] **Step 1: `Spinner`'ı ui.rs'e ekle**

```rust
/// LLM beklerken tek satır animasyon. Düz modda hiç çizmez.
/// `stop` çağrılınca satır silinir — yanıt temiz zemine basılır.
pub struct Spinner {
    stop_tx: Option<tokio::sync::oneshot::Sender<()>>,
    handle: Option<tokio::task::JoinHandle<()>>,
}

impl Spinner {
    pub fn start(msg: &'static str) -> Spinner {
        if is_plain() {
            return Spinner { stop_tx: None, handle: None };
        }
        let (tx, mut rx) = tokio::sync::oneshot::channel::<()>();
        let handle = tokio::spawn(async move {
            const FRAMES: [&str; 4] = ["⠋", "⠙", "⠸", "⠴"];
            let mut i = 0usize;
            loop {
                print!("\r{DIM}{} {msg}{RESET}", FRAMES[i % FRAMES.len()]);
                let _ = std::io::Write::flush(&mut std::io::stdout());
                i += 1;
                tokio::select! {
                    _ = &mut rx => break,
                    _ = tokio::time::sleep(std::time::Duration::from_millis(120)) => {}
                }
            }
            // Satırı sil — yanıt temiz zemine bassın.
            print!("\r\x1b[2K");
            let _ = std::io::Write::flush(&mut std::io::stdout());
        });
        Spinner { stop_tx: Some(tx), handle: Some(handle) }
    }

    pub async fn stop(mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(h) = self.handle.take() {
            let _ = h.await;
        }
    }
}
```

- [ ] **Step 2: `ask_usta` sarmalayıcısını ekle, çağrı yerlerini değiştir**

`src/main.rs`:

```rust
/// LLM çağrısını spinner ile sar — kullanıcı beklerken sessizlik olmasın.
async fn ask_usta(
    backend: &mut Backend,
    system: &str,
    history: &[Message],
) -> Result<(String, bool)> {
    let spinner = ui::Spinner::start("Usta düşünüyor…");
    let result = backend.complete(system, history).await;
    spinner.stop().await;
    result
}
```

DÖRT çağrı yerinde `backend.complete(...)` → `ask_usta(&mut backend, ...)` (veya fonksiyon içindeyse parametredeki `backend`):

1. Select-loop kullanıcı turn'ü
2. `handle_file_change`
3. Açılış drilli bloğu (v0.3)
4. `flush_progress`

- [ ] **Step 3: Test + build**

Run: `cargo test && cargo build`
Expected: hepsi PASS.

- [ ] **Step 4: Commit**

```bash
git add src/ui.rs src/main.rs
git commit -m "ui: spinner — LLM beklerken 'Usta düşünüyor…'

Dört çağrı yeri ask_usta sarmalayıcısından geçer; düz modda no-op.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Konu sorusu — tek kelime kuralı (TDD)

**Files:**
- Modify: `src/main.rs` (`single_token` + `resolve_topic` TTY yolu)

**Interfaces:**
- Produces: `single_token(input: &str) -> Option<String>` — girdi tek kelimeyse slug'ı, birden çok kelimeyse `None`.
- Davranış: "aklimda bir proje var" gibi cümle girilince sessizce `aklimda` konusu açılmaz — kullanıcıya tek kelime olduğu söylenir, yeniden sorulur (3 deneme; hâlâ cümleyse ilk kelime alınır ve bildirilir). Boş girdi / Ctrl-D / pipe davranışı DEĞİŞMEZ (`genel`).

- [ ] **Step 1: Failing testleri yaz**

`src/main.rs` test modülüne:

```rust
#[test]
fn single_token_accepts_one_word() {
    assert_eq!(single_token("Rust"), Some("rust".to_string()));
    assert_eq!(single_token("  C++  "), Some("c".to_string()));
}

#[test]
fn single_token_rejects_sentence() {
    assert_eq!(single_token("aklimda bir proje var"), None);
}

#[test]
fn single_token_rejects_empty() {
    assert_eq!(single_token("   "), None);
}
```

Run: `cargo test single_token`
Expected: FAIL (fonksiyon tanımsız).

- [ ] **Step 2: Implemente et**

```rust
/// Girdi tek kelimeyse konu slug'ını döndür; cümleyse `None` — konu bir
/// dosyalama anahtarıdır, cümleden sessizce ilk kelimeyi kapmak sürprizdir.
pub fn single_token(input: &str) -> Option<String> {
    let mut words = input.split_whitespace();
    let first = words.next()?;
    match words.next() {
        Some(_) => None,
        None => Some(slugify_topic(first)),
    }
}
```

`resolve_topic`'in TTY yolunu değiştir:

```rust
    let mut rl = DefaultEditor::new()?;
    let mut last = String::new();
    for attempt in 0..3 {
        match rl.readline("Tek kelimeyle konu (ör. rust, javascript): ") {
            Ok(line) => {
                let t = line.trim().to_string();
                if t.is_empty() {
                    return Ok("genel".to_string());
                }
                if let Some(slug) = single_token(&t) {
                    return Ok(slug);
                }
                last = t;
                if attempt < 2 {
                    println!("Konu tek kelime olmalı — dosyalama anahtarı bu (ör. rust).");
                }
            }
            // Ctrl-D / Ctrl-C promptta → engellemeden "genel"e düş.
            Err(_) => return Ok("genel".to_string()),
        }
    }
    // Üç denemede tek kelime gelmedi — ilk kelimeyi al, açıkça bildir.
    let slug = slugify_topic(&last);
    ui::notice(&format!("ilk kelime konu alındı: {slug}"));
    Ok(slug)
```

- [ ] **Step 3: Test + build**

Run: `cargo test && cargo build`
Expected: 3 yeni test dahil hepsi PASS.

- [ ] **Step 4: Commit**

```bash
git add src/main.rs
git commit -m "main: konu sorusu tek kelime kuralı — cümleden sessiz slug bitti

'aklimda bir proje var' → 'aklimda' konusu açılmaz; yeniden sorulur,
3 denemede ilk kelime açıkça bildirilerek alınır.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: SPEC v0.5 güncellemesi

**Files:**
- Modify: `SPEC.md`

- [ ] **Step 1: "Alınan Kararlar" bölümüne ekle**

`## 11. Alınan Kararlar` sonuna:

```markdown
- **Sunum katmanı (v0.5):** roller ikonla ayrılır — `●` (turuncu 208) Usta bloğu, `■` kullanıcı promptu, soluk `·`/`!` sistem bildirimi. Usta yanıtları termimad ile markdown render edilir; LLM beklerken spinner. TTY değilse veya `NO_COLOR` set'liyse düz çıktı (pipe/test uyumu). Davranış katmanına dokunulmadı.
- **Konu girişi (v0.5):** TTY promptunda tek kelime zorunlu — cümle girilirse yeniden sorulur (3 deneme, sonra ilk kelime açık bildirimle alınır). `usta start <konu>` ve pipe davranışı değişmedi.
```

- [ ] **Step 2: Commit + push**

```bash
git add SPEC.md
git commit -m "SPEC: v0.5 sunum katmanı kararları

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Bitiş Doğrulaması (tüm görevler sonrası)

- [ ] `cargo test` — tamamı PASS
- [ ] `cargo build` — uyarısız
- [ ] Düz mod: `echo "" | cargo run -- start deneme` → çıktıda ANSI escape YOK (`grep -c $'\x1b'` 0 olmalı)
- [ ] TTY duman (backend varsa): oturum aç → banner tek satır soluk; mesaj yaz → spinner dönsün, yanıt `●` bloğu + render'lı markdown (madde işaretleri, bold görünür); `■ ` promptu ayrışsın; konu sorusuna cümle yaz → yeniden sorsun
- [ ] `usta topics` / `reset` çıktıları değişmemiş (komut modu düz kaldı)
