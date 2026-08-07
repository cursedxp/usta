# Yeni Konu Onayı — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **ÖN KOŞUL — ÖNCE DOĞRULA:** `docs/superpowers/plans/2026-08-07-topic-resume.md` planı main'e UYGULANMIŞ olmalı (`git log`'da "konu devamlılığı" commit'leri + `interpret_topic_input`/`TopicChoice` `src/main.rs`'te mevcut). Değilse DUR ve kullanıcıya söyle. Spec: `docs/superpowers/specs/2026-08-07-new-topic-confirm-design.md` — OKU.

**Goal:** Yeni konu açılmadan önce tek tuş onay (yalnız devam edilebilir konu varken). Ret → konu seçimine geri dönüş.

**Architecture:** `run()` konu-belirleme bloğu bir `loop`'a alınır: ask → interpret → New ise slug türet → onay; ret → döngü başına (giriş sorusu tekrar). Plain'de `resolve_topic` aynı döngü + mevcut `confirm()`. Yeni bağımlılık yok.

## Global Constraints

- Türkçe yorum/UI; `cargo test --quiet`; commit stili aynı.
- `Resume` yollarına ve ilk-oturum akışına (yerel konu yokken) DOKUNULMAZ — onay yalnız `New` + yerel liste dolu.
- Pipe/boş-stdin yolu (`genel`) değişmez.

---

### Task 1: Onay mesajı + TUI akış döngüsü

**Files:**
- Modify: `src/tui/run.rs`
- Modify: `src/main.rs` (saf mesaj fn — plain ile paylaşılır)

**Interfaces:**
- Produces: `pub(crate) fn new_topic_confirm_msg(slug: &str) -> String`
- Consumes: topic-resume'dan `TopicChoice`, `interpret_topic_input`, mevcut `tui_confirm` (run.rs — imzası: mesaj basar, `e`/`E` → true, diğer → false).

- [ ] **Step 1: Failing test** (`src/main.rs` test modülü)

```rust
    #[test]
    fn new_topic_confirm_msg_names_slug_and_keys() {
        let m = new_topic_confirm_msg("rust-cli");
        assert!(m.contains("rust-cli"));
        assert!(m.contains("[e"));
    }
```

Çalıştır: `cargo test --quiet new_topic_confirm 2>&1 | tail -3` → derleme hatası (fn yok).

- [ ] **Step 2: Mesaj fn**

```rust
/// Yeni konu onay metni — TUI notice ve plain confirm aynı dili kullanır.
pub(crate) fn new_topic_confirm_msg(slug: &str) -> String {
    format!("yeni konu: {slug} — açayım mı? [e = evet / başka tuş = geri dön]")
}
```

- [ ] **Step 3: `run()` konu bloğunu döngüye al**

Topic-resume sonrası `run()`'daki konu belirleme (topic_arg `None` dalı) şu yapıya gelir — `local`/`other` listeleri döngü DIŞINDA bir kez hesaplanır, welcome `ask_topic` içinde ilk turda basılır (tekrar basılmaması için bayrak):

```rust
        None => {
            // ... local/other hesaplama (topic-resume'dan aynen) ...
            let mut welcome_shown = false;
            loop {
                let raw = match ask_topic(
                    &mut tui, &mut editor, &mut events,
                    /* profil/model/dir */, &local, &other,
                    !welcome_shown, // yeni parametre: kimlik welcome basılsın mı
                ).await? {
                    Some(line) => line,
                    None => return Ok(None),
                };
                welcome_shown = true;
                match crate::interpret_topic_input(&raw, &local) {
                    Some(crate::TopicChoice::Resume(t)) => {
                        page_notice(&mut tui, &format!("devam: {t}"))?;
                        resumed = true;
                        break t;
                    }
                    Some(crate::TopicChoice::New(raw)) => {
                        let slug = /* topic-resume'daki mevcut kısa/LLM türetme aynen */;
                        if local.contains(&slug) {
                            page_notice(&mut tui, &format!("devam: {slug}"))?;
                            resumed = true;
                            break slug;
                        }
                        // Onay: yalnız devam edilebilir konu varken (spec §2).
                        if local.is_empty()
                            || tui_confirm(&mut tui, &editor, &mut events,
                                   &crate::new_topic_confirm_msg(&slug)).await?
                        {
                            page_notice(&mut tui, &format!("konu: {slug} — detayı sohbette anlatırsın"))?;
                            break slug;
                        }
                        // Ret → giriş sorusuna geri dön (welcome tekrar basılmaz).
                        page_notice(&mut tui, "vazgeçildi — Enter = devam, ya da başka konu yaz")?;
                    }
                    None => { /* boş girdi + konu yok — döngü devam (yut) */ }
                }
            }
        }
```

Uygulama notları:
- `loop { ... break value }` deseni: `topic` değişkenine `let topic = match topic_arg { Some(t)=>..., None => loop { ... } };` şeklinde bağlanır — mevcut yapıya uydur.
- `ask_topic`'e `show_welcome: bool` parametresi eklenir: `false` ise kimlik welcome + ilk notice basılmaz (yalnız giriş döngüsü). Mevcut çağrı tek yerden — imza değişikliği lokal.
- `interpret_topic_input` `None` dönüşü: topic-resume planı `unreachable`/güvenli-düşüş notu koymuştu; bu döngüde doğal karşılığı "yut, tekrar sor" — güvenli düşüş budur, `genel` fallback'i kaldırılabilir (döngü zaten tekrar sorar).
- Slug LLM çağrısı onay REDDEDİLİRSE boşa gitmiş olur — kabul (ucuz); reset_session satırı (B1) her turda yerinde kalır.

- [ ] **Step 4: Testler + commit**

```bash
cargo test --quiet 2>&1 | tail -3
git add src/main.rs src/tui/run.rs
git commit -m "tui: yeni konu onayı — tek tuş; ret konu seçimine geri döner"
```

---

### Task 2: Plain yol + elle doğrulama + kurulum

**Files:**
- Modify: `src/main.rs` (`resolve_topic`)

- [ ] **Step 1: `resolve_topic` döngüsü**

Topic-resume sonrası `resolve_topic` gövdesi döngüye alınır; `New` dalında slug türetimi sonrası (yerel liste doluysa) mevcut `confirm()` ile sor:

```rust
    loop {
        // ... readline + interpret (topic-resume'dan aynen) ...
        match interpret_topic_input(raw, &local) {
            None => return Ok("genel".to_string()),
            Some(TopicChoice::Resume(t)) => return Ok(t),
            Some(TopicChoice::New(raw)) => {
                let slug = if raw.split_whitespace().count() <= 2 {
                    slugify_topic(&raw)
                } else {
                    derive_slug(backend, &raw, &local).await
                };
                if local.contains(&slug) {
                    return Ok(slug); // LLM devam'a çözdü
                }
                if local.is_empty()
                    || confirm(&format!("Yeni konu '{slug}' açılsın mı? [e/H] "), &["e", "evet"])?
                {
                    return Ok(slug);
                }
                println!("vazgeçildi — Enter = {}'e devam, ya da başka konu yaz", local[0]);
                // döngü başa: tekrar sor
            }
        }
    }
```

NOT: `confirm()` main.rs'te mevcut (lock çakışması kullanıyor) — imzasına bak, aynen kullan. Pipe yolu (`!is_terminal` erken dönüşü) döngünün DIŞINDA kalır — dokunma.

- [ ] **Step 2: Tüm testler + pipe regresyonu**

```bash
cargo test --quiet 2>&1 | tail -3
NO_COLOR=1 echo "" | cargo run --quiet -- start rust 2>&1 | head -5   # değişmemiş
```

- [ ] **Step 3: Elle doğrulama (spec §4)**

1. Mevcut konulu klasörde `usta` → "docker öğrenmek istiyorum" → onay çıkar → `e` → yeni konu açılır.
2. Aynısı → başka tuş → "vazgeçildi" → Enter → son konuya devam.
3. Boş klasörde `usta` → yeni konu → onay ÇIKMAZ.
4. Enter / rakam / "devam edelim" yolları → onay ÇIKMAZ, doğrudan devam.
5. Plain: `NO_COLOR=1 usta` (etkileşimli) → aynı onay `[e/H]` satırıyla.

- [ ] **Step 4: Kurulum + commit**

```bash
cargo install --path .
git add -A
git commit -m "main: plain yolda yeni konu onayı — elle doğrulama geçti"
```

---

## Self-Review Notları

- **Spec kapsaması:** §2 TUI→Task 1, plain→Task 2, ilk-oturum muafiyeti→`local.is_empty()` kısa devresi (iki görevde de), Resume muafiyeti→onay yalnız `New` dalında. Boşluk yok.
- **Çakışma riski:** Bu plan topic-resume'un yeniden yazdığı bloklara dokunur — ön koşul kontrolü plan başında ZORUNLU.
- **`tui_confirm` yeniden kullanımı:** mevcut davranış (e=true, diğer=false) onay semantiğiyle birebir — yeni widget yok.
