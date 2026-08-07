# Usta v0.7 — Bağlam Yönetimi + Görsel Cila Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Bağlam doluluğunu her turn sonrası görsel göster (▓░ bar + k token), %70 eşiğinde otomatik ara-kayıt + kompaksiyon yap (oturum kesintisiz devam eder), banner'da kullanılan modeli göster, görünümü Claude Code hissine yaklaştır (`❯` kullanıcı promptu + Usta bloğuna padding).

**Architecture:** Her iki backend token kullanımını zaten raporluyor (API: `usage`, CLI: `--output-format json` içindeki `usage`) — `complete` dönüşü `Reply { text, web, context_tokens }` struct'ına çıkar, gösterge son çağrının toplam bağlamını 200k pencereye oranlar. Kompaksiyon mevcut parçalardan kurulur: eşik aşılınca mevcut `flush_progress` çalışır (dosyalar güncellenir), system prompt taze dosyalarla YENİDEN yüklenir, history "[ARA KAYIT]" notu + son 4 turn'e kırpılır, CLI `session_id` sıfırlanır → yeni server oturumu kompakt bağlamla açılır. Kayıp minimal çünkü önemli olan zaten yapılandırılmış dosyalarda (progress = damıtılmış oturum).

**Tech Stack:** v0.6 sonrası yığın. Yeni bağımlılık YOK (termimad zaten var).

## Global Constraints

- **ÖN KOŞUL: v0.2–v0.6 planlarının TAMAMI uygulanmış ve commit'lenmiş olmalı** (`Reply` plumbing'i `ask_usta`/flush/drill/tanışma çağrı yerlerine, kompaksiyon `flush_progress` + `brain::load_system_prompt`'a dayanır). Bitmemişse DUR ve bildir.
- Bu repo kendi git'ine sahip (`usta/` içinde çalış, headspace repo'suna commit atma).
- Tüm kod yorumları ve kullanıcıya görünen mesajlar **Türkçe**. Modül başları `//!` doc.
- Commit başlık deseni: `<scope>: kısa türkçe özet`.
- Her görev sonunda `cargo test` ve `cargo build` temiz (uyarı çıkarsa düzelt); `cargo clippy` de temiz tut.
- Test isimleri `snake_case`, davranışı cümle gibi anlatır. Mevcut testler imza değişiminde UYARLANIR, silinmez.
- Saf mantık test edilebilir fonksiyonda; IO/async kabukta.
- termimad API'si sürüme göre küçük fark gösterebilir (`terminal_size`, `skin.text`): hedef görsel sabittir — Usta bloğu 2 boşluk sol padding'li, satır sonu sarmalı terminal genişliğine göre.
- Commit mesajı sonuna ekle: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>`

## Dosya Haritası

| Dosya | Sorumluluk | Değişim |
|---|---|---|
| `src/anthropic.rs` | `sum_context_tokens` + API yanıtından usage | güncellenir |
| `src/backend.rs` | `Reply` struct, `parse_cli_output` usage'lı, `label()`, `reset_session()` | güncellenir |
| `src/session.rs` | `compact()` — history kırpma | güncellenir |
| `src/ui.rs` | `❯` prompt, padding'li render, `context_gauge`, model'li banner | güncellenir |
| `src/main.rs` | `Reply` geçişi, gösterge, `maybe_compact` tetiği | güncellenir |
| `SPEC.md` | v0.7 kararları | güncellenir |

Hedef görünüm:

```
● Usta — konu: rust · model: opus · cli · /quit ile çık

❯ ownership konusunu bitirdik mi?

●
  Hayır — haritada `ownership` **görüldü** durumunda, henüz *oturdu* değil:
  • borrow checker'la iki hata çözdün
  • lifetimes hiç görülmedi

  ▓▓░░░░░░ bağlam 41k/200k
❯ _
```

---

### Task 1: Usage plumbing — `Reply` struct + token toplama (TDD)

**Files:**
- Modify: `src/anthropic.rs` (`sum_context_tokens`, `MessageResponse.usage`, `Client::complete` dönüşü)
- Modify: `src/backend.rs` (`Reply`, `parse_cli_output` 3'lü, `complete` → `Result<Reply>`)
- Modify: `src/main.rs` (çağrı yerleri + `print_reply` imzası — görsel kısım Task 2'de)

**Interfaces:**
- Produces:
  - `anthropic::sum_context_tokens(usage: &serde_json::Value) -> Option<u64>` — `input_tokens` (zorunlu) + `cache_read_input_tokens` + `cache_creation_input_tokens` (opsiyonel) toplamı
  - `anthropic::Client::complete(...) -> Result<(String, bool, Option<u64>)>` (üçüncü eleman: bağlam token'ı)
  - `backend::Reply { pub text: String, pub web: bool, pub context_tokens: Option<u64> }`
  - `Backend::complete(&mut self, system, history) -> Result<Reply>` — TÜM çağrı yerleri uyarlanır
  - `backend::parse_cli_output(stdout: &str) -> (String, Option<String>, Option<u64>)` — v0.4 testleri 3'lü tuple'a UYARLANIR
- `main::ask_usta` → `Result<Reply>`; `print_reply(reply: &backend::Reply)` (gövdesi şimdilik `ui::print_usta_reply(&reply.text, reply.web)` — gösterge Task 2'de eklenir).

- [x] **Step 1: Failing testleri yaz**

`src/anthropic.rs` test modülüne:

```rust
#[test]
fn sum_context_tokens_adds_all_categories() {
    let usage = json!({
        "input_tokens": 1000,
        "cache_read_input_tokens": 30000,
        "cache_creation_input_tokens": 500
    });
    assert_eq!(sum_context_tokens(&usage), Some(31500));
}

#[test]
fn sum_context_tokens_works_with_only_input() {
    assert_eq!(sum_context_tokens(&json!({"input_tokens": 42})), Some(42));
}

#[test]
fn sum_context_tokens_none_without_input_tokens() {
    assert_eq!(sum_context_tokens(&json!({"output_tokens": 5})), None);
}
```

`src/backend.rs` test modülünde v0.4'ün iki `parse_cli_output` testini 3'lü tuple'a uyarla ve ekle:

```rust
#[test]
fn parse_cli_output_reads_usage_tokens() {
    let out = r#"{"result":"m","session_id":"s1","usage":{"input_tokens":100,"cache_read_input_tokens":900}}"#;
    let (_, _, tokens) = parse_cli_output(out);
    assert_eq!(tokens, Some(1000));
}

#[test]
fn parse_cli_output_tokens_none_when_usage_missing() {
    let out = r#"{"result":"m","session_id":"s1"}"#;
    let (_, _, tokens) = parse_cli_output(out);
    assert_eq!(tokens, None);
}
```

(uyarlama: `parse_cli_output_reads_json_result_and_session` → `let (text, sid, _) = ...`; `parse_cli_output_falls_back_to_plain_text` → `let (text, sid, _) = ...`.)

Run: `cargo test 'tokens'`
Expected: FAIL (fonksiyon/alanlar yok).

- [x] **Step 2: Implemente et**

`src/anthropic.rs`:

```rust
/// usage bloğundan toplam bağlam token'ı: input + cache okuma + cache yazma.
/// `input_tokens` yoksa None — gösterge sessizce atlanır.
pub fn sum_context_tokens(usage: &Value) -> Option<u64> {
    let get = |k: &str| usage.get(k).and_then(Value::as_u64);
    Some(
        get("input_tokens")?
            + get("cache_read_input_tokens").unwrap_or(0)
            + get("cache_creation_input_tokens").unwrap_or(0),
    )
}
```

`MessageResponse`'a alan ekle: `usage: Option<Value>,` — `complete` döngüsünde son yanıtın usage'ını tut, dönüşe ekle:

```rust
    // struct: #[derive(Debug, Deserialize)] struct MessageResponse { content, stop_reason, usage: Option<Value> }
    // complete imzası: Result<(String, bool, Option<u64>)>
    // pause_turn olmayan dönüş satırı:
    let tokens = parsed.usage.as_ref().and_then(sum_context_tokens);
    return Ok((extract_text(&parsed.content), web, tokens));
```

`src/backend.rs`:

```rust
/// Tek tamamlama sonucu — metin + web ipucu + bağlam doluluğu.
pub struct Reply {
    pub text: String,
    pub web: bool,
    /// Son çağrının toplam bağlam token'ı (input + cache) — gösterge için.
    pub context_tokens: Option<u64>,
}
```

`parse_cli_output` — `CliJson`'a `usage: Option<serde_json::Value>` ekle, dönüş 3'lü:

```rust
pub fn parse_cli_output(stdout: &str) -> (String, Option<String>, Option<u64>) {
    #[derive(serde::Deserialize)]
    struct CliJson {
        result: Option<String>,
        session_id: Option<String>,
        usage: Option<serde_json::Value>,
    }
    match serde_json::from_str::<CliJson>(stdout) {
        Ok(j) => {
            let tokens = j.usage.as_ref().and_then(anthropic::sum_context_tokens);
            (j.result.unwrap_or_default(), j.session_id, tokens)
        }
        Err(_) => (stdout.trim().to_string(), None, None),
    }
}
```

`run_claude_cli` dönüşü `Result<(String, Option<String>, Option<u64>)>` olur (son satır zaten `parse_cli_output`). `Backend::complete` her iki kolda `Reply` kurar:

```rust
    pub async fn complete(&mut self, system: &str, history: &[Message]) -> Result<Reply> {
        match self {
            Backend::Api { client, model } => {
                let (text, web, tokens) = client.complete(model, system, history).await?;
                Ok(Reply { text, web, context_tokens: tokens })
            }
            Backend::Cli { model, session_id } => {
                let resume = session_id.clone();
                let input = match &resume {
                    Some(_) => last_user_text(history),
                    None => render_transcript(history),
                };
                let attempt = run_claude_cli(model, system, &input, resume.as_deref()).await;
                let (text, new_sid, tokens) = match attempt {
                    Ok(v) => v,
                    Err(_) if resume.is_some() => {
                        *session_id = None;
                        run_claude_cli(model, system, &render_transcript(history), None).await?
                    }
                    Err(e) => return Err(e),
                };
                if new_sid.is_some() {
                    *session_id = new_sid;
                }
                Ok(Reply { text, web: false, context_tokens: tokens })
            }
        }
    }
```

`src/main.rs` uyarlamaları:
- `ask_usta` dönüşü `Result<backend::Reply>` (gövde aynı).
- `print_reply` imzası: `fn print_reply(reply: &backend::Reply) { ui::print_usta_reply(&reply.text, reply.web); }`
- Tüm `Ok((reply, web)) => { print_reply(&reply, web); session.push_assistant(reply); }` desenleri şuna döner: `Ok(reply) => { print_reply(&reply); session.push_assistant(reply.text); }` (select-loop, `handle_file_change`, drill, tanışma).
- `flush_progress` içindeki `let (reply, _) = ask_usta(...)` → `let reply = ask_usta(...)?;` + `progress::split_files(&reply.text)`.

- [x] **Step 3: Test + build**

Run: `cargo test && cargo build`
Expected: yeni 5 test dahil hepsi PASS.

- [x] **Step 4: Commit**

```bash
git add src/anthropic.rs src/backend.rs src/main.rs
git commit -m "backend: Reply struct — token kullanımı yüzeye çıktı

Her iki backend'den usage toplanır (input+cache); bağlam göstergesinin
veri kaynağı hazır.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 2: Görsel — model'li banner, `❯` prompt, padding, bağlam göstergesi

**Files:**
- Modify: `src/ui.rs`
- Modify: `src/backend.rs` (`label()`)
- Modify: `src/main.rs` (banner çağrısı, prompt, `print_reply`'a gösterge)

**Interfaces:**
- Produces:
  - `Backend::label(&self) -> String` — ör. `"opus · cli"` / `"claude-opus-4-8 · api"`
  - `ui::banner(topic: &str, model: &str)` — İMZA DEĞİŞİR
  - `ui::context_gauge(tokens: Option<u64>, window: u64)` — ▓░ bar; `None` veya düz modda sessiz; ≥%70 sarı
  - `main::CONTEXT_WINDOW: u64 = 200_000`
- Kullanıcı promptu `"■ "` → `"❯ "`. Usta bloğu 2 boşluk sol padding + terminal genişliğine sarma.

- [x] **Step 1: `Backend::label` ekle**

`src/backend.rs` — `impl Backend` içine:

```rust
    /// Banner'da gösterilecek model etiketi.
    pub fn label(&self) -> String {
        match self {
            Backend::Cli { model, .. } => format!("{model} · cli"),
            Backend::Api { model, .. } => format!("{model} · api"),
        }
    }
```

- [x] **Step 2: ui.rs güncellemeleri**

Sarı sabiti ekle:

```rust
pub const YELLOW: &str = "\x1b[33m";
```

`banner` imzası ve gövdesi:

```rust
/// Oturum açılış satırı — konu + model + çıkış ipucu.
pub fn banner(topic: &str, model: &str) {
    if is_plain() {
        println!("Usta hazır — konu: {topic} · model: {model}. (/quit ile çık)");
        return;
    }
    println!("{ORANGE}● Usta{RESET} {DIM}— konu: {topic} · model: {model} · /quit ile çık{RESET}");
}
```

`print_usta_reply` padding'li render (renkli kolda):

```rust
    println!("\n{ORANGE}●{RESET}");
    let width = termimad::terminal_size().0.max(40) as usize;
    let text = skin().text(reply, Some(width.saturating_sub(4)));
    for line in format!("{text}").lines() {
        println!("  {line}");
    }
    if web {
        println!("{DIM}  🔎 web araştırıldı{RESET}");
    }
    println!();
```

(`terminal_size` sürümde farklıysa eş değerini kullan — hedef: genişlik-4 sarma + 2 boşluk girinti. Düz mod kolu DEĞİŞMEZ.)

Yeni gösterge:

```rust
/// Bağlam doluluk göstergesi — 8 hücreli bar, ≥%70 sarı uyarı.
/// Token bilgisi yoksa veya düz moddaysa hiç çizilmez (gürültü yok).
pub fn context_gauge(tokens: Option<u64>, window: u64) {
    let Some(t) = tokens else { return };
    if is_plain() {
        return;
    }
    let ratio = (t as f64 / window as f64).min(1.0);
    let filled = ((ratio * 8.0).round() as usize).min(8);
    let bar = format!("{}{}", "▓".repeat(filled), "░".repeat(8 - filled));
    let color = if ratio >= 0.7 { YELLOW } else { DIM };
    println!("{color}  {bar} bağlam {}k/{}k{RESET}", t / 1000, window / 1000);
}
```

- [x] **Step 3: main.rs bağla**

```rust
/// Opus bağlam penceresi — gösterge ve kompaksiyon eşiği bu tabana oranlanır.
const CONTEXT_WINDOW: u64 = 200_000;
```

- Banner çağrısı: `ui::banner(&topic, &backend.label());` (backend banner'dan önce seçili ✓).
- Prompt: `input::spawn("❯ ", ready_rx)`.
- `print_reply` göstergeyi de basar:

```rust
fn print_reply(reply: &backend::Reply) {
    ui::print_usta_reply(&reply.text, reply.web);
    ui::context_gauge(reply.context_tokens, CONTEXT_WINDOW);
}
```

- [x] **Step 4: Test + build + duman**

Run: `cargo test && cargo build`
Expected: hepsi PASS. Duman: `echo "" | cargo run -- start deneme` → düz modda ANSI yok, gösterge yok.

- [x] **Step 5: Commit**

```bash
git add src/ui.rs src/backend.rs src/main.rs
git commit -m "ui: model'li banner, ❯ prompt, padding'li render, bağlam göstergesi

Claude Code hissi: girintili Usta bloğu + her yanıt altında ▓░ doluluk
barı (≥%70 sarı).

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 3: Otomatik ara-kayıt + kompaksiyon (TDD)

**Files:**
- Modify: `src/session.rs` (`compact`)
- Modify: `src/backend.rs` (`reset_session`)
- Modify: `src/main.rs` (`maybe_compact` + tetik)

**Interfaces:**
- Produces:
  - `Session::compact(&mut self, keep_last: usize, note: &str)` — history'yi `[note user-turn] + son keep_last mesaj`a indirir; history zaten ≤ keep_last ise dokunmaz
  - `Backend::reset_session(&mut self)` — CLI `session_id = None`, API no-op
  - `main::COMPACT_THRESHOLD: f64 = 0.70`, `main::COMPACT_KEEP_LAST: usize = 4`
- Davranış sözleşmesi: eşik aşılınca (1) ara-flush dosyaları yazar, (2) system prompt taze dosyalarla yeniden yüklenir, (3) history kırpılır, (4) CLI oturumu sıfırlanır. Kullanıcı akışı kesilmez; flush hatası kompaksiyonu İPTAL eder (veri yazılmadan history atılmaz).

- [x] **Step 1: Failing testleri yaz**

`src/session.rs` test modülüne:

```rust
#[test]
fn compact_keeps_note_plus_last_n() {
    let mut s = Session::new("rust", "sistem");
    for i in 0..10 {
        s.push_user(&format!("m{i}"));
    }
    s.compact(4, "[ARA KAYIT]");
    let h = s.history();
    assert_eq!(h.len(), 5);
    assert_eq!(h[0].content, serde_json::Value::String("[ARA KAYIT]".into()));
    assert_eq!(h[4].content, serde_json::Value::String("m9".into()));
}

#[test]
fn compact_noop_when_history_short() {
    let mut s = Session::new("rust", "sistem");
    s.push_user("tek");
    s.compact(4, "[ARA KAYIT]");
    assert_eq!(s.history().len(), 1);
}
```

Run: `cargo test compact`
Expected: FAIL.

- [x] **Step 2: Implemente et**

`src/session.rs`:

```rust
    /// Kompaksiyon: history'yi `note` + son `keep_last` mesaja indir.
    /// Ara-flush SONRASI çağrılır — atılan turn'lerin özü zaten progress/
    /// curriculum dosyalarına yazılmıştır, note bunu modele söyler.
    pub fn compact(&mut self, keep_last: usize, note: &str) {
        if self.history.len() <= keep_last {
            return;
        }
        let tail = self.history.split_off(self.history.len() - keep_last);
        self.history.clear();
        self.history.push(Message::user(note));
        self.history.extend(tail);
    }
```

`src/backend.rs` — `impl Backend` içine:

```rust
    /// CLI server oturumunu sıfırla — kompaksiyon sonrası sıradaki çağrı
    /// kompakt history ile YENİ oturum açar. API'de no-op.
    pub fn reset_session(&mut self) {
        if let Backend::Cli { session_id, .. } = self {
            *session_id = None;
        }
    }
```

`src/main.rs`:

```rust
const COMPACT_THRESHOLD: f64 = 0.70;
const COMPACT_KEEP_LAST: usize = 4;
const COMPACT_NOTE: &str = "[ARA KAYIT] Bağlam sıkıştırıldı. Önceki konuşmanın özü \
system prompt'taki progress/curriculum/approach dosyalarına yazıldı — güncel durum \
orada. Kaldığımız yerden devam et; kullanıcıya kompaksiyonu anlatma.";

/// Eşik aşıldıysa: ara-flush → system prompt'u taze dosyalarla yeniden yükle →
/// history'yi kırp → CLI oturumunu sıfırla. Flush başarısızsa kompaksiyon
/// İPTAL — veri diske inmeden history atılmaz.
async fn maybe_compact(
    backend: &mut Backend,
    session: &mut Session,
    project_root: &Path,
    tokens: Option<u64>,
) {
    let Some(t) = tokens else { return };
    if (t as f64) < COMPACT_THRESHOLD * CONTEXT_WINDOW as f64 {
        return;
    }
    if session.history().len() <= COMPACT_KEEP_LAST {
        return;
    }
    ui::notice("bağlam doluyor — ara kayıt alınıyor…");
    if let Err(e) = flush_progress(backend, session, project_root).await {
        ui::warn(&format!("ara kayıt başarısız, kompaksiyon ertelendi: {e}"));
        return;
    }
    match config::global_root() {
        Ok(global) => {
            session.system =
                brain::load_system_prompt(&global, Some(project_root), &session.topic);
        }
        Err(e) => ui::warn(&format!("system prompt yenilenemedi: {e}")),
    }
    session.compact(COMPACT_KEEP_LAST, COMPACT_NOTE);
    backend.reset_session();
    ui::notice("bağlam sıkıştırıldı — kaldığın yerden devam");
}
```

Tetik — select-loop'ta İKİ noktaya, yanıt işlendikten sonra ekle:

1. Kullanıcı turn'ünde `Ok(reply)` kolunun sonuna (push_assistant'tan sonra, `ready_tx.send`'den önce):

```rust
    maybe_compact(&mut backend, &mut session, &project_root, reply.context_tokens).await;
```

Bunun için `Ok(reply)` kolunda `session.push_assistant(reply.text)` yerine önce token'ı sakla:

```rust
    Ok(reply) => {
        print_reply(&reply);
        let tokens = reply.context_tokens;
        session.push_assistant(reply.text);
        maybe_compact(&mut backend, &mut session, &project_root, tokens).await;
    }
```

2. `handle_file_change` dönüşünde: fonksiyon `Result<Option<u64>>` döndürür (başarıda `reply.context_tokens`), select-loop'taki çağrı:

```rust
    match handle_file_change(&mut backend, &mut session, &mut files, &project_root, &path).await {
        Ok(tokens) => maybe_compact(&mut backend, &mut session, &project_root, tokens).await,
        Err(e) => ui::warn(&format!("dosya feedback atlandı: {}: {e}", path.display())),
    }
```

(`handle_file_change` son satırları: `let tokens = reply.context_tokens; print_reply(&reply); session.push_assistant(reply.text); Ok(tokens)`.)

- [x] **Step 3: Test + build**

Run: `cargo test && cargo build && cargo clippy`
Expected: yeni 2 test dahil hepsi PASS, clippy temiz.

- [x] **Step 4: Commit**

```bash
git add src/session.rs src/backend.rs src/main.rs
git commit -m "main: otomatik ara-kayıt + kompaksiyon — bağlam dolunca oturum ölmez

%70 eşiğinde flush → taze system prompt → history kırp → CLI oturum
sıfırla. Flush başarısızsa iptal: veri inmeden history atılmaz.

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

### Task 4: SPEC v0.7 güncellemesi

**Files:**
- Modify: `SPEC.md`

- [x] **Step 1: §4.8'den sonra yeni bölüm ekle**

```markdown
## 4.9 Bağlam Yönetimi (v0.7)

- **Gösterge:** her yanıt altında `▓▓░░░░░░ bağlam 41k/200k` (son çağrının input+cache toplamı / 200k pencere); ≥%70 sarı. Düz modda ve token bilgisi yoksa çizilmez.
- **Otomatik ara-kayıt + kompaksiyon:** %70 eşiğinde flush çalışır (progress/approach/curriculum diske iner), system prompt taze dosyalarla yeniden yüklenir, history `[ARA KAYIT]` notu + son 4 turn'e kırpılır, CLI `session_id` sıfırlanır. Kullanıcı akışı kesilmez. Flush başarısızsa kompaksiyon iptal — veri yazılmadan history atılmaz. Kayıp minimal: önemli olan zaten dosyalarda (progress = damıtılmış oturum).
- **Görsel:** banner model etiketi taşır (`opus · cli`), kullanıcı promptu `❯`, Usta bloğu 2 boşluk padding + genişliğe sarma.
```

- [x] **Step 2: "Alınan Kararlar" bölümüne ekle**

```markdown
- **Bağlam (v0.7):** pencere sabiti 200k (Opus); kompaksiyon eşiği %70, korunan kuyruk 4 mesaj; ölçüm = son çağrının `usage` toplamı (input + cache_read + cache_creation) — ayrı sayaç tutulmaz, kaynak API/CLI raporudur.
```

- [x] **Step 3: Commit + push**

```bash
git add SPEC.md
git commit -m "SPEC: v0.7 bağlam yönetimi — gösterge, ara-kayıt, kompaksiyon

Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>"
```

---

## Bitiş Doğrulaması (tüm görevler sonrası)

- [x] `cargo test` — tamamı PASS
- [x] `cargo build` — uyarısız, `cargo clippy` temiz
- [x] Düz mod: `echo "" | cargo run -- start deneme` → ANSI yok, gösterge yok
- [x] TTY duman (backend varsa, sandbox): banner'da `model: opus · cli` görünsün; mesaj at → yanıt altında `▓░ bağlam Xk/200k` satırı; prompt `❯ ` olsun; Usta bloğu girintili sarsın
- [x] Kompaksiyon smoke (backend varsa): `COMPACT_THRESHOLD`'u geçici `0.01` yapıp derle → ilk yanıttan sonra "ara kayıt alınıyor… / bağlam sıkıştırıldı" notları düşsün, dosyalar güncellensin, sohbet devam etsin → eşiği `0.70`'e GERİ AL, yeniden derle-commit'e dahil etme (test-only değişiklik)
