# Tasarım — TUI Resize Bozulması Düzeltmesi (v0.24.6)

**Tarih:** 2026-08-27
**Kapsam:** Terminal yeniden boyutlandırılınca inline viewport görüntüsünün bozulması. Kök neden: `Event::Resize` üç event döngüsünde de yok sayılıyor (`run.rs` ana döngü `_ => continue`, `ask.rs` ask_live + confirm döngüleri, `entry.rs` giriş döngüsü) — ratatui `Viewport::Inline`'ın ezberlediği alan bayatlıyor, sonraki `draw` eski ofsete çiziyor.
**Durum:** Onaylandı → implement (Anil: "fixle", 2026-08-27)

## Davranış

- Yeni yardımcı `page.rs`'te: `pub(crate) fn handle_resize(tui: &mut Tui) -> Result<()>` — `terminal.autoresize()` + `terminal.clear()` (inline viewport alanını tazeler; bir sonraki `draw` temiz alana çizer).
- Dört yakalama noktası: `run.rs` ana match'ine tek satırlık `Event::Resize(..)` kolu · `ask_live`'ın event zincirine `else if` dalı · `ask.rs` confirm ve `entry.rs` giriş döngülerinde catch-all'dan ÖNCE Resize kolu. Hepsi yalnız `handle_resize` çağırır; döngüler zaten her turda `draw` çağırdığından ek redraw gerekmez.
- Scrollback'te DAHA ÖNCE basılmış metin terminalin kendi reflow'unda kalır — kapsam dışı (yeniden boyanamaz). Hedef: resize sonrası alt bölge (input + status) ve yeni içerik düzgün.
- `run.rs` production satır bütçesi: +1 satır (tek satırlık kol) → 600 sınırında kalır.

## Test

- Source-pin (TUI doğrudan sürülemiyor — mevcut `run_rs_wiring_call_sites_are_pinned` deseni): `page.rs` test bloğunda `include_str!` ile `run.rs`, `ask.rs`, `entry.rs` kaynaklarının `Event::Resize` işlediği assert edilir; `page.rs` production kaynağında `handle_resize` tanımı pinlenir.
- Elle doğrulama (Anil): oturum açıkken pencereyi daralt/genişlet → input kutusu + status tek kopya, düzgün çizili; mentor yanıtı beklerken (spinner) resize → aynı; yeni mesajlar doğru genişlikte sarılıyor.

## Kapsam dışı

- Eski scrollback'in yeniden sarılması · plain yol (TUI yok) · resize sırasında satır-sarma geçmişini düzeltme.
