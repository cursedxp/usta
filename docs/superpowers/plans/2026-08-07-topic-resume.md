# Konu Devamlılığı — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **Not:** Spec: `docs/superpowers/specs/2026-08-07-topic-resume-design.md` — başlamadan OKU. Taban: `969128d` sonrası main (bracketed paste + tui-fixes merged). Satır numaraları kaymışsa fonksiyon adıyla bul.

**Goal:** Bare `usta` açılışında kaldığın konuya dönüş: boş Enter = son konu, rakam = listeden seç, ad/niyet eşleşmesi = devam; LLM slug'ı mevcut konulara farkında. "Hafıza boş" senaryosu ölür.

**Architecture:** İki katman — saf `interpret_topic_input` (deterministik kurallar) + `slug_system(known)` (LLM güvenlik ağı). Konu listesi proje-yerel progress dosyalarından; global katalog sadece sıralama tarihi + "diğer projeler" bilgisi. Devam seçilince mevcut `build_session`/drill mekanizması aynen çalışır — değişen tek şey konu SEÇİMİ.

**Tech Stack:** Mevcut. Yeni bağımlılık YOK.

## Global Constraints

- Kod yorumları/UI Türkçe; mevcut stil. Test: `cargo test --quiet`. Commit: Türkçe `alan: özet — gerekçe`.
- Plain yol pipe/boş-stdin davranışı değişmez (`genel` fallback aynen). Mevcut testler yeşil kalır.
- Proje-yerel konu = `.usta/learner/progress/<slug>.md` (boş olmayan). "Son konu" = global index tarihi yeniye göre; index kaydı yoksa dosya mtime.

## Dosya Haritası

| Dosya | Değişiklik |
|---|---|
| `src/index.rs` | `local_topics()` eklenir |
| `src/main.rs` | `TopicChoice`, `interpret_topic_input()`, `slug_system()`, `resolve_topic` güncellenir |
| `src/tui/welcome.rs` | `render_welcome_identity` imzası: yerel + diğer-projeler listeleri, Enter-devam satırı |
| `src/tui/run.rs` | `ask_topic` boş-Enter sentineli + liste dışarıdan; `run()` seçim akışı |

---

### Task 1: `index::local_topics` — proje-yerel konular, yeniden-eskiye

**Files:**
- Modify: `src/index.rs`

**Interfaces:**
- Produces: `pub fn local_topics(project_root: &Path, index_content: &str) -> Vec<String>` — progress dosyası olan konular, son-çalışılan önce. `[0]` = "son konu".
- Consumes: mevcut `entries()`.

- [ ] **Step 1: Failing testler**

`src/index.rs` test modülüne:

```rust
    #[test]
    fn local_topics_lists_progress_stems_sorted_by_index_date_desc() {
        let base = std::env::temp_dir().join(format!("usta_localtopics_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let pdir = base.join(".usta/learner/progress");
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(pdir.join("eski-konu.md"), "içerik").unwrap();
        std::fs::write(pdir.join("yeni-konu.md"), "içerik").unwrap();
        std::fs::write(pdir.join("bos.md"), "  ").unwrap(); // boş → listelenmez
        let index = format!(
            "## Kayıtlar\n- eski-konu | {p} | 2026-08-01\n- yeni-konu | {p} | 2026-08-07\n- baska-proje-konu | /tmp/baska | 2026-08-06\n",
            p = base.display()
        );
        let t = local_topics(&base, &index);
        assert_eq!(t, vec!["yeni-konu".to_string(), "eski-konu".to_string()]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn local_topics_without_index_entry_still_lists_by_mtime() {
        let base = std::env::temp_dir().join(format!("usta_localtopics_mtime_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let pdir = base.join(".usta/learner/progress");
        std::fs::create_dir_all(&pdir).unwrap();
        std::fs::write(pdir.join("tek-konu.md"), "içerik").unwrap();
        let t = local_topics(&base, ""); // index boş (factory reset senaryosu)
        assert_eq!(t, vec!["tek-konu".to_string()]);
        let _ = std::fs::remove_dir_all(&base);
    }
```

- [ ] **Step 2: FAIL doğrula**

```bash
cargo test --quiet local_topics 2>&1 | tail -3   # derleme hatası: local_topics tanımsız
```

- [ ] **Step 3: Implementasyon**

```rust
/// Bu projede devam edilebilir konular: `.usta/learner/progress/*.md`
/// (boş olmayan) dosya adları. Sıralama: global index tarihi yeniden-eskiye;
/// index kaydı olmayan konu dosya mtime'ına düşer (factory reset sonrası
/// katalog boş olabilir — progress hâlâ gerçek kaynak). `[0]` = son konu.
pub fn local_topics(project_root: &Path, index_content: &str) -> Vec<String> {
    let dir = project_root.join(".usta/learner/progress");
    let Ok(rd) = std::fs::read_dir(&dir) else { return Vec::new() };
    let idx = entries(index_content);
    let date_of = |topic: &str| -> Option<String> {
        idx.iter()
            .find(|e| e.topic == topic && e.project == project_root)
            .map(|e| e.date.clone())
    };
    let mut out: Vec<(String, String)> = rd
        .flatten()
        .filter_map(|f| {
            let p = f.path();
            if p.extension().and_then(|e| e.to_str()) != Some("md") { return None; }
            let stem = p.file_stem()?.to_str()?.to_string();
            let content = std::fs::read_to_string(&p).ok()?;
            if content.trim().is_empty() { return None; }
            // Sıralama anahtarı: index tarihi (YYYY-MM-DD sıralanabilir);
            // yoksa mtime'dan üretilmiş kaba anahtar (epoch saniye, sabit genişlik).
            let key = date_of(&stem).unwrap_or_else(|| {
                let secs = f.metadata()
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                format!("0000-epoch-{secs:020}")
            });
            Some((key, stem))
        })
        .collect();
    out.sort_by(|a, b| b.0.cmp(&a.0)); // yeniden-eskiye
    out.into_iter().map(|(_, t)| t).collect()
}
```

- [ ] **Step 4: PASS + commit**

```bash
cargo test --quiet local_topics 2>&1 | tail -3 && cargo test --quiet 2>&1 | tail -3
git add src/index.rs
git commit -m "index: local_topics — proje-yerel devam edilebilir konular, son-çalışılan önce"
```

---

### Task 2: `TopicChoice` + `interpret_topic_input` (saf kurallar K1)

**Files:**
- Modify: `src/main.rs` (slug yardımcılarının yanına)

**Interfaces:**
- Produces:
  - `pub(crate) enum TopicChoice { Resume(String), New(String) }`
  - `pub(crate) fn interpret_topic_input(raw: &str, local: &[String]) -> Option<TopicChoice>` — `None` = yut (boş girdi + konu yok).
- Consumes: `slugify_topic`, `deasciify` (mevcut).

- [ ] **Step 1: Failing testler**

`src/main.rs` test modülüne:

```rust
    #[test]
    fn interpret_empty_resumes_latest_or_swallows() {
        let local = vec!["son-konu".to_string(), "eski".to_string()];
        assert!(matches!(interpret_topic_input("", &local), Some(TopicChoice::Resume(t)) if t == "son-konu"));
        assert!(interpret_topic_input("  ", &[]).is_none()); // konu yok → yut
    }

    #[test]
    fn interpret_digit_selects_from_list_out_of_range_is_new() {
        let local = vec!["a".to_string(), "b".to_string()];
        assert!(matches!(interpret_topic_input("2", &local), Some(TopicChoice::Resume(t)) if t == "b"));
        assert!(matches!(interpret_topic_input("5", &local), Some(TopicChoice::New(r)) if r == "5"));
    }

    #[test]
    fn interpret_existing_slug_match_resumes() {
        let local = vec!["linux-guvenlik".to_string()];
        // Slugify eşleşmesi: Türkçe yazım da yakalanır.
        assert!(matches!(
            interpret_topic_input("Linux Güvenlik", &local),
            Some(TopicChoice::Resume(t)) if t == "linux-guvenlik"
        ));
    }

    #[test]
    fn interpret_resume_phrases_short_input_only() {
        let local = vec!["son-konu".to_string()];
        for s in ["devam", "devam edelim", "kaldığımız yerden devam", "continue", "resume"] {
            assert!(matches!(interpret_topic_input(s, &local), Some(TopicChoice::Resume(t)) if t == "son-konu"), "{s}");
        }
        // >4 kelime → LLM'e/yeni akışa (K2 yakalar).
        assert!(matches!(
            interpret_topic_input("devam edelim ama bu sefer docker öğrenelim", &local),
            Some(TopicChoice::New(_))
        ));
        // Devam kalıbı ama hiç konu yok → yeni konu.
        assert!(matches!(interpret_topic_input("devam", &[]), Some(TopicChoice::New(_))));
    }

    #[test]
    fn interpret_other_input_is_new() {
        let local = vec!["son-konu".to_string()];
        assert!(matches!(interpret_topic_input("docker compose", &local), Some(TopicChoice::New(r)) if r == "docker compose"));
    }
```

- [ ] **Step 2: FAIL doğrula**

```bash
cargo test --quiet interpret_ 2>&1 | tail -3
```

- [ ] **Step 3: Implementasyon**

```rust
/// Konu girişi yorumu: devam mı, yeni konu mu? (spec K1)
#[derive(Debug)]
pub(crate) enum TopicChoice {
    /// Mevcut proje-yerel konuya devam.
    Resume(String),
    /// Yeni konu akışı — ham girdi (slug'lama çağıranda).
    New(String),
}

/// Deterministik seçim kuralları — sıra spec §3/K1 tablosu. `None` = girdiyi
/// yut (boş + devam edilecek konu yok). LLM'siz; cümleler `New` döner, K2
/// (slug_system) orada devreye girer.
pub(crate) fn interpret_topic_input(raw: &str, local: &[String]) -> Option<TopicChoice> {
    let raw = raw.trim();
    // 1-2: boş Enter.
    if raw.is_empty() {
        return local.first().map(|t| TopicChoice::Resume(t.clone()));
    }
    // 3: rakam seçimi.
    if let Ok(n) = raw.parse::<usize>() {
        if n >= 1 && n <= local.len() {
            return Some(TopicChoice::Resume(local[n - 1].clone()));
        }
    }
    // 4: slug eşleşmesi.
    let slug = slugify_topic(raw);
    if let Some(t) = local.iter().find(|t| **t == slug) {
        return Some(TopicChoice::Resume(t.clone()));
    }
    // 5: kısa devam-kalıbı (deasciify sonrası substring).
    if !local.is_empty() && raw.split_whitespace().count() <= 4 {
        let d: String = raw.chars().map(deasciify).collect::<String>().to_lowercase();
        const RESUME_WORDS: &[&str] = &["devam", "kaldigimiz", "kaldigim", "continue", "resume"];
        if RESUME_WORDS.iter().any(|w| d.contains(w)) {
            return Some(TopicChoice::Resume(local[0].clone()));
        }
    }
    // 6: yeni konu.
    Some(TopicChoice::New(raw.to_string()))
}
```

NOT: `deasciify` mevcut fonksiyonun imzasına bak (char→char) — çağrıyı ona uydur.

- [ ] **Step 4: PASS + commit**

```bash
cargo test --quiet 2>&1 | tail -3
git add src/main.rs
git commit -m "main: interpret_topic_input — boş Enter/rakam/eşleşme/devam-kalıbı devam kuralları (saf)"
```

---

### Task 3: `slug_system(known)` — LLM güvenlik ağı (K2)

**Files:**
- Modify: `src/main.rs` (`SLUG_SYSTEM` + `derive_slug`)
- Modify: `src/tui/run.rs` (slug LLM dalı)

**Interfaces:**
- Produces: `pub(crate) fn slug_system(known: &[String]) -> String`. `SLUG_SYSTEM` const gövdesi taban metin olarak kalır (`SLUG_SYSTEM_BASE` adına çevrilebilir).
- Consumes: Task 2 çıktısı değil — bağımsız; ama Task 4 her ikisini bağlar.

- [ ] **Step 1: Failing testler**

```rust
    #[test]
    fn slug_system_injects_known_topics() {
        let s = slug_system(&["linux-guvenlik".to_string(), "rust".to_string()]);
        assert!(s.contains("linux-guvenlik, rust"));
        assert!(s.contains("DEVAM"));
    }

    #[test]
    fn slug_system_without_topics_is_base_only() {
        let s = slug_system(&[]);
        assert!(s.contains("slug"));
        assert!(!s.contains("Mevcut konular"));
    }
```

- [ ] **Step 2: FAIL → implementasyon**

```rust
/// Slug sistem promptu — kayıtlı konular varsa devam-farkındalığı eklenir
/// (spec K2): model devam niyetini mevcut slug'a çevirir, akış Resume sayar.
pub(crate) fn slug_system(known: &[String]) -> String {
    if known.is_empty() {
        return SLUG_SYSTEM.to_string();
    }
    format!(
        "{SLUG_SYSTEM}\n\nMevcut konular: {list}. Kullanıcının yazdığı bu konulardan \
         birine DEVAM ETME isteğiyse (aynı işin sürdürülmesi, 'kaldığımız yer', önceki \
         çalışmaya atıf) SADECE o konunun slug'ını AYNEN döndür. Yeni bir konuysa yeni slug üret.",
        list = known.join(", ")
    )
}
```

`derive_slug` imzası `(backend, raw, known: &[String])` olur; `SLUG_SYSTEM` yerine `&slug_system(known)` geçer (reset_session satırı yerinde kalır). `run.rs` slug dalında `crate::SLUG_SYSTEM` → `&crate::slug_system(&local)` (local listesi Task 4'te akışa girer — bu görevde derleme için `&[]` geçici verilebilir, Task 4 bağlar; geçiciyi `// TODO(Task 4)` ile işaretle).

- [ ] **Step 3: PASS + commit**

```bash
cargo test --quiet 2>&1 | tail -3
git add src/main.rs src/tui/run.rs
git commit -m "main: slug_system(known) — LLM slug'ı mevcut konulara devam-farkındalı"
```

---

### Task 4: Welcome render — numaralı yerel liste + Enter-devam satırı

**Files:**
- Modify: `src/tui/welcome.rs` (`render_welcome_identity`)
- Modify: `src/tui/run.rs` (çağrı yeri — imza uyumu)

**Interfaces:**
- Produces: `render_welcome_identity(name: Option<&str>, model: &str, dir: &str, local: &[String], other: &[String], width: u16) -> Text<'static>`
  - `local` boş değilse sağ kolon: `Enter → <local[0]>'e devam` + `1) ...` numaralı liste (≤6) + `Yeni konu için yaz.` + (`other` boş değilse) `Diğer projelerde: a, b` soluk satırı.
  - `local` boşsa bugünkü ilk-oturum görünümü AYNEN (regresyon).

- [ ] **Step 1: Failing testler** (welcome.rs test modülüne; mevcut `plain_lines` yardımcısını kullan)

```rust
    #[test]
    fn identity_welcome_lists_local_topics_with_enter_hint() {
        let local = vec!["brainstorm-ilk-adim".to_string(), "linux-guvenlik".to_string()];
        let other = vec!["rust".to_string()];
        let t = render_welcome_identity(Some("Anil"), "opus · cli", "~/x", &local, &other, 80);
        let joined = plain_lines(&t).join("\n");
        assert!(joined.contains("Enter"));
        assert!(joined.contains("brainstorm-ilk-adim"));
        assert!(joined.contains("1)"));
        assert!(joined.contains("2)"));
        assert!(joined.contains("Diğer projelerde"));
        // Hizalama korunur.
        use unicode_width::UnicodeWidthStr;
        let lines = plain_lines(&t);
        let w = lines[0].width();
        assert!(lines.iter().all(|l| l.width() == w), "hizasız: {lines:#?}");
    }

    #[test]
    fn identity_welcome_without_local_topics_keeps_first_run_look() {
        let t = render_welcome_identity(None, "opus · cli", "~/x", &[], &[], 80);
        let joined = plain_lines(&t).join("\n");
        assert!(joined.contains("Ne öğrenmek istiyorsun"));
        assert!(!joined.contains("Enter →"));
    }
```

- [ ] **Step 2: FAIL → implementasyon**

Mevcut `render_welcome_identity` gövdesini oku; sağ kolon vektörü şöyle kurulur (mevcut `fit`/`pad`/`render_box` altyapısı aynen):

```rust
    let mut right: Vec<String> = vec!["Ne öğrenmek istiyorsun?".to_string(), String::new()];
    if let Some(first) = local.first() {
        right.push(fit(&format!("Enter → {first}'e devam"), right_w));
        for (i, t) in local.iter().take(6).enumerate() {
            right.push(fit(&format!("{}) {t}", i + 1), right_w));
        }
        right.push(String::new());
        right.push("Yeni konu için yaz.".to_string());
        if !other.is_empty() {
            right.push(fit(&format!("Diğer projelerde: {}", other.join(", ")), right_w));
        }
    } else {
        right.push("Kısa yaz ya da cümleyle anlat.".to_string());
        // mevcut "Kayıtlı:" satırı KALKAR — yerine other bilgi satırı (varsa):
        if !other.is_empty() {
            right.push(String::new());
            right.push(fit(&format!("Diğer projelerde: {}", other.join(", ")), right_w));
        }
    }
```

(Değişken adları mevcut gövdeye göre uyarlanır — `right_w` oradaki hesap.)

- [ ] **Step 3: PASS + commit**

```bash
cargo test --quiet 2>&1 | tail -3
git add src/tui/welcome.rs src/tui/run.rs
git commit -m "tui: kimlik welcome — yerel konu listesi + Enter-devam satırı + diğer-projeler bilgisi"
```

---

### Task 5: Akış entegrasyonu — TUI `run()`/`ask_topic` + plain `resolve_topic`

**Files:**
- Modify: `src/tui/run.rs`
- Modify: `src/main.rs` (`resolve_topic`, `derive_slug` çağrıları)

**Interfaces:**
- Consumes: Task 1-4'ün tümü.
- Produces: dış davranış — spec §6 elle doğrulama senaryoları.

- [ ] **Step 1: `ask_topic` — listeler dışarıdan, boş-Enter sentineli**

`ask_topic` imzasına `local: &[String], other: &[String]` eklenir; içerideki global-katalog liste kurulumu SİLİNİR (çağıran hesaplar). `render_welcome_identity` yeni imzayla çağrılır. Döngüde boş-Enter yakalama — `editor.handle_key`'den ÖNCE:

```rust
            Some(Ok(Event::Key(k))) => {
                // Boş Enter = devam sentineli (yalnız devam edilecek konu varsa) —
                // editör boş satırı yutmadan biz yakalarız (spec K1 kural 1).
                if matches!(k.code, KeyCode::Enter)
                    && editor.value().trim().is_empty()
                    && !local.is_empty()
                {
                    return Ok(Some(String::new()));
                }
                match editor.handle_key(k) {
                    Action::Submit(line) => return Ok(Some(line)),
                    Action::Exit => return Ok(None),
                    Action::None => {}
                }
            }
```

NOT: `editor.value()` şu an `#[allow(dead_code)]` — allow kaldırılır.

- [ ] **Step 2: `run()` konu seçim akışı**

Konu belirleme bloğu (topic_arg `None` dalı) şu yapıya gelir:

```rust
        None => {
            let index_content =
                std::fs::read_to_string(global.join("learner/index.md")).unwrap_or_default();
            let local = crate::index::local_topics(project_root, &index_content);
            let other: Vec<String> = {
                let mut o: Vec<String> = crate::index::entries(&index_content)
                    .into_iter()
                    .filter(|e| e.project != project_root)
                    .map(|e| e.topic)
                    .collect();
                o.dedup();
                o.truncate(4);
                o
            };
            let raw = match ask_topic(&mut tui, &mut editor, &mut events, /* profil/model/dir mevcut */, &local, &other).await? {
                Some(line) => line,
                None => return Ok(None),
            };
            match crate::interpret_topic_input(&raw, &local) {
                None => unreachable!("ask_topic boş girdiyi yalnız local doluyken döndürür"),
                Some(crate::TopicChoice::Resume(t)) => {
                    page_notice(&mut tui, &format!("devam: {t}"))?;
                    resumed = true; // aşağıda tam-mod welcome için
                    t
                }
                Some(crate::TopicChoice::New(raw)) => {
                    // Mevcut yeni-konu akışı: ≤2 kelime yerel slug, cümle → LLM.
                    // LLM dalında sistem promptu artık crate::slug_system(&local);
                    // dönen slug local'de varsa o da DEVAM sayılır:
                    let slug = /* mevcut kısa/LLM akışı, slug_system(&local) ile */;
                    if local.contains(&slug) {
                        page_notice(&mut tui, &format!("devam: {slug}"))?;
                        resumed = true;
                    } else {
                        page_notice(&mut tui, &format!("konu: {slug} — detayı sohbette anlatırsın"))?;
                    }
                    slug
                }
            }
        }
```

Uygulama notları:
- `unreachable!` YERİNE güvenli davranış tercih edilebilir: `None` gelirse döngüye dönülemez (ask_topic döndü) → `"genel"`e düşme + notice. `unreachable!` panik riski taşır — güvenli varyantı uygula, yorumla açıkla.
- `resumed` bayrağı: mevcut `if had_topic_arg { tam-mod welcome }` koşulu `if had_topic_arg || resumed { ... }` olur — devamda öğrenme-durumu kutusu basılır (kimlik welcome zaten basıldı; iki kutu üst üste kabul — Claude Code'daki akışa benzer).
- Yeni-konu notice'ı yalnız gerçekten yeni konuda basılır (mevcut `if !had_topic_arg` bloğu bu match içine taşındı — dıştaki eski notice satırı silinir).

- [ ] **Step 3: Plain yol — `resolve_topic`**

`resolve_topic` imzasına `project_root: &Path, global: &Path` eklenir (çağıran main'de mevcut). Gövde:

```rust
    let index_content =
        std::fs::read_to_string(global.join("learner/index.md")).unwrap_or_default();
    let local = index::local_topics(project_root, &index_content);
    if !local.is_empty() {
        println!("kayıtlı: {} — Enter = {}'e devam", local.join(", "), local[0]);
    }
    // ... rustyline readline aynen ...
    match interpret_topic_input(raw, &local) {
        None => return Ok("genel".to_string()),
        Some(TopicChoice::Resume(t)) => return Ok(t),
        Some(TopicChoice::New(raw)) => {
            if raw.split_whitespace().count() <= 2 {
                return Ok(slugify_topic(&raw));
            }
            let slug = derive_slug(backend, &raw, &local).await;
            return Ok(slug); // local'de olsa da olmasa da doğru — devam/yeni ayrımı TUI görseli; plain'de fark notice'ı yok
        }
    }
```

Boş-stdin/pipe yolu (`!is_terminal` → `genel`) DOKUNULMAZ.

- [ ] **Step 4: Tüm testler + regresyon**

```bash
cargo test --quiet 2>&1 | tail -3
NO_COLOR=1 echo "" | cargo run --quiet -- start rust 2>&1 | head -5   # pipe → değişmemiş
```

- [ ] **Step 5: Commit**

```bash
git add src/tui/run.rs src/tui/welcome.rs src/main.rs
git commit -m "tui+main: konu devamlılığı — Enter/rakam/eşleşme/niyet ile kaldığın konuya dön"
```

---

### Task 6: Elle doğrulama + kurulum

- [ ] **Step 1: Elle doğrulama (spec §6, gerçek terminalde)**

1. Bir konu çalış → `/quit` → aynı klasörde `usta` → welcome'da "Enter → <konu>'e devam" görünüyor → Enter → "devam: <konu>" + açılış drilli (tanışma DEĞİL).
2. `usta` → "kaldığımız yerden devam edelim" → aynı konuya drill.
3. İkinci konu aç, çık → `usta` → `2` → listedeki ikinci konuya devam.
4. `usta` → "docker compose öğrenmek istiyorum" → yeni konu normal açılır.
5. Cümleyle devam niyeti ("dün başladığımız linux işini sürdürelim") → LLM mevcut slug'ı döndürür → "devam:" notice.
6. Boş klasörde `usta` → ilk-oturum akışı aynen.

- [ ] **Step 2: Kurulum + commit**

```bash
cargo install --path .
git add -A
git commit -m "tui: konu devamlılığı v1 tamam — elle doğrulama geçti"
```

---

## Self-Review Notları

- **Spec kapsaması:** K1 kural 1-2→Task 2+5 (sentinel), 3-4-5→Task 2, 6→mevcut akış; K2→Task 3+5; welcome→Task 4; plain→Task 5 Step 3; elle doğrulama→Task 6. Boşluk yok.
- **Sıra bağımlılığı:** Task 3'ün `run.rs` geçici `&[]` parametresi Task 5'te gerçek listeye bağlanır (TODO işaretli).
- **Panik riski:** Task 5 Step 2 notu `unreachable!` yerine güvenli düşüşü zorunlu kılar.
- **Regresyon kapıları:** welcome ilk-oturum testi (Task 4), pipe `genel` yolu (Task 5 Step 4).
