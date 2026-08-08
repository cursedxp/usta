# Kullanıcı Echo + Profil Yaşam Döngüsü — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Not:** Spec: `docs/superpowers/specs/2026-08-08-user-echo-profile-reset-design.md` — başlamadan OKU. Taban: `5b16897` sonrası main. Satır numaraları kaymışsa fonksiyon adıyla bul. Bekleyen çakışan plan YOK.

**Goal:** (1) Kullanıcının gönderdiği mesajlar scrollback'te belirgin görünsün (dim değil, `❯` önekli, çok satırlı girintili, konu girişi dahil). (2) `usta reset --profile` — global profili yedekleyip gömülü jenerik şablona sıfırlar. (3) Profil yaşam döngüsü: profil boşken açılışta kısa kişisel tanışma + kapanışta profilin canlı belge olarak güncellenmesi (yalnız kişi-hakkında bilgi, konu bilgisi asla).

**Architecture:** Echo: saf `user_echo_text()` + `page_user_echo()` sarmalayıcı, mevcut DIM format satırının yerine. Reset: mevcut `ResetTarget` enum'una `Profile` varyantı + yol-parametreli saf `run_reset_profile(global)` çekirdek. Tanıma: `profile_is_generic()` tespiti → açılış promptlarına koşullu `[PROFİL BOŞ]` bloğu. Canlı profil: `closing_prompt`'a 4. dosya (`profile`) + flush'ta GLOBAL yola eşleme. Yeni bağımlılık YOK.

## Global Constraints

- Türkçe yorum/UI; mevcut stil. Test: `cargo test --quiet`. Commit: Türkçe `alan: özet — gerekçe`.
- Turuncu = `Color::Indexed(208)` (mevcut sabit dil). Kullanıcı echo metninde DIM modifier KULLANILMAZ.
- Plain yol ve pipe davranışı değişmez; mevcut 155+ test yeşil kalır.
- Gömülü jenerik profil kaynağı: `defaults::global_defaults()` içindeki `learner/profile.md` içeriği — başka kopya oluşturma.

---

### Task 1: `user_echo_text` — belirgin kullanıcı bloğu (saf + TDD)

**Files:**
- Modify: `src/tui/run.rs`

**Interfaces:**
- Produces:
  - `fn user_echo_text(line: &str) -> ratatui::text::Text<'static>` — ilk satır: turuncu `❯ ` span + NORMAL renkli metin span'ı; sonraki satırlar `"  "` girinti + normal metin; en başta bir boş satır.
  - `fn page_user_echo(tui: &mut Tui, line: &str) -> Result<()>` — `page(tui, user_echo_text(line))`.
- Consumes: mevcut `page`, `Line`, `Span`, `Style`, `Color`.

- [ ] **Step 1: Failing testler** — `src/tui/run.rs` test modülüne (yoksa mevcut modüle ekle):

```rust
    use ratatui::style::Modifier;

    fn line_text(l: &ratatui::text::Line) -> String {
        l.spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn user_echo_prefixes_first_line_and_indents_rest() {
        let t = user_echo_text("satır1\nsatır2");
        let lines: Vec<String> = t.lines.iter().map(line_text).collect();
        // [0] boş ayraç satırı, [1] ❯ + metin, [2] girintili devam.
        assert_eq!(lines[0], "");
        assert_eq!(lines[1], "❯ satır1");
        assert_eq!(lines[2], "  satır2");
    }

    #[test]
    fn user_echo_text_is_not_dim() {
        let t = user_echo_text("merhaba");
        // Hiçbir span DIM taşımaz — görünürlük sorununun kökü buydu (spec S1).
        for l in &t.lines {
            for s in &l.spans {
                assert!(!s.style.add_modifier.contains(Modifier::DIM), "DIM span: {:?}", s.content);
            }
        }
    }

    #[test]
    fn user_echo_prefix_is_orange() {
        let t = user_echo_text("x");
        let first = &t.lines[1].spans[0];
        assert_eq!(first.content.as_ref(), "❯ ");
        assert_eq!(first.style.fg, Some(ratatui::style::Color::Indexed(208)));
    }
```

- [ ] **Step 2: FAIL doğrula**

```bash
cargo test --quiet user_echo 2>&1 | tail -3   # derleme hatası: user_echo_text tanımsız
```

- [ ] **Step 3: Implementasyon** — `page_notice` yakınına:

```rust
/// Kullanıcı bloğu: boş ayraç satırı + turuncu `❯ ` önek + NORMAL renkli metin.
/// DIM KULLANMA — koyu temalarda zemine karışıp görünmez oluyordu (spec S1).
/// Çok satırlı gönderimde devam satırları 2 boşluk girintili — yapıştırma yapısı korunur.
fn user_echo_text(line: &str) -> Text<'static> {
    let mut lines: Vec<Line> = vec![Line::raw("")];
    for (i, l) in line.lines().enumerate() {
        if i == 0 {
            lines.push(Line::from(vec![
                Span::styled("❯ ", ratatui::style::Style::default().fg(ratatui::style::Color::Indexed(208))),
                Span::raw(l.to_string()),
            ]));
        } else {
            lines.push(Line::from(vec![Span::raw("  "), Span::raw(l.to_string())]));
        }
    }
    Text::from(lines)
}

/// Kullanıcının gönderdiği satırı scrollback'e bas.
fn page_user_echo(tui: &mut Tui, line: &str) -> Result<()> {
    page(tui, user_echo_text(line))
}
```

(Gerekli `use`'lar dosyada zaten var — `Line`, `Span`, `Text`; `Style`/`Color` tam yolla kullanılabilir.)

- [ ] **Step 4: Eski echo'yu değiştir + konu girişine echo ekle**

1. Ana döngü Submit'teki `page(&mut tui, ansi_to_text(&format!("\x1b[2m│ > {line}\x1b[0m")))?;` satırı → `page_user_echo(&mut tui, &line)?;`
2. Konu girişi: `run()` içinde `ask_topic` dönüşünde, `interpret_topic_input` çağrısından ÖNCE, boş olmayan girdi echo'lanır:

```rust
                if !raw.trim().is_empty() {
                    page_user_echo(&mut tui, raw.trim())?;
                }
```

(Boş Enter sentineli echo edilmez — `raw` boş string. Onay tuşları zaten bu yoldan geçmez.)

- [ ] **Step 5: Tüm testler + commit**

```bash
cargo test --quiet 2>&1 | tail -3
git add src/tui/run.rs
git commit -m "tui: kullanıcı echo'su belirgin — ❯ önekli normal renk, konu girişi dahil, dim kalktı"
```

---

### Task 2: `usta reset --profile`

**Files:**
- Modify: `src/main.rs` (`ResetTarget`, `parse_command`, komut dağıtımı, `run_reset_profile`)

**Interfaces:**
- Produces:
  - `ResetTarget::Profile` varyantı.
  - `fn reset_profile_files(global: &Path) -> Result<()>` — SAF çekirdek: mevcut `learner/profile.md`'yi `.bak`'a kopyalar, gömülü jenerik şablonu yazar. Onay İÇERMEZ.
  - `fn run_reset_profile() -> Result<()>` — onay + `global_root()` + çekirdek çağrısı + sonuç mesajı.
- Consumes: `defaults::global_defaults()` (profil içeriği buradan bulunur), mevcut `confirm()`, `config::global_root()`.

- [ ] **Step 1: Failing testler** — `src/main.rs` test modülüne:

```rust
    #[test]
    fn parse_reset_profile_flag_both_spellings() {
        let args = |s: &str| vec!["usta".to_string(), "reset".to_string(), s.to_string()];
        assert_eq!(parse_command(&args("--profile")).unwrap(), Command::Reset(ResetTarget::Profile));
        assert_eq!(parse_command(&args("--profil")).unwrap(), Command::Reset(ResetTarget::Profile));
        // Regresyon: konu ve factory aynen.
        assert_eq!(parse_command(&args("--factory")).unwrap(), Command::Reset(ResetTarget::Factory));
        assert!(matches!(parse_command(&args("rust")).unwrap(), Command::Reset(ResetTarget::Topic(t)) if t == "rust"));
    }

    #[test]
    fn reset_profile_files_backs_up_and_writes_generic_template() {
        let base = std::env::temp_dir().join(format!("usta_reset_profile_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(base.join("learner")).unwrap();
        std::fs::write(base.join("learner/profile.md"), "# Öğrenci Profili — Anil\nkişisel notlar").unwrap();

        reset_profile_files(&base).unwrap();

        let yeni = std::fs::read_to_string(base.join("learner/profile.md")).unwrap();
        let sablon = defaults::global_defaults()
            .into_iter()
            .find(|(rel, _, _)| *rel == "learner/profile.md")
            .map(|(_, c, _)| c)
            .unwrap();
        assert_eq!(yeni, sablon); // jenerik şablona eşit
        assert_eq!(
            std::fs::read_to_string(base.join("learner/profile.md.bak")).unwrap(),
            "# Öğrenci Profili — Anil\nkişisel notlar"
        ); // eski içerik yedekte
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn reset_profile_files_works_without_existing_profile() {
        let base = std::env::temp_dir().join(format!("usta_reset_profile_yok_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        reset_profile_files(&base).unwrap(); // dosya yokken de: dizin kurulur, şablon yazılır, .bak yok
        assert!(base.join("learner/profile.md").exists());
        assert!(!base.join("learner/profile.md.bak").exists());
        let _ = std::fs::remove_dir_all(&base);
    }
```

- [ ] **Step 2: FAIL doğrula**

```bash
cargo test --quiet reset_profile 2>&1 | tail -3
```

- [ ] **Step 3: Implementasyon**

`ResetTarget`'a varyant:

```rust
    /// Global kullanıcı profili — gömülü jenerik şablona döner (yedekli).
    Profile,
```

`parse_command` reset kolu:

```rust
        Some("reset") => match rest.next().map(String::as_str) {
            Some("--factory") => Ok(Command::Reset(ResetTarget::Factory)),
            Some("--profile") | Some("--profil") => Ok(Command::Reset(ResetTarget::Profile)),
            Some(topic) => Ok(Command::Reset(ResetTarget::Topic(slugify_topic(topic)))),
            None => anyhow::bail!("kullanım: usta reset <konu>  |  --factory  |  --profile"),
        },
```

Komut dağıtımı (`main()`): `Command::Reset(ResetTarget::Profile) => return run_reset_profile(),`

Çekirdek + komut:

```rust
/// Profil sıfırlama çekirdeği — SAF (onay yok, global_root yok): mevcut
/// profili `.bak`'a al, gömülü jenerik şablonu yaz. Konu progress'lerine
/// DOKUNMAZ (spec Ç2).
fn reset_profile_files(global: &Path) -> Result<()> {
    let sablon = defaults::global_defaults()
        .into_iter()
        .find(|(rel, _, _)| *rel == "learner/profile.md")
        .map(|(_, c, _)| c)
        .context("gömülü profil şablonu bulunamadı")?;
    let path = global.join("learner/profile.md");
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("dizin oluşturulamadı: {}", parent.display()))?;
    }
    if path.exists() {
        std::fs::copy(&path, path.with_extension("md.bak"))
            .with_context(|| format!("yedek alınamadı: {}", path.display()))?;
    }
    std::fs::write(&path, sablon)
        .with_context(|| format!("yazılamadı: {}", path.display()))?;
    Ok(())
}

/// `usta reset --profile` — onaylı; Usta kullanıcıyı "tanımadan" başlar.
fn run_reset_profile() -> Result<()> {
    let global = config::global_root()?;
    let path = global.join("learner/profile.md");
    if !confirm(
        &format!(
            "Profil sıfırlanacak — Usta seni tanımadan başlayacak (yedek: {}.bak). Devam? [e/H] ",
            path.display()
        ),
        &["e", "evet"],
    )? {
        println!("vazgeçildi — profil değişmedi.");
        return Ok(());
    }
    reset_profile_files(&global)?;
    println!("profil sıfırlandı: {} (eski hali .bak'ta)", path.display());
    Ok(())
}
```

NOT: `confirm()` pipe/TTY-dışı durumda nasıl davranıyor bak — mevcut kullanım TTY varsayıyor; TTY yoksa sıfırlamadan hata/uyarıyla çık (yıkıcı işlem onaysız koşmasın).

- [ ] **Step 4: Bilinmeyen-komut yardım metni** — `parse_command` sonundaki "Komutlar: …" satırına `reset` zaten yoksa ekle: `start [konu], init, topics, reset <konu>|--factory|--profile`.

- [ ] **Step 5: Tüm testler + commit**

```bash
cargo test --quiet 2>&1 | tail -3
git add src/main.rs
git commit -m "main: usta reset --profile — profil yedekli jenerik şablona döner"
```

---

### Task 3: `profile_is_generic` + açılışta kişisel tanışma (Ç3a)

**Files:**
- Modify: `src/main.rs` (veya `src/brain.rs` — tespit fonksiyonu; profili zaten okuyan yere yakın koy)
- Modify: `src/progress.rs` (`onboarding_prompt` + `opening_prompt` imzaları)
- Modify: `src/tui/run.rs`, `src/main.rs` (çağrı yerleri)

**Interfaces:**
- Produces:
  - `pub(crate) fn profile_is_generic(disk: &str) -> bool` — gömülü `learner/profile.md` şablonuyla (defaults) birebir eşitlik (trim'li karşılaştır: satır sonu farkı yüzünden yanlış-negatif olmasın).
  - `onboarding_prompt(topic, intro, profile_generic: bool)` ve `opening_prompt(topic, profile_generic: bool)` — `true` iken `[PROFİL BOŞ]` bloğu eklenir.
- Consumes: `defaults::global_defaults()`.

- [ ] **Step 1: Failing testler**

`profile_is_generic` (main.rs test modülü):

```rust
    #[test]
    fn profile_is_generic_matches_embedded_template_only() {
        let sablon = defaults::global_defaults()
            .into_iter()
            .find(|(rel, _, _)| *rel == "learner/profile.md")
            .map(|(_, c, _)| c)
            .unwrap();
        assert!(profile_is_generic(sablon));
        assert!(profile_is_generic(&format!("{sablon}\n"))); // satır sonu toleransı
        assert!(!profile_is_generic("# Öğrenci Profili — Anil\nkişisel"));
    }
```

`progress.rs` test modülüne:

```rust
    #[test]
    fn opening_prompts_include_meet_block_only_when_profile_generic() {
        let on = onboarding_prompt("rust", None, true);
        assert!(on.contains("[PROFİL BOŞ]"));
        assert!(on.contains("1-2 soru"));
        assert!(!onboarding_prompt("rust", None, false).contains("[PROFİL BOŞ]"));

        let op = opening_prompt("rust", true);
        assert!(op.contains("[PROFİL BOŞ]"));
        assert!(!opening_prompt("rust", false).contains("[PROFİL BOŞ]"));
    }
```

Mevcut testlerdeki çağrılar yeni parametreyle güncellenir (`false` geç).

- [ ] **Step 2: FAIL doğrula**

```bash
cargo test --quiet profile_is_generic 2>&1 | tail -3
cargo test --quiet meet_block 2>&1 | tail -3
```

- [ ] **Step 3: Implementasyon**

`profile_is_generic`:

```rust
/// Profil hâlâ gömülü jenerik şablon mu? (= Usta kullanıcıyı henüz tanımıyor.)
/// Trim'li karşılaştırma — satır sonu/boşluk farkı yanlış-negatif üretmesin.
pub(crate) fn profile_is_generic(disk: &str) -> bool {
    defaults::global_defaults()
        .into_iter()
        .find(|(rel, _, _)| *rel == "learner/profile.md")
        .map(|(_, c, _)| c.trim() == disk.trim())
        .unwrap_or(false)
}
```

Prompt bloğu (iki fonksiyonda ortak — `progress.rs` içinde sabit):

```rust
/// Profil boşken açılış turn'lerine eklenen tanışma talimatı (spec Ç3a).
const MEET_BLOCK: &str = "\n[PROFİL BOŞ] Kullanıcıyı henüz tanımıyorsun. Sohbetin \
başında kısaca tanış — adı, bu alanla geçmişi, nasıl öğrenmeyi sevdiği. En fazla \
1-2 soru, form değil; konuya girmeyi geciktirme. Kullanıcı kendini zaten anlattıysa \
tekrar sorma. Öğrendiklerin oturum kapanışında profiline yazılacak.\n";
```

İmzalar: `onboarding_prompt(topic, intro, profile_generic: bool)`, `opening_prompt(topic, profile_generic: bool)` — `profile_generic` true ise `MEET_BLOCK` format çıktısına eklenir (onboarding'de intro bloğundan sonra; opening'de drill talimatından önce).

Çağrı yerleri: TUI `run()` — açılışta `read(global.join("learner/profile.md"))` zaten okunuyor; `let profile_generic = read(...).as_deref().map(crate::profile_is_generic).unwrap_or(true);` (dosya yoksa da tanışılır) → iki prompt çağrısına geçir. Plain `run_plain_loop` — aynı hesap `main()`'de yapılır, `run_plain_loop`'a `profile_generic: bool` parametresi eklenir.

- [ ] **Step 4: Tüm testler + commit**

```bash
cargo test --quiet 2>&1 | tail -3
git add src/main.rs src/progress.rs src/tui/run.rs
git commit -m "usta: profil boşken açılışta kısa kişisel tanışma — profile_is_generic tespiti"
```

---

### Task 4: Kapanışta profil canlı belge (Ç3b)

**Files:**
- Modify: `src/progress.rs` (`closing_prompt` imzası + kurallar)
- Modify: `src/main.rs` (`flush_progress` — `profile` adının GLOBAL yola eşlenmesi)

**Interfaces:**
- Produces:
  - `closing_prompt(topic, progress, approach, curriculum, profile: Option<&str>)` — profile bölümü + mevcut profil içeriği.
  - `flush_progress` içinde `"profile"` dosya adı → `config::global_root()?.join("learner/profile.md")` hedefi, `write_atomic` ile.
- Consumes: mevcut `split_files` (ad-bazlı — değişiklik gerekmez), `write_atomic`.

- [ ] **Step 1: Failing testler** (`progress.rs`)

```rust
    #[test]
    fn closing_prompt_defines_profile_rules() {
        let s = closing_prompt("rust", None, None, None, Some("MEVCUT PROFİL"));
        assert!(s.contains("===DOSYA: profile==="));
        assert!(s.contains("MEVCUT PROFİL"));
        assert!(s.contains("KONU BİLGİSİ YAZILMAZ"));
        assert!(s.contains("yalnız")); // yalnız yeni/değişen bilgi varsa üretilir
    }
```

Mevcut `closing_prompt` testleri yeni parametreyle güncellenir (`None` geç). `flush_progress` için: mevcut flush testleri varsa incele; yoksa `split_files(["profile"])` çıktısının global yola yazıldığını doğrulayan, temp global+proje kökleriyle çalışan bir test ekle (flush'ın dosya-yazma kısmı ayrı yardımcıya çıkarılabilir: `fn flush_target(name: &str, project_root: &Path, global: &Path, topic: &str) -> Option<PathBuf>` — SAF, testlenebilir; bilinmeyen ad → `None`).

- [ ] **Step 2: FAIL → implementasyon**

`closing_prompt` kural bloğuna eklenecek madde (mevcut Kurallar listesine):

```text
- `profile` YALNIZ kullanıcı hakkında bu oturumda yeni/değişen kalıcı bilgi
  öğrenildiyse üretilir: ad, geçmiş/deneyim, öğrenme tarzı, tercihler,
  tekrarlayan güçlü/zayıf yönler. KONU BİLGİSİ YAZILMAZ — 'X kavramını öğrendi'
  progress'in işidir; 'örnek üzerinden öğrenmeyi sever' profile girer. Mevcut
  profildeki geçerli bilgiyi KORU (kullanıcı elle düzenlemiş olabilir), ~1 sayfa
  tavan, yinelenenleri birleştir. Değişiklik yoksa bu dosyayı HİÇ üretme.
```

Ve prompt gövdesine `Mevcut profil:\n---\n{p}\n---` bloğu (diğer üç dosyayla aynı kalıp).

`flush_progress`: mevcut ad-eşleme match'ine (`"progress" | "approach" | "curriculum"` benzeri yapı) `"profile"` kolu eklenir — hedef global yol; `config::global_root()` hatası uyarıyla atlanır (flush'ın geri kalanı yaşar). Çağrı yerinde mevcut profil içeriği okunup `closing_prompt`'a geçirilir.

- [ ] **Step 3: Tüm testler + commit**

```bash
cargo test --quiet 2>&1 | tail -3
git add src/progress.rs src/main.rs
git commit -m "usta: profil canlı belge — kapanışta kişi-hakkında bilgi profile işlenir (konu bilgisi yasak)"
```

---

### Task 5: README + elle doğrulama + kurulum

**Files:**
- Modify: `README.md` (reset bölümüne `--profile`; profil yaşam döngüsü 2-3 satır)

- [ ] **Step 1: README güncelle + tüm testler**

```bash
cargo test --quiet 2>&1 | tail -3
```

- [ ] **Step 2: Elle doğrulama (spec §5, gerçek terminalde)**

1. `usta` → mesaj yaz-gönder → metnin `❯` önekiyle NET görünüyor (dim değil).
2. Çok satırlı yapıştır-gönder → tüm satırlar girintili görünüyor.
3. Konu girişinde cümle yaz → cümlen echo'lanıyor, sonra "konu: …" notice'ı.
4. `usta reset --profile` → `e` → profil jenerik, `.bak` eski hali.
5. `usta reset --profile` → `h` → dosyalar değişmedi.
6. `usta reset <konu>` ve `--factory` regresyon.
7. Reset sonrası yeni oturum → Usta konuya girmeden kısaca tanışıyor (1-2 soru); cevap ver → `/quit` → `~/.config/usta/learner/profile.md` dolmuş, KONU bilgisi yok.
8. Dolu profille yeni oturum → tanışma sorusu YOK, isimle/tarza göre davranıyor.
9. Profili elle düzenle → sonraki kapanış elle yazılanı ezmiyor.

- [ ] **Step 3: Kurulum + commit**

```bash
cargo install --path .
git add -A
git commit -m "tui+main: kullanıcı echo + profil yaşam döngüsü v1 tamam — elle doğrulama geçti"
```

---

## Self-Review Notları

- **Spec kapsaması:** S1/Ç1→Task 1 (stil + konu girişi + çok satır), S2/Ç2→Task 2 (parse + çekirdek + onay + yedek), S3/Ç3a→Task 3 (tespit + açılış blokları), Ç3b→Task 4 (kapanış 4. dosya + global eşleme), elle doğrulama→Task 5. Kapsam dışılar (onay echo'su, seçici unutma, konu-üstü köprü) hiçbir görevde yok — doğru.
- **Görev sırası bilinçli:** Task 2 (reset) Task 3'ün tetiğiyle (jenerik tespiti) aynı gömülü şablonu kullanır — Task 3 testi Task 2'den bağımsız ama aynı `defaults` deseni; sıra 2→3→4 bağımlılığı doğal.
- **İmza zinciri:** `onboarding_prompt` bu planda İKİ kez genişler (Task 3: `profile_generic`) — intro parametresi zaten main'de (`5b16897`); Task 3 çağrı güncellemeleri derleyici rehberliğinde tamamlanır.
- **Güvenlik:** yıkıcı işlem (profil silme) onaylı + yedekli; TTY-dışı onaysız koşmaz (Task 2 NOT); kapanış profili `write_atomic` + `.bak` ile yazar, elle düzenleme koruma kuralı prompt'ta.
- **Tip tutarlılığı:** `defaults::global_defaults()` üçlü tuple `(rel, content, ownership)` — test, çekirdek ve tespit aynı desen; `flush_target` saf yardımcısı bilinmeyen adda `None` (mevcut güvenlik korunur).
