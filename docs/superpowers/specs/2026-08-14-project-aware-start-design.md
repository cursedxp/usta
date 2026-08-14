# Tasarım — Proje-Farkında Başlangıç Önerisi

**Tarih:** 2026-08-14
**Kapsam:** İlk oturumda `mentor/PROJECT.md` varsa konu-giriş ekranı bağlam-kör kalmasın: boş Enter → Usta PROJECT.md'den başlangıç önerir (konu + gerekçe + ilk adım), kullanıcı onaylar.
**Durum:** Onaylandı → writing-plans
**Bağımlılık:** 2026-08-14-mentor-context-layer (implement edildi — `mentor/PROJECT.md` mevcut altyapı).

## Amaç

Anil'in canlı testte yakaladığı boşluk: `mentor/PROJECT.md`'yi elle doldurdu, ama welcome ekranı "First session — type a topic." dedi — dosyayı okumuyor. Usta kullanıcının NE yaptığını biliyor ama kullanıcı NASIL başlayacağını bilmiyor; araç tam bu anda yardım etmiyor. Akış tersine dönmeli: proje tanımı varken kullanıcı konu uydurmak zorunda kalmasın — **Usta önersin, kullanıcı onaylasın.**

İki gerekçe (Anil):
1. **Koşullandırma:** kullanıcı bir şey yazarsa Usta'yı kendi tahminine koşullandırır — halbuki plan (PROJECT.md) zaten elinde; başlangıç plandan türemeli, kullanıcının o anki tahmininden değil.
2. **İnsanlar bazen nasıl başlayacağını bilmez** — "bilmiyorum" geçerli bir başlangıç olmalı; boş Enter tam bunu temsil eder.

Kararlar:
- Tetik: TUI konu girişinde **boş Enter**, yalnız şu koşulda: bu projede resume edilecek konu YOK (`local` boş) + `mentor/PROJECT.md` var ve boş değil. (`local` doluysa boş Enter = resume — mevcut davranış DEĞİŞMEZ.)
- Öneri tek mini LLM çağrısıyla üretilir (slug mini-session deseniyle birebir: çağrı sonrası `backend.reset_session()` — öneri sohbeti öğrenme oturumuna taşmaz).
- Öneri = konu slug'ı + 2-3 cümle gerekçe + somut ilk adım. Kullanıcı onaylarsa oturum o konuyla açılır; öneri metni `intro` olarak onboarding'e taşınır (Usta kendi önerisinden habersiz başlamasın).
- Reddederse konu-giriş sorusuna dönülür (mevcut "cancelled → ask again" deseni).
- Plain/pipe yolu (`resolve_topic`, TTY yok) kapsam DIŞI — davranış değişmez.
- `usta start <konu>` arg yolu değişmez.

## Davranış

### 1. Welcome ipucu (`src/tui/welcome.rs`)

`render_welcome_identity` sağ kolonunda, `local` boş + PROJECT.md varken:
- Eski: `First session — type a topic.`
- Yeni: `PROJECT.md found — press Enter, Usta suggests where to start.`

Prompt satırı (`src/tui/run.rs:329`) aynı koşulda:
- Eski: `What do you want to learn? (a word, or describe it in a sentence)`
- Yeni: `What do you want to learn? (Enter = Usta suggests from PROJECT.md; or type a topic)`

PROJECT.md yokken veya `local` doluyken iki metin de mevcut haliyle kalır.

### 2. Karar mantığı (`src/main.rs`)

`TopicChoice` enum'una `Suggest` varyantı; `interpret_topic_input(raw, local, project_known: bool)`:
- boş input + `local` boş + `project_known` → `Some(TopicChoice::Suggest)`
- boş input + `local` dolu → `Resume` (değişmez, `project_known`'dan bağımsız — resume önceliklidir)
- boş input + `local` boş + `!project_known` → `None` (değişmez)
- dolu input → mevcut kurallar aynen.

`project_known` = `progress::project_md_path(project_root)` var VE içeriği boş/whitespace değil.

### 3. Öneri mini-çağrısı (`src/main.rs`, slug helper'larının yanı)

- `start_suggest_system() -> String` — system prompt: "You are Usta. The user has a project definition but doesn't know where to start learning. Reply in the session language of the project file. FIRST line exactly `KONU: <topic-slug>` (lowercase, hyphenated, 1-3 words). Then 2-4 sentences: why this topic first, and ONE concrete first step (small, startable today). No greeting, no markdown headings."
- Kullanıcı mesajı: yalnız `mentor/PROJECT.md` içeriği — öneri plandan türer, başka girdiyle koşullanmaz (Amaç'taki gerekçe 1).
- `parse_start_suggestion(reply: &str) -> Option<(String, String)>` — ilk satırdan `KONU:` sonrasını `slugify_topic` ile slug'lar; kalan satırlar = öneri metni. `KONU:` satırı yoksa veya slug boşsa `None` → TUI notice "suggestion failed — type a topic" + giriş sorusuna dön (oturum açılmaz, çökme yok).

### 4. TUI akışı (`src/tui/run.rs`, konu-giriş döngüsü)

`TopicChoice::Suggest` kolu:
1. `ask_live` ile mini-çağrı (spinner — slug çağrısıyla aynı mekanik), ardından HER durumda `backend.reset_session()`.
2. Parse başarılı → öneri metni `page_notice` ile gösterilir + `tui_confirm("start with '<slug>'? [E/h]")`.
3. Onay → `topic = slug`, `intro = Some(öneri metni, başına "Usta's own opening suggestion (already shown to the user):" notu)` — onboarding bunu ilk cevap olarak alır, öneriyi tekrar anlatmaz, ilk adımdan devam eder. `local` boş olduğundan mevcut kural gereği yeni-konu onayı TEKRAR sorulmaz.
4. Red → notice `cancelled — type a topic` → giriş sorusuna dön (welcome yeniden basılmaz).
5. Çağrı iptal/hata (`Cancelled`/`Err`) → notice + giriş sorusuna dön.

## Test

- `interpret_topic_input`: boş+boş+known → `Suggest`; boş+dolu+known → `Resume`; boş+boş+unknown → `None`; dolu inputlar `project_known`'dan etkilenmez. Mevcut çağrı yerleri yeni parametreyle güncellenir.
- `parse_start_suggestion`: düzgün yanıt → (slug, metin); `KONU:` yok → `None`; slug satırı Türkçe karakter/boşluklu → slugify normalize eder; tek satırlık yanıt (metin boş) → `None` DEĞİL, boş-metin kabul (metin opsiyonel değil ama tolere edilir — TUI yine confirm gösterir, slug yeter).
- `start_suggest_system`: `KONU:` format sözleşmesini içerir (string assert).
- Welcome/prompt ipucu: `project_known` true/false render farkı (mevcut welcome test deseniyle).
- TUI döngü kolu: kod incelemesiyle (mevcut desen — döngü unit-test edilmiyor).

## Kapsam dışı

- Plain/pipe yolunda öneri.
- `local` doluyken öneri ("bugün ne çalışayım" önerisi — ayrı fikir, ayrı spec).
- Birden fazla öneri seçeneği / menü.
- PROJECT.md yokken LLM'siz jenerik öneri.

## Açık sorular

Yok.
