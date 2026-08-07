# TUI-İçi Konu Girişi — Tasarım Spec'i

> **Bağlam:** İlk TUI (`docs/superpowers/specs/2026-08-07-tui-interface-design.md`, `main` @ v0.10) konuyu TUI'den ÖNCE, çıplak rustyline promptuyla soruyor. Kullanıcı Claude Code tarzı istiyor: **önce welcome (üstte), sonra konu sorusu ayrı kutuda (altta).** Bu spec o değişimi tanımlar.

## Amaç

İnteraktif TTY oturumunda Usta arayüzü **konu sorusundan önce** görünsün. `usta` (konusuz) çalıştırıldığında: welcome kutusu scrollback'e basılır (kimlik modu), altta canlı girdi kutusu konuyu sorar, kullanıcı yazar, oturum başlar. Konu argümanla verildiyse (`usta start rust`) welcome tam mod (öğrenme durumu) gösterir, soru sormaz.

## Kapsam

**Dahil:**
- `tui::run` konusuz da açılabilir; topic resolution TUI olay döngüsüne taşınır.
- `WelcomeData`/`render_welcome` **kimlik modu** (konu yok) kazanır.
- Konu-girişi turn'ü: hint satırı + girdi kutusu + slug çözümü (yerel ≤2 kelime / cümle→LLM+spinner).
- Session kurulum kodu (system prompt, Session, lock, recorder, has_progress) `build_session` yardımcısına çıkarılır; hem TUI hem plain kullanır.
- Kapanış (`flush_progress`, `mark_done`, lock silme, veda) `main`'de paylaşımlı kalır; her iki yol `(Session, Recorder, PathBuf)` döndürür.
- Lock-çakışması onayı TUI'de tek-tuş (`e/h`) ile; plain'de mevcut rustyline `confirm`.

**Dışında (YAGNI):**
- Plain yol davranışı **birebir korunur** — `resolve_topic` rustyline dalı + banner + döngü aynen.
- Konu-girişinde geçmiş/autocomplete yok (kayıtlı konular sadece hint satırında metin olarak listelenir).
- Yeniden konu değiştirme (oturum ortası) — yok.

## Davranış

### `usta` (konu argümanı yok, TTY)
1. `main`: backend seç, scaffold, global root — konu ÇÖZÜLMEZ.
2. `tui::run` çağrılır (`topic_arg = None`).
3. `run`: `term::setup()` (bir kez) → **kimlik welcome** scrollback'e:
   - sol kolon: logo + selam (kullanıcı kendi profiline isim yazdıysa "Merhaba, <isim>!", yoksa "Merhaba!" — gömülü default profil isimsizdir) + model + dizin
   - alt/sağ: "Ne öğrenmek istiyorsun?" + varsa "kayıtlı: rust · gtm · …" (global index'ten, boşsa atlanır)
4. Hint satırı (`page_notice` benzeri, soluk): "Ne öğrenmek istiyorsun? (kısa yaz ya da cümleyle anlat)"
5. Girdi kutusu aktif. Kullanıcı yazar, Enter.
   - Boş Enter → hiçbir şey (InputBox zaten `Action::None`).
   - Ctrl-C/Ctrl-D → temiz çıkış, oturum YOK (Tui drop → restore, `main` boş-oturum kapanışı).
6. Slug çözümü (mevcut `resolve_topic` mantığı):
   - ≤2 kelime → `slugify_topic` (LLM yok).
   - Cümle → `derive_slug` (LLM; **ask_live spinner** döner). Hata → yerel slug.
   - Seçilen slug bir `page_notice` ile bildirilir ("konu: rust-todo — detayı sohbette anlatırsın").
7. Lock-çakışması kontrolü: `lock_path(topic)` varsa `tui_confirm` (tek-tuş); reddedilirse `run` temiz döner (session yok). Sonra `build_session(global, project_root, topic, today)` → system prompt, Session, kendi lock yazımı, recorder, has_progress.
8. **Konu-belli olduğu için** istenirse tam-mod welcome BASILMAZ (kimlik zaten basıldı) — bunun yerine öğrenme durumu açılış drillinde sözel gelir (mevcut `opening_prompt` "neredeyiz, sırada ne" der). *(Karar: tek welcome; ikinci kutu gürültü.)*
9. Açılış drilli (has_progress → drill / değilse onboarding) → ana döngü.

### `usta start rust` (konu belli, TTY)
1. `main`: backend, scaffold, global root; `topic_arg = Some("rust")` (slug'lanmış).
2. `tui::run(topic_arg = Some(...))`.
3. `run`: setup → `build_session` → **tam-mod welcome** (mevcut çift kolon: seviye, harita %, sırada, drill) scrollback'e → açılış drilli → ana döngü. **Soru sorulmaz.**

### Plain (`ui::is_plain()`: TTY yok / `NO_COLOR`) — DEĞİŞMEZ
`main`: `resolve_topic` (rustyline / pipe→"genel") → `build_session` → `ui::banner` → `run_plain_loop` → paylaşımlı kapanış. Bugünle birebir.

## Mimari

### Sorumluluk sınırları

| Birim | Sorumluluk | Değişim |
|---|---|---|
| `main` | backend/scaffold/global; **yol seçimi** (TUI vs plain); **paylaşımlı kapanış** | `resolve_topic`+session-build TUI dalından çıkar; her iki yol `(Session, Recorder, PathBuf)` döndürür |
| `build_session` (yeni, `main`) | topic'ten: system prompt + Session + lock(+onay) + recorder + has_progress | çıkarım — bugün main gövdesinde satır içi olan kod |
| `tui::run` | setup → topic-entry (arg yoksa) → build_session → welcome → drill → döngü; artefaktları döndür | konusuz açılabilir; topic-entry turn'ü eklenir |
| `tui::welcome` | `WelcomeData` + render; **kimlik modu** eklenir | `render_welcome` konu-yok dalı |
| `tui::editor` | girdi kutusu | değişmez (konu da normal satır) |

### `tui::run` yeni imza (öneri)
```rust
pub async fn run(
    backend: &mut Backend,
    global: &Path,
    project_root: &Path,
    today: &str,
    topic_arg: Option<String>,   // None → TUI-içi konu girişi
    max_feedback_batch: usize,
    watch_rx: &mut UnboundedReceiver<PathBuf>,
) -> Result<(Session, Recorder, PathBuf)>   // (session, recorder, lock) → main kapanışa verir
```
`run` içinde topic belirlenince `build_session` çağrılır. Dönüşte `Tui` drop olur (restore), `main` paylaşımlı kapanışı restore edilmiş terminalde koşar.

### `build_session` (yeni yardımcı, `main`)
```rust
fn build_session(
    global: &Path, project_root: &Path, topic: &str, today: &str,
) -> Result<(Session, Recorder, PathBuf /*lock*/, bool /*has_progress*/)>
```
İçi (bugün main gövdesinde satır içi olan kod, aynen): `load_system_prompt` → `Session::new` → `lock_path` → **kendi kilidini yaz** (`std::fs::write(lock, pid)`) → `Recorder::new` → `has_progress` hesabı. Döner: `(session, recorder, lock, has_progress)`.

**Lock yazımı `build_session`'da; lock-ÇAKIŞMASI onayı DIŞARIDA** — çünkü onay I/O'su yola göre değişir (plain: stdin `confirm()`, TUI: tek-tuş). Çağıran, `build_session`'dan ÖNCE çakışmayı kontrol edip onayı alır:
- **plain** (`main`): mevcut `lock.exists()` + `confirm(...)` bloğu aynen (topic `resolve_topic`'ten sonra biliniyor).
- **TUI** (`run`): topic-entry'den sonra, `build_session`'dan önce `lock_path(topic)` var mı bak; varsa `tui_confirm(&mut tui, events, msg) -> Result<bool>` — mesajı `page_notice` ile basar, `EventStream`'den tek tuş okur (`e`/`evet` → devam, diğer/Ctrl-C → vazgeç → `run` temiz döner, session yok). Non-TTY dalı TUI'de yok (TUI zaten TTY).

*(Böylece `build_session` I/O-yolundan bağımsız saf kurulum; onay her yolun kendi döngüsünde.)*

### `WelcomeData` / `render_welcome` — kimlik modu
`WelcomeData`'ya `topic: Option<String>` semantiği (veya ayrı `render_welcome_identity`). Konu `None`/boş iken:
- sağ kolon "Öğrenme Durumu" yerine "Ne öğrenmek istiyorsun?" + kayıtlı konular (global `index::entries` → `konu` alanları, ilk N) veya "İlk oturum — bir konu yaz."
- sol kolon logo + selam + model + dizin (değişmez).
Eşit-genişlik invaryantı + `fit()` sarma korunur (mevcut testler + yeni kimlik-modu testi).

## Hata / kenar durumları
- Konu-girişinde Ctrl-C/Ctrl-D → `run` erken döner, session YOK; `main` boş-oturum kapanışı (dosya yok, lock yok — lock henüz yazılmadı).
- Boş/whitespace Enter → InputBox `Action::None`, kutu bekler.
- `derive_slug` LLM hatası → yerel `slugify_topic`'e düş (oturum engellenmez).
- Slug "genel"e düşerse (dolu girdi ama hep stopword) → mevcut `resolve_topic` davranışı (yerel slug fallback).
- Lock onayı reddedilirse (TUI) → `run` temiz döner, session yok, "vazgeçildi" notice.

## Test
- `welcome`: kimlik-modu render → eşit genişlik, "Ne öğrenmek istiyorsun?" içerir, konu-yok'ta öğrenme-durumu satırları YOK. `fit` kayıtlı-konu listesini sarar.
- `slugify_topic`/`derive_slug` mevcut testleri korunur (mantık taşınmadı, çağrı yeri değişti).
- Plain regresyon: mevcut 125 test + `NO_COLOR` smoke birebir.
- IO/olay kodu (`run` topic-entry, `tui::confirm`) unit-test edilmez → elle TTY doğrulama (spec §Doğrulama).

## Doğrulama (elle, gerçek TTY + canlı LLM)
1. `usta` → welcome (kimlik) üstte, altta "Ne öğrenmek istiyorsun?" + girdi kutusu.
2. Konu yaz ("rust") → drill başlar. Cümle yaz ("golang öğrenmek istiyorum") → spinner → slug bildirilir → drill.
3. `usta start rust` → tam-mod welcome (öğrenme durumu) → drill, soru YOK.
4. Konu-girişinde Ctrl-C → temiz çıkış, shell sağlam, lock yok.
5. `NO_COLOR=1 usta` → eski düz akış (rustyline "Konu nedir?"), TUI açılmaz.
6. Aynı konuda ikinci oturum → lock onayı (TUI tek-tuş) çalışır.

## İlgili
- İlk TUI spec: `docs/superpowers/specs/2026-08-07-tui-interface-design.md`
- İlk TUI planı: `docs/superpowers/plans/2026-08-07-tui-interface.md`
- Kod: `src/tui/run.rs`, `src/tui/welcome.rs`, `src/main.rs`
