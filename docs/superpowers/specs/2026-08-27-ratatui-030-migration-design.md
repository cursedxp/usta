# Tasarım — ratatui 0.30 Migration: Yatay Resize Kalıcı Fix (v0.26.0)

**Tarih:** 2026-08-27
**Kapsam:** ratatui 0.29.0 → 0.30.2 yükseltmesi. Gerekçe: inline viewport'un yatay resize bozulması upstream bug (ratatui issue #2086); resmî düzeltme PR #2355 ("fix inline viewport resizing issues by clearing the screen") 0.30'da yayınlandı — 0.29'da bizim v0.24.6 workaround'u (autoresize+clear) yatayı kurtaramıyor. Davranış paritesi esas: görünür TUI davranışı (renkler, kutular, akış) değişmez; tek beklenen fark resize'ın düzelmesi.
**Durum:** Onaylandı → implement (Anil, 2026-08-27: "yeni sürüme geçelim")

## Bilinen kısıtlar / bağımlılık düğümü

- `Cargo.toml`'da bilinçli **crossterm tekillik pini** var (0.28; manifest yorumu: coolor 1.1+/crokey 1.5+ crossterm ^0.29'a geçti — ikili crossterm ağacı yasak). ratatui 0.30'un crossterm gereksinimi ile ağaç TEK crossterm sürümünde birleşmeli (muhtemelen hep birlikte 0.29'a çıkarak; `cargo tree -i crossterm` tek kök göstermeli). Pin yorumu yeni duruma göre güncellenir.
- Yan bağımlılıklar ratatui/crossterm sürümüne bağlı: `tui-input` (editor), `termimad` (markdown skin), varsa `coolor/crokey` zinciri — 0.30/yeni-crossterm uyumlu sürümlere birlikte çıkarılır.
- 0.30 kırıcı sürüm (crate/modül yeniden düzenlemesi). Migration referansı: ratatui repo `BREAKING-CHANGES.md` + 0.30 duyurusu — implementasyonda WebFetch ile OKUNUR, ezberden API uydurulmaz.
- Etkilenen modüller: `src/tui/*` (term, page, paint, status, theme, editor, welcome, ask, entry, run) + `src/ui.rs`.

## Davranış

- **Parite:** tüm mevcut testler değişmeden yeşil kalır (test GÜNCELLEMESİ yalnız API adı değiştiyse — davranış assert'leri aynı). Görsel çıktı birebir (tema indexed renkler, glyph'ler, viewport 6 satır).
- **Resize:** `Event::Resize` yakalama noktaları (v0.24.6'nın 4 noktası) ve pin testleri KALIR. `page::handle_resize` gövdesi 0.30'un resize semantiğine göre yeniden değerlendirilir: upstream artık kendisi doğru temizliyorsa gövde sadeleşir (yalnız `autoresize` veya hiçbir şey + redraw), gereksiz `clear` bırakılmaz — karar BREAKING-CHANGES/API dokümanına dayanır, yorum satırıyla gerekçelenir.
- **Sürüm:** v0.26.0 (bağımlılık major migration). MSRV: 0.30'un MSRV'si `rust-version` alanıyla karşılaştırılır (`cargo metadata --locked` yeniden ölçülür — v0.24.2 dersi: pin ölçülür, uydurulmaz).

## Test

- Mevcut 398 test parite (davranış assert'leri değişmez).
- `cargo tree -i crossterm` → TEK sürüm (test/verify adımında kanıtlanır, plan raporuna yazılır).
- Elle doğrulama (Anil — kapanış kriteri): oturum açıkken pencereyi YATAY daralt/genişlet → alt bölge tek kopya, bozulma yok; dikey resize; spinner sırasında resize; scrollback okunabilir.

## Kapsam dışı

- Davranış/özellik değişikliği yok · plain yol dokunulmaz · tema/görsel yenileme yok · ratatui 0.30'un yeni özelliklerinin benimsenmesi (yalnız zorunlu API uyarlaması).
