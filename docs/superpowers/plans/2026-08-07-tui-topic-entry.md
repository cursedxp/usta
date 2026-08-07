# TUI-İçi Konu Girişi — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Spec:** `docs/superpowers/specs/2026-08-07-tui-topic-entry-design.md` — başlamadan OKU.

**Goal:** İnteraktif TTY'de Usta arayüzü konu sorusundan ÖNCE görünsün — `usta` (konusuz) çalıştırıldığında kimlik-welcome üstte scrollback'e, altta canlı girdi kutusu konuyu sorar; `usta start rust` (konu belli) tam-mod welcome + doğrudan drill.

**Architecture:** Konu-bağımlı kurulum (system prompt, Session, lock yazımı, recorder, has_progress) `build_session` yardımcısına çıkarılır (hem TUI hem plain kullanır). `tui::run` konusuz açılabilir hale gelir: topic argümanı yoksa kimlik-welcome + girdi kutusundan konu okur + slug çözer (yerel/LLM), sonra `build_session` çağırır; artefaktları (`Session`, `Recorder`, lock) `main`'e döndürür. `main` kapanışı her iki yol için paylaşımlı koşar. Plain yol birebir korunur.

**Tech Stack:** Rust, tokio, ratatui, tui-input, crossterm EventStream, mevcut backend/session/progress/index modülleri.

## Global Constraints

- Kod yorumları ve UI metinleri **Türkçe** (mevcut stil).
- **Plain yol (`ui::is_plain()`: TTY yok / `NO_COLOR`) davranışsal olarak BİREBİR korunur** — mevcut testler + `NO_COLOR` smoke kırılamaz.
- **Alternate screen YASAK** — sadece `Viewport::Inline` + `insert_before`.
- Renkler: turuncu = `Color::Indexed(208)`; DIM soluk. Değişmez.
- Genişlik hesapları **unicode-width** ile (byte/char değil).
- **Kişisel isim gömülü DEĞİL** — kimlik selamı default'ta isimsiz ("Merhaba!"); isim yalnız kullanıcının kendi profilinden gelir (`extract_name` None ise isimsiz).
- `setup()` panic hook kurar → **süreç başına TAM 1 kez** (mevcut kural korunur).
- Test komutu: `cargo test --quiet`. Clippy: `cargo clippy -- -D warnings` temiz. Commit mesajları Türkçe, `alan: özet — gerekçe` stili.
- API sürüm kayması: derleyici REDDETMEDİKÇE brief'in çağrısını AYNEN kullan; saptığında GERÇEK derleyici hatasını rapora yaz (uydurma drift gerekçesi yok).

## Dosya Haritası

| Dosya | Sorumluluk | Değişim |
|---|---|---|
| `src/main.rs` | `build_session` çıkarımı; slug yardımcıları; TUI/plain dallanması her iki yol `(Session, Recorder, PathBuf)` üretir; paylaşımlı kapanış | Task 1, 2, 4 |
| `src/tui/welcome.rs` | kimlik-mod render (`render_welcome_identity`) + paylaşılan `render_box` | Task 3 |
| `src/tui/run.rs` | `run` konusuz açılabilir: topic-entry + slug + `tui_confirm` + `build_session` + artefakt döndür | Task 4 |

---

### Task 1: `build_session` çıkarımı (saf refactor)

**Files:**
- Modify: `src/main.rs` (main() gövdesi + yeni `build_session` fn)

**Interfaces:**
- Produces:
  - `fn build_session(global: &Path, project_root: &Path, topic: &str, today: &str) -> anyhow::Result<(Session, Recorder, std::path::PathBuf, bool)>` — döner `(session, recorder, lock_path, has_progress)`; içi: `load_system_prompt` → `Session::new` → lock YAZ (`std::fs::write(lock, pid)`) → `Recorder::new` → `has_progress`.
  - `lock_path` → `pub(crate)` (Task 4'te `run` de çağıracak).
- Consumes: mevcut `brain::load_system_prompt`, `Session::new`, `transcript::Recorder::new`, `progress::progress_path`.

**Not:** Bu task DAVRANIŞI DEĞİŞTİRMEZ — konu hâlâ `main`'de `resolve_topic` ile önden çözülür, lock-çakışma onayı `main`'de kalır, `run`/`run_plain_loop` imzaları DEĞİŞMEZ. Sadece kurulum kodu bir yardımcıya toplanır. Doğrulama: tam süit yeşil.

- [ ] **Step 1: `build_session` fn'i ekle**

`src/main.rs`'e (örn. `flush_progress`'in üstüne). Lock yazımını + kurulum kodunu buraya taşı (mevcut main.rs:72,74,102-104,106-108,112-114 mantığı):

```rust
/// Konu belli olduktan sonra oturum kurulumu — system prompt + Session + kendi
/// kilidini yaz + recorder + has_progress. Lock-ÇAKIŞMASI onayı burada DEĞİL
/// (çağıran yola göre halleder: plain stdin, TUI tek-tuş). Döner:
/// `(session, recorder, lock_yolu, has_progress)`.
fn build_session(
    global: &Path,
    project_root: &Path,
    topic: &str,
    today: &str,
) -> Result<(Session, Recorder, PathBuf, bool)> {
    let system = brain::load_system_prompt(global, Some(project_root), topic, today);
    let session = Session::new(topic.to_string(), system);

    let lock = lock_path(project_root, topic);
    if let Err(e) = std::fs::write(&lock, std::process::id().to_string()) {
        ui::warn(&format!("konu kilidi yazılamadı: {e}"));
    }

    let recorder = Recorder::new(transcript::session_path(project_root, topic, &now_stamp()));

    let has_progress = std::fs::read_to_string(progress::progress_path(project_root, topic))
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);

    Ok((session, recorder, lock, has_progress))
}
```

`use` gerekiyorsa: `transcript::Recorder` zaten kullanımda; `Recorder`/`Session` yollarını mevcut import'a göre yaz (dosyada `use crate::session::Session;` var; `Recorder` için `transcript::Recorder` tam-yol veya import).

- [ ] **Step 2: `lock_path`'i `pub(crate)` yap**

`src/main.rs`'te `fn lock_path(...)` → `pub(crate) fn lock_path(...)`.

- [ ] **Step 3: `main()`'i yeniden düzenle — build_session kullan**

`main()` içinde (mevcut satır 67-114 bölgesi) şu sırayı kur: `resolve_topic` → `watcher::spawn` → `find_unfinished` uyarıları → **lock-çakışma kontrolü+onayı** (mevcut 85-101 bloğu, AMA lock YAZMA satırını sil — build_session yazacak) → `build_session` → dallanma. Örnek:

```rust
    let topic = resolve_topic(&mut backend, topic_arg).await?;
    let global = config::global_root()?;

    let mut watch_rx = watcher::spawn(&project_root)?;

    for p in transcript::find_unfinished(&project_root) {
        ui::warn(&format!("yarım oturum kaydı bulundu (flush edilememiş olabilir): {}", p.display()));
    }

    // Lock-çakışması onayı (plain/pipe) — build_session'dan ÖNCE, kendi lock'unu
    // yazmadan. (TUI yolunda bu kontrol run() içinde tui_confirm ile yapılır — Task 4.)
    let lock = lock_path(&project_root, &topic);
    if lock.exists() {
        let pid = std::fs::read_to_string(&lock).unwrap_or_default();
        if std::io::stdin().is_terminal() {
            let msg = format!(
                "Bu konuda başka bir oturum açık görünüyor (pid {}). İki oturum aynı anda \
                 kapanırsa progress birbirini EZER. Yine de devam? [e/H] ",
                pid.trim()
            );
            if !confirm(&msg, &["e", "evet"])? {
                println!("vazgeçildi — önce diğer oturumu kapat (veya kalıntıysa sil: {})", lock.display());
                return Ok(());
            }
        } else {
            ui::warn("kalıntı konu kilidi bulundu — pipe modunda devam ediliyor");
        }
    }

    let (mut session, recorder, lock, has_progress) =
        build_session(&global, &project_root, &topic, &today())?;
```

Dikkat: `lock` iki kez bağlanıyor (önce çakışma kontrolü, sonra build_session'ın döndürdüğü) — ikincisi gölgeler, kapanışta o kullanılır. Kalan `if !ui::is_plain() { tui::run::run(...) } else { ... }` bloğu ve kapanış (flush/mark_done/lock remove) **DEĞİŞMEZ** — `session`/`recorder`/`has_progress` artık build_session'dan gelir, `run`/`run_plain_loop` çağrıları aynen.

- [ ] **Step 4: Derle + tam süit + clippy**

```bash
cargo build 2>&1 | tail -3
cargo test --quiet 2>&1 | tail -3      # 126 yeşil kalmalı
cargo clippy --quiet -- -D warnings 2>&1 | tail -3
```

Beklenen: davranış değişmedi, 126 test yeşil, clippy temiz.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "main: build_session yardımcısı çıkarıldı — konu-bağımlı kurulum tek yerde (TUI konu girişi hazırlığı)"
```

---

### Task 2: Slug yardımcıları çıkarımı (SLUG_SYSTEM + finalize_slug)

**Files:**
- Modify: `src/main.rs` (`derive_slug` refactor + yeni saf yardımcı + test)

**Interfaces:**
- Produces:
  - `pub(crate) const SLUG_SYSTEM: &str` — cümleden slug çıkaran system prompt (mevcut `derive_slug` içindeki metin).
  - `pub(crate) fn finalize_slug(raw: &str, model_reply: &str) -> String` — model çıktısını slug'a çevirir; "genel"e düşerse `slugify_topic(raw)`'a fallback. Saf, test edilebilir.
  - `slugify_topic` zaten `pub`.
- Consumes: `slugify_topic`.

**Not:** `derive_slug` (async, backend çağırır) bu iki yardımcıyı kullanacak şekilde sadeleşir. TUI (Task 4) `derive_slug`'ı çağırmaz — `ask_live` + `SLUG_SYSTEM` + `finalize_slug`'ı doğrudan kullanır (TUI spinner için).

- [ ] **Step 1: Failing test yaz**

`src/main.rs` test modülüne (`mod tests`) ekle:

```rust
    #[test]
    fn finalize_slug_uses_model_reply_then_slugifies() {
        // Model tire'li slug döndürür → tireler korunur, slugify garantiler.
        assert_eq!(finalize_slug("ben golang öğrenmek istiyorum", "golang-web"), "golang-web");
        // Model gürültülü döndürürse yine slug'lanır.
        assert_eq!(finalize_slug("x", "Rust Todo"), "rust-todo");
    }

    #[test]
    fn finalize_slug_falls_back_to_raw_when_model_gives_genel() {
        // Model "genel" derse ham girdiden yerel slug türet.
        assert_eq!(finalize_slug("temel linux güvenliği", "genel"), "temel-linux-guvenligi");
    }
```

- [ ] **Step 2: FAIL doğrula**

```bash
cargo test --quiet finalize_slug 2>&1 | tail -5   # finalize_slug tanımsız
```

- [ ] **Step 3: Yardımcıları ekle + derive_slug'ı sadeleştir**

`src/main.rs`'e `derive_slug`'ın YERİNE:

```rust
/// Cümleden konu slug'ı çıkaran system prompt — hem plain (`derive_slug`) hem
/// TUI konu girişi kullanır.
pub(crate) const SLUG_SYSTEM: &str = "Kullanıcının öğrenmek/yapmak istediğini TEK kısa dosya-adı slug'ına indir. \
    Kurallar: yalnız küçük harf, ascii (Türkçe karakter yok), kelimeler tire ile ayrılır, \
    EN FAZLA 3 kelime, dolgu kelimeleri (ben/bir/ile/yapmak/istiyorum) atılır. \
    SADECE slug'ı döndür — açıklama, tırnak, noktalama yok. \
    Örnek: 'ben rust ile bir todo yapmak istiyorum' -> rust-todo";

/// Model slug cevabını nihai slug'a çevir — tireleri boşluğa çevirip `slugify_topic`
/// ile garantile; "genel"e düşerse ham girdiden yerel slug türet. Saf.
pub(crate) fn finalize_slug(raw: &str, model_reply: &str) -> String {
    let s = slugify_topic(&model_reply.trim().replace(['-', '_'], " "));
    if s == "genel" {
        slugify_topic(raw)
    } else {
        s
    }
}

/// Cümleden konu slug'ını modele çıkart (plain yol). Hata → yerel slug.
async fn derive_slug(backend: &mut Backend, raw: &str) -> String {
    let history = [Message::user(raw)];
    match ask_usta(backend, SLUG_SYSTEM, &history).await {
        Ok(reply) => finalize_slug(raw, &reply.text),
        Err(_) => slugify_topic(raw),
    }
}
```

- [ ] **Step 4: PASS + süit + clippy**

```bash
cargo test --quiet finalize_slug 2>&1 | tail -3
cargo test --quiet 2>&1 | tail -3      # 128 yeşil (126 + 2 yeni)
cargo clippy --quiet -- -D warnings 2>&1 | tail -3
```

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "main: slug yardımcıları çıkarıldı — SLUG_SYSTEM + finalize_slug (saf, test edilir); TUI konu girişi için paylaşılır"
```

---

### Task 3: Kimlik-mod welcome render

**Files:**
- Modify: `src/tui/welcome.rs` (paylaşılan `render_box` + `render_welcome_identity` + test)

**Interfaces:**
- Consumes: mevcut `fit`, `pad`, `LOGO`, `ORANGE`, border matematiği (bugün `render_welcome` içinde).
- Produces:
  - `pub fn render_welcome_identity(name: Option<&str>, model: &str, dir: &str, topics: &[String], width: u16) -> ratatui::text::Text<'static>` — konu YOK; sağ kolon "Ne öğrenmek istiyorsun?" + kayıtlı konular veya "İlk oturum — bir konu yaz."
  - (refactor) `fn render_box(version: &str, left: Vec<(String, bool)>, right: Vec<String>, width: u16) -> Text<'static>` — kutu çizimi (kenar + iki kolon + eşit-genişlik). `render_welcome` ve `render_welcome_identity` bunu paylaşır.

**Not:** `render_welcome` (tam-mod, mevcut) davranışı DEĞİŞMEZ — sadece iç gövdesi `render_box`'a delege eder. Mevcut welcome testleri (eşit-genişlik, first-session, fit) yeşil kalmalı.

- [ ] **Step 1: Failing test yaz**

`src/tui/welcome.rs` test modülüne ekle:

```rust
    #[test]
    fn render_identity_with_topics_lists_them_and_equal_width() {
        use unicode_width::UnicodeWidthStr;
        let topics = vec!["rust".to_string(), "gtm".to_string()];
        let t = render_welcome_identity(Some("Ada"), "opus · cli", "~/p", &topics, 80);
        let lines = plain_lines(&t);
        let w = lines[0].width();
        assert!(lines.iter().all(|l| l.width() == w), "hizasız: {lines:#?}");
        let joined = lines.join("\n");
        assert!(joined.contains("Ne öğrenmek istiyorsun?"));
        assert!(joined.contains("rust"));
        assert!(joined.contains("Merhaba, Ada!"));
        assert!(lines[0].starts_with('╭') && lines.last().unwrap().starts_with('╰'));
    }

    #[test]
    fn render_identity_no_topics_shows_first_session_and_no_name() {
        let t = render_welcome_identity(None, "opus · cli", "~/p", &[], 80);
        let joined = plain_lines(&t).join("\n");
        assert!(joined.contains("İlk oturum"));
        assert!(joined.contains("Merhaba!"));       // isim yok → jenerik
        assert!(!joined.contains("Merhaba,"));      // "Merhaba, X!" biçimi yok
    }
```

(`plain_lines` yardımcısı Task 3-öncesi welcome testlerinde zaten var; yoksa mevcut testteki tanımı kullan.)

- [ ] **Step 2: FAIL doğrula**

```bash
cargo test --quiet tui::welcome 2>&1 | tail -5   # render_welcome_identity tanımsız
```

- [ ] **Step 3: `render_box` çıkar + `render_welcome_identity` ekle**

`render_welcome`'ın kutu-çizen gövdesini (border top/bottom + satır döngüsü, mevcut welcome.rs ~satır 84-116 kısmı) `render_box`'a taşı. `render_welcome` sol/sağ kolonları kurup `render_box`'a delege etsin. Sonra kimlik render'ı:

```rust
/// Kimlik modu: konu YOK. Sol kolon logo + selam + model + dizin; sağ kolon
/// "Ne öğrenmek istiyorsun?" + kayıtlı konular (veya ilk-oturum mesajı).
/// Konu seçilmeden gösterilir (Claude tarzı: welcome üstte, soru altta).
pub fn render_welcome_identity(
    name: Option<&str>,
    model: &str,
    dir: &str,
    topics: &[String],
    width: u16,
) -> Text<'static> {
    let total = (width as usize).clamp(60, 100);
    let inner = total - 2;
    let left_w = 34usize;
    let right_w = inner - left_w - 3;

    let greet = match name {
        Some(n) => format!("Merhaba, {n}!"),
        None => "Merhaba!".to_string(),
    };
    let mut left: Vec<(String, bool)> = vec![(String::new(), false)];
    for l in LOGO { left.push((format!("  {l}"), true)); }
    left.push((String::new(), false));
    left.push((format!("  {}", fit(&greet, left_w - 2)), false));
    left.push((format!("  {}", fit(model, left_w - 2)), false));
    left.push((format!("  {}", fit(dir, left_w - 2)), false));

    let mut right: Vec<String> = vec!["Ne öğrenmek istiyorsun?".to_string(), String::new()];
    if topics.is_empty() {
        right.push(fit("İlk oturum — bir konu yaz.", right_w));
    } else {
        right.push(fit("Kısa yaz ya da cümleyle anlat.", right_w));
        right.push(String::new());
        let list = format!("Kayıtlı: {}", topics.join(" · "));
        right.push(fit(&list, right_w));
    }

    render_box(env!("CARGO_PKG_VERSION"), left, right, width)
}
```

`render_box` imzası ve gövdesi (mevcut render_welcome'dan çıkarılan; `version` parametresi başlık için, `right`'ın ilk dolu satırı bold-turuncu):

```rust
/// Çift kolonlu kutuyu çiz — kenar + " │ " ayracı + eşit-genişlik padding.
/// `left`: (metin, logo-mu). `right`: düz satırlar (ilk dolu satır başlık stili).
fn render_box(version: &str, left: Vec<(String, bool)>, right: Vec<String>, width: u16) -> Text<'static> {
    // ... mevcut render_welcome'ın top/bottom border + satır döngüsü buraya ...
}
```

(Mevcut `render_welcome`'daki border matematiğini — `4 + title.width()` düzeltmesi dahil — AYNEN taşı. `render_welcome` artık kendi left/right'ını kurup `render_box(env!("CARGO_PKG_VERSION"), left, right, width)` çağırır.)

- [ ] **Step 4: PASS + tam süit**

```bash
cargo test --quiet tui::welcome 2>&1 | tail -3   # eski + 2 yeni yeşil
cargo test --quiet 2>&1 | tail -3
cargo clippy --quiet -- -D warnings 2>&1 | tail -3
```

- [ ] **Step 5: Commit**

```bash
git add src/tui/welcome.rs
git commit -m "tui: kimlik-mod welcome render — render_box paylaşımı + render_welcome_identity (konusuz açılış)"
```

---

### Task 4: TUI konu girişi + `run` yeniden yapılanması + main dallanması

**Files:**
- Modify: `src/tui/run.rs` (run imzası + topic-entry + tui_confirm + build_session entegrasyonu)
- Modify: `src/main.rs` (dallanma: her iki yol `(Session, Recorder, PathBuf)` üretir)

**Interfaces:**
- Consumes: Task 1 `build_session` + `lock_path` (pub(crate)); Task 2 `SLUG_SYSTEM` + `finalize_slug` + `slugify_topic`; Task 3 `render_welcome_identity`; mevcut `render_welcome`, `ask_live`, `page*`, `index::entries`.
- Produces:
  - `run` yeni imza: `pub async fn run(backend: &mut Backend, global: &Path, project_root: &Path, today: &str, topic_arg: Option<String>, max_feedback_batch: usize, watch_rx: &mut UnboundedReceiver<PathBuf>) -> Result<Option<(Session, Recorder, PathBuf)>>` — `None` = kullanıcı konu girişinde çıktı (session yok).

**En büyük task — bilinçli tek parça:** yarım-migrasyon derlenmeyen ara commit üretmesin. Aşağıdaki uyarlama noktaları implementasyoncunun main.rs + run.rs'i OKUMASINI zorunlu kılar.

- [ ] **Step 1: `run` imzasını değiştir + session'ı içeride kur**

`run` artık `session: &mut` / `recorder` / `topic` / `has_progress` PARAMETRE ALMAZ. İçeride:
1. `let mut tui = term::setup()?;` (bir kez).
2. `let width = tui.terminal.size()?.width;`
3. Konu belirle:
   - `topic_arg` `Some(t)` → `let topic = t;` (zaten slug'lanmış — main `resolve_topic`/`slugify` uygulamaz; **DİKKAT:** aşağıda main `topic_arg`'ı slug'layıp mı geçiyor kontrol et — `parse_command` `Start(Option<String>)` ham verir; slug main'de `slugify_topic` ile uygulanmalı VEYA run içinde. Karar: `Some` dalında `slugify_topic(&t)` uygula ki `usta start "JavaScript Basics"` çalışsın).
   - `None` → **kimlik welcome + konu sor** (Step 2).
4. Konu belli olunca lock-çakışma: `let lock = crate::lock_path(project_root, &topic);` — `lock.exists()` ise `tui_confirm` (Step 3); reddedilirse `return Ok(None);` (session yok, Tui drop restore).
5. `let (mut session, recorder, _lock, has_progress) = crate::build_session(global, project_root, &topic, today)?;` (build_session lock'u yazar; dönen lock = aynı yol).
6. **Welcome:** `topic_arg` `Some` idi (konu baştan belli) → **tam-mod** `render_welcome` (öğrenme durumu) basılır. `None` idi (kimlik zaten basıldı) → tekrar basma. *(Spec kararı: tek welcome.)*
7. Drill + ana döngü (mevcut gövde) — `session`/`recorder` artık yerel.
8. Döngü bitince: son drain + `Ok(Some((session, recorder, lock)))`.

- [ ] **Step 2: `ask_topic` — girdi kutusundan konu oku**

`run.rs`'e yardımcı. Kimlik welcome'ı basar, kayıtlı konuları global index'ten okur, sonra girdi kutusundan bir satır bekler; boş Enter yutulur; Ctrl-C/D → `None`.

```rust
/// Kimlik welcome'ı basıp konuyu girdi kutusundan okur. `None` = kullanıcı
/// konu vermeden çıktı (Ctrl-C/D). Slug çözümü çağırana bırakılır.
async fn ask_topic(
    tui: &mut Tui,
    editor: &mut InputBox,
    events: &mut EventStream,
    global: &Path,
    profile: Option<&str>,
    model: &str,
    dir: &str,
    width: u16,
) -> Result<Option<String>> {
    // Kayıtlı konular (global katalog) — boşsa ilk-oturum mesajı.
    let idx = std::fs::read_to_string(global.join("learner/index.md")).unwrap_or_default();
    let topics: Vec<String> = {
        let mut t: Vec<String> = crate::index::entries(&idx).into_iter().map(|e| e.topic).collect();
        t.dedup();
        t.truncate(6);
        t
    };
    let name = profile.and_then(welcome::extract_name);
    page(tui, welcome::render_welcome_identity(name.as_deref(), model, dir, &topics, width))?;
    page_notice(tui, "Ne öğrenmek istiyorsun? (kısa yaz ya da cümleyle anlat)")?;

    loop {
        // Konu girişinde watcher olayları YOK sayılır (henüz oturum yok) — sadece tuş.
        draw(tui, editor, &Status::Idle, None, 0)?;
        if let Some(Ok(Event::Key(k))) = events.next().await {
            match editor.handle_key(k) {
                Action::Submit(line) => return Ok(Some(line)),
                Action::Exit => return Ok(None),
                Action::None => {}
            }
        }
    }
}
```

**DİKKAT:** `welcome::extract_name` `pub` mü? (Task 2 planında `extract_name` `pub`.) Değilse `pub` yap. `crate::index` erişilebilir olmalı (main modülü kardeş — `crate::index::entries`). `render_status`'a `window=0` verilince gauge çizilmez (tokens None) — sorun yok.

- [ ] **Step 3: `tui_confirm` — TUI tek-tuş onay**

```rust
/// TUI'de tek-tuş onay: mesajı bas, bir tuş bekle. `e`/`E` → true, diğer → false.
async fn tui_confirm(tui: &mut Tui, editor: &InputBox, events: &mut EventStream, msg: &str) -> Result<bool> {
    page_notice(tui, msg)?;
    loop {
        draw(tui, editor, &Status::Idle, None, 0)?;
        if let Some(Ok(Event::Key(k))) = events.next().await {
            match k.code {
                KeyCode::Char('e') | KeyCode::Char('E') => return Ok(true),
                KeyCode::Char('c') | KeyCode::Char('d') if k.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => return Ok(false),
                _ => return Ok(false),
            }
        }
    }
}
```

Kullanım (Step 1.4): `if lock.exists() { if !tui_confirm(&mut tui, &editor, &mut events, "Bu konuda başka oturum açık olabilir — progress çakışabilir. Devam? [e/H]").await? { page_notice(&mut tui, "vazgeçildi")?; return Ok(None); } }`

- [ ] **Step 4: Slug çözümü (TUI, spinner ile)**

Step 1.3 `None` dalında, `ask_topic` bir satır döndürünce:

```rust
let raw = match ask_topic(&mut tui, &mut editor, &mut events, global, /*profile*/ read(global.join("learner/profile.md")).as_deref(), &backend.label(), &short_dir(project_root), width).await? {
    Some(line) => line,
    None => return Ok(None), // konu vermeden çıktı
};
let topic = if raw.split_whitespace().count() <= 2 {
    crate::slugify_topic(&raw)
} else {
    // Cümle → LLM slug, TUI spinner (ask_live). Hata → yerel slug.
    match ask_live(&mut tui, &mut editor, &mut events, backend, crate::SLUG_SYSTEM, &[Message::user(raw.as_str())], None).await {
        Ok(reply) => crate::finalize_slug(&raw, &reply.text),
        Err(_) => crate::slugify_topic(&raw),
    }
};
page_notice(&mut tui, &format!("konu: {topic} — detayı sohbette anlatırsın"))?;
```

`read` yardımcısı (mevcut run içinde `let read = |p: PathBuf| ...` var) — profile okuması için kullan. `Message` zaten import.

- [ ] **Step 5: `main()` dallanmasını yeniden yaz**

`main()`: `resolve_topic`/`build_session`/lock-conflict artık **plain dalına** özgü. TUI dalı `topic_arg`'ı `run`'a verir, `run` her şeyi yapar. Her iki dal `(Session, Recorder, PathBuf)` üretir; kapanış paylaşımlı.

```rust
    // Ortak: backend, scaffold, global, watcher, yarım-oturum uyarıları — Task 1'den önce olduğu gibi.
    let global = config::global_root()?;
    let mut watch_rx = watcher::spawn(&project_root)?;
    for p in transcript::find_unfinished(&project_root) {
        ui::warn(&format!("yarım oturum kaydı bulundu (flush edilememiş olabilir): {}", p.display()));
    }

    let (session, recorder, lock) = if !ui::is_plain() {
        ui::set_tui_active(true);
        let r = tui::run::run(
            &mut backend, &global, &project_root, &today(),
            topic_arg, MAX_FEEDBACK_BATCH, &mut watch_rx,
        ).await;
        ui::set_tui_active(false);
        match r? {
            Some(artifacts) => artifacts,
            None => { ui::notice("Görüşürüz — suya girmeye devam et."); return Ok(()); } // konu verilmeden çıkıldı
        }
    } else {
        let topic = resolve_topic(&mut backend, topic_arg).await?;
        // Plain lock-çakışma onayı (Task 1'deki blok) — build_session'dan önce.
        let lock = lock_path(&project_root, &topic);
        if lock.exists() {
            let pid = std::fs::read_to_string(&lock).unwrap_or_default();
            if std::io::stdin().is_terminal() {
                let msg = format!("Bu konuda başka bir oturum açık görünüyor (pid {}). İki oturum aynı anda kapanırsa progress birbirini EZER. Yine de devam? [e/H] ", pid.trim());
                if !confirm(&msg, &["e", "evet"])? {
                    println!("vazgeçildi — önce diğer oturumu kapat (veya kalıntıysa sil: {})", lock.display());
                    return Ok(());
                }
            } else {
                ui::warn("kalıntı konu kilidi bulundu — pipe modunda devam ediliyor");
            }
        }
        let (mut session, recorder, lock, has_progress) =
            build_session(&global, &project_root, &topic, &today())?;
        ui::banner(&topic, &backend.label());
        run_plain_loop(&mut backend, &mut session, &recorder, &project_root, &topic, has_progress, &mut watch_rx).await?;
        (session, recorder, lock)
    };

    // Paylaşımlı kapanış — her iki yol.
    if let Err(e) = flush_progress(&mut backend, &session, &project_root).await {
        ui::warn(&format!("progress güncellenemedi: {e} — ham kayıt duruyor: {}", recorder.path().display()));
    } else if session.history().is_empty() {
    } else if let Err(e) = transcript::mark_done(recorder.path()) {
        ui::warn(&format!("oturum kaydı işaretlenemedi: {e}"));
    }
    let _ = std::fs::remove_file(&lock);
    ui::notice("Görüşürüz — suya girmeye devam et.");
    Ok(())
```

**DİKKAT noktaları (main.rs + run.rs OKU):**
- `flush_progress` `&session` alır → `session` `let (session, ...)` ile bağlı (immutable) yeterli; plain dalında `run_plain_loop` `&mut session` istiyor → o dalda `let mut session` (blok içi) kullan, tuple'ı döndür.
- `topic_arg` slug'lama: TUI dalında `run` `Some` konuyu `slugify_topic`'ler (Step 1.3). Plain dalında `resolve_topic` zaten slug'lar. Çift slug'lama zararsız (idempotent) ama TUI `Some` dalında bir kez yeterli.
- `build_session` artık yalnız plain dalında ve `run` içinde çağrılır — Task 1'de main'in gövdesindeki tek çağrı plain dalına indi.
- `run.rs` importları: `crate::{build_session, lock_path, slugify_topic, finalize_slug, SLUG_SYSTEM, index}` + `crossterm::event::KeyModifiers`. Gerçek yola göre ekle.
- `Recorder`/`Session` `run`'dan döndüğü için `run.rs` bunları `use` etmeli (zaten `Session`, `Recorder` import).

- [ ] **Step 6: Derle + tam süit + clippy + plain smoke**

```bash
cargo build 2>&1 | tail -15
cargo test --quiet 2>&1 | tail -3      # tüm testler yeşil (plain regresyon dahil)
cargo clippy --quiet -- -D warnings 2>&1 | tail -5
NO_COLOR=1 sh -c 'echo "" | cargo run --quiet -- start rust' 2>&1 | cat -v | head -8   # plain: banner + TUI YOK
```

Beklenen: build OK, testler yeşil, clippy temiz, plain akış değişmemiş.

- [ ] **Step 7: Commit**

```bash
git add src/tui/run.rs src/main.rs
git commit -m "tui: konu girişi TUI içinde — kimlik welcome + girdi kutusu soru + slug/onay; run artefakt döndürür, main dallanması"
```

---

### Task 5: Regresyon + elle doğrulama + kurulum

**Files:**
- Modify: `README.md` + `SPEC.md` (kısa not)

- [ ] **Step 1: Tam süit + plain regresyon**

```bash
cargo test --quiet 2>&1 | tail -3
cargo clippy --quiet -- -D warnings 2>&1 | tail -3
NO_COLOR=1 sh -c 'echo "" | cargo run --quiet -- start rust' 2>&1 | cat -v | head -6
```

- [ ] **Step 2: Elle doğrulama (gerçek TTY + canlı LLM, spec §Doğrulama)**

Sırayla, her biri OK olmalı:
1. `usta` → kimlik welcome üstte, altta "Ne öğrenmek istiyorsun?" + girdi kutusu. İsim: kendi profilinde isim varsa "Merhaba, X!", yoksa "Merhaba!".
2. Konu yaz ("rust") → drill başlar. Cümle yaz ("golang öğrenmek istiyorum") → spinner → "konu: golang" bildirimi → drill.
3. `usta start rust` → tam-mod welcome (öğrenme durumu) → drill, SORU YOK.
4. Konu girişinde Ctrl-C → temiz çıkış, shell sağlam, lock YOK (yazılmadı).
5. `NO_COLOR=1 usta` → eski düz akış ("Konu nedir?" rustyline), TUI açılmaz.
6. Aynı konuda ikinci oturum → lock onayı (TUI tek-tuş) çalışır.

- [ ] **Step 3: README + SPEC notu + kurulum + commit**

README "Arayüz" bölümüne 1-2 satır: konusuz `usta` TUI'de kimlik-welcome + konu sorar; `usta start <konu>` tam-mod. SPEC karar günlüğüne (v0.11) tek satır: TUI-içi konu girişi. Sonra:

```bash
cargo install --path .   # kullanıcı binary'si güncellensin
git add README.md SPEC.md
git commit -m "tui: konu girişi v1 tamam — README/SPEC arayüz notu, elle doğrulama geçti"
```

---

## Self-Review Notları

- **Spec kapsaması:** §Davranış (usta/start/plain)→Task 4; §build_session→Task 1; §WelcomeData kimlik-mod→Task 3; §slug→Task 2+4; §tui_confirm→Task 4 Step 3; §Test→her taskın adımları; §Doğrulama→Task 5. Boşluk yok.
- **Tip tutarlılığı:** `build_session -> (Session, Recorder, PathBuf, bool)` (Task 1) — Task 4 aynı tuple'ı tüketir. `run -> Result<Option<(Session, Recorder, PathBuf)>>` (Task 4) — main `match r?` ile tüketir. `finalize_slug(raw, reply) -> String` (Task 2) — Task 4 aynı imzayla çağırır. `render_welcome_identity(Option<&str>, &str, &str, &[String], u16)` (Task 3) — Task 4 aynı sırayla.
- **Task 4 en büyük** — bilinçli tek parça (yarım-migrasyon derlenmez). DİKKAT blokları main.rs/run.rs okumayı zorunlu kılar.
- **Plain regresyon riski:** Task 1 (refactor) + Task 4 (dallanma) plain yolu değiştirmez — mevcut süit + NO_COLOR smoke her iki taskta doğrulanır.
- **Kişisel isim:** kimlik selamı default'ta isimsiz (yakın commit `5e19183` seed'i jenerikleştirdi); isim yalnız kullanıcı profilinden.
