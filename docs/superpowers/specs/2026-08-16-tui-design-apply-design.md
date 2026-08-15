# Tasarım — TUI Design System Uygulaması

**Tarih:** 2026-08-16
**Kapsam:** Claude Design'da onaylanan "Usta TUI — Terminal UI Design System" projesinin (ID: `f8cc2dc7-e09d-4b62-9000-1a023265fdc9`) koda uygulanması: tema modülü, welcome/input/status restyle, notice katmanları, exam/stats/topics biçimlendirme.
**Durum:** Onaylandı (Anil, 2026-08-16: "uygun gözüküyor, devam et tamamla" — amber uyarı, mor oyun rengi, exam kart görünümü dahil) → writing-plans
**Bağımlılık:** #6/#7/#8 planları MERGE EDİLMİŞ olmalı (stats/exam/game yüzeyleri var olmalı). Kaynak-of-truth: Claude Design projesi — implementasyon agent'ı MCP ile (`mcp__claude-design__list_files`/`read_file`, ToolSearch'le yükle) mockup'ları OKUMALI.

## Amaç

TUI bugün işlevsel ama görsel dil gelişigüzel (ham `Color::Yellow`, tutarsız vurgu, glif dili yok). Onaylı tasarım sistemi: sakin ekran (ADHD), renk semantiği, renk-körü güvenliği, monokrom dayanıklılığı.

## Çekirdek kurallar (tasarım projesinden — bağlayıcı)

- **Palet (truecolor + 256 fallback):** marka turuncu `#ff8700`/208 — YALNIZ kimlik (logo, `●` marka noktası, `❯` prompt, başlıklar); başarı yeşil 149; uyarı **amber** 179 (mevcut `Color::Yellow`'un yerine); hata kırmızı 210; oyun/XP **mor** 141 (Usta'nın sesiyle karışmaz); ortam metni dim.
- **Turuncu disiplini:** durağan ekranda en fazla 2 turuncu öğe.
- **Glif+renk çifti** (renk körlüğü/monokrom): bilgi `·` dim · başarı `✓` yeşil · uyarı `⚠` amber · hata `✗` kırmızı · oyun `▸` mor · marka `●` turuncu · prompt `❯` turuncu.
- **Kutu dili:** canlı çerçeveler yuvarlak `╭╮╰╯`; tablo başlığı altı ince tek çizgi; gauge `▓░`; exam ilerleme `●○`. Ağır grid yok.
- **Durum satırı:** tek `Line`, ≤3 span; spinner `⠋⠙⠸⠴` ~120ms; bağlam göstergesi ≥%70'te amber.
- **Exam kartı (onaylı öneri, model-çizimli):** kabuk soru-durumu parse ETMEZ — GOAL.md kuralına format eklenir: model her soruyu `── Question N/M ──` başlık satırı + sonda `●●●○○` ilerleme ile yazar; skor kırılımı başlık-altı-çizgili tablo. Kabuk riski sıfır.
- **stats/topics/help çıktıları:** mockup'taki hizalama + başlık-altı çizgi + glif kullanımı.

## Davranış (uygulama alanları)

1. **`src/tui/theme.rs` (yeni):** semantik renk sabitleri (`Color::Indexed` — 208/149/179/210/141; truecolor terminalde indexed de doğru — basitlik için indexed tek yol), glif sabitleri, hazır `Style` yardımcıları (`info()`, `success()`, `warn()`, `error()`, `game()`, `brand()`). TÜM tui modülleri renk/glifi buradan alır — dağınık `Color::` literal'leri temizlenir (`ui.rs` plain-mode ANSI'si dahil — eşdeğer ANSI kodlarıyla).
2. **Welcome kutuları:** yuvarlak köşeler, turuncu disiplini (logo + tek vurgu), satır glifleri (reviews due `·`/`✓`, hafta satırı), mockup 02 birebir.
3. **Input + status:** `❯` prompt turuncu, spinner glifleri, gauge `▓░` + amber eşiği, watching göstergesi.
4. **Notice katmanları:** `page_notice`/`ui::notice` → `·` dim; `ui::warn` → `⚠ ` amber; hata yolları → `✗ ` kırmızı. Mevcut metinler değişmez, ön-ek + stil değişir.
5. **Exam formatı:** `GOAL.md ## Mock Exams`'a format kuralı (`── Question N/M ──`, `●○` ilerleme, kırılım tablosu) — model çizer.
6. **Game satırı:** `[GAME]` besleme satırı ve TEACHING.md doz kuralına glif notu (`▸` mor tek satır).
7. **stats/topics/help:** render fonksiyonları mockup 05'e göre hizalanır (kolon hizası unicode-width ile, başlık altı çizgi).

## Test

- theme sabitleri: semantik→(renk,glif) eşlemesi assert (vacuous olmayan: warn≠yellow, game=141, glif seti tam).
- Welcome/status/notice render testleri: glif ön-ekleri + stil uygulanmış span'ler (mevcut render-test desenleri).
- `render_stats`/help: hizalama + çizgi satırı assert.
- Turuncu disiplini: welcome render'da turuncu span sayısı ≤2 (identity kutusunda logo bloğu tek öğe sayılır — test yorumla netleştirir).
- Davranış regresyonu YOK: tüm mevcut testler yeşil kalır (metin içerikleri değişmiyorsa assert'ler dayanır; glif ön-eki eklenen notice assert'leri güncellenir).

## Kapsam dışı

- Kabuğun exam-durumu parse etmesi / gerçek kart widget'ı.
- Tema konfigürasyonu (kullanıcı teması) — tek tasarım.
- Plain/pipe modunda renk (NO_COLOR yolu zaten düz).
- Welcome logosunun yeniden çizimi.

## Açık sorular

Yok — dört açık soru Anil onayıyla kapandı (amber ✓, model-çizimli exam kartı ✓, mor oyun ✓, görsel doğrulama Anil'de).
