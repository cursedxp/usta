# Input kutusu — opencode tarzı çok satırlı editör

**Tarih:** 2026-10-26 · **Branch:** `multiline-input` · **Kapsam:** `src/tui/editor.rs`, `src/tui/term.rs`, `src/tui/page.rs`, `Cargo.toml`

## İstek

Anil: "opencode'daki yazı alanı bizimkinden daha iyi ve responsive — ustaya uygula, opencode'daki
şekilde kopyala." Referans opencode 2.1.4 (Bun binary'sinden REA ile çıkarılan keybind tablosu ve
prompt kodu). Bugünkü `InputBox` `tui-input` üstünde tek `String`; `\n` düz karakter, dikey imleç
yok, ↑ her zaman history, yükseklik sabit 6.

## Tasarım

### 1. Çekirdek: `ratatui-textarea` 0.9.3

`tui-input` → `ratatui-textarea` (ratatui org; `ratatui-core ^0.1` + `ratatui-widgets ^0.3` =
ratatui 0.30 ile aynı çekirdek). Satır listesi + `(row, col)` imleç, undo/redo, kelime hareketi,
seçim kütüphaneden gelir. Derleme uyumu ilk görevde doğrulanır; tutmazsa plan durur, Anil'e sorulur.

`InputBox` dış API'si aynı kalır: `handle_key(KeyEvent) -> Option<Action>`, `insert_str(&str)`,
`render(&self, f, area)`, `value()`, `reset()`. Yeni: `desired_height(width) -> u16`.
`run.rs`/`paint.rs`/`ask.rs` çağrıları değişmez.

### 2. Tuşlar (opencode 2.1.4 varsayılanları)

| Tuş | Davranış |
|---|---|
| Enter | gönder (boş/whitespace → yok sayılır, bugünkü gibi) |
| Shift/Ctrl/Alt+Enter, Ctrl+J | yeni satır |
| ↑ | imleç ilk satırda değilse satır yukarı; ilk satırdaysa history geri |
| ↓ | imleç son satırda değilse satır aşağı; son satırdaysa history ileri |
| ←/→, Ctrl+B/F | karakter |
| Ctrl+A / Ctrl+E | satır başı / sonu |
| Home / End | satır başı / sonu |
| Ctrl+K / Ctrl+U | satır sonuna / başına kadar sil |
| Ctrl+W, Ctrl+Backspace, Alt+Backspace | önceki kelimeyi sil |
| Alt+D, Ctrl+Delete | sonraki kelimeyi sil |
| Alt+F/B, Alt+→/←, Ctrl+→/← | kelime ileri / geri |
| Shift+ok, Shift+Home/End | seçim |
| Ctrl+- / Ctrl+. | geri al / yinele |
| Backspace / Delete | karakter sil |
| Ctrl+C / Ctrl+D | `Action::Exit` (usta davranışı korunur; opencode'daki "ctrl+c = temizle" alınmaz) |

**History kuralı (opencode `PromptHistory.move`):** ↑ ile geri gidilirken mevcut metin boş değilse
ve history'deki o kayda eşit değilse hareket yapılmaz — düzenlenmiş metin ezilmez. Geri dönüşte
stash (yazılmakta olan metin) bugünkü gibi geri gelir. Recall edilen metinde imleç sona konur.
History hâlâ bellek içi (kalıcılık kapsam dışı).

### 3. Yükseklik: büyüyen kutu

- Kutu = üst `─` kural + N metin satırı; N = görsel satır sayısı, **min 1, max 8**; 8'den sonra
  kutu içinde kayar (imleç görünür kalır). Viewport yüksekliği = `1 + N + 1` (kural + metin + durum),
  yani 3..10. Durum satırı daima kutunun hemen altında.
- Görsel satır = kelime sınırında sarma (`width - 2`, `> ` / `  ` öneki düşülür); uzun tek kelime
  karakterden bölünür. Genişlik `unicode-width`.
- `VIEWPORT_H` sabiti kalkar → `term::viewport_height(editor, width)`.
- **Yükseklik değişimi:** ratatui 0.30'da inline viewport yüksekliğini değiştiren public API yok
  (`Terminal::resize` sadece terminal boyutuna bakar). Yükseklik değişince `Tui::set_height(h)`:
  eski viewport bölgesi temizlenir, `Terminal` eski viewport'un üst satırı seed'iyle yeni
  `Viewport::Inline(h)` ile yeniden kurulur (`TrackedBackend` aynı kalır, CPR sorgusu yok).
  Büyürken ratatui'nin `compute_inline_size`'ı gerekirse ekranı yukarı kaydırır.
- **Ghost-frame koruması:** v0.29.1 dersi — yeniden kurulumun bıraktığı satır kalmamalı.
  TestBackend testleri: büyü → küçül → büyü sonrası viewport dışında kutu kalıntısı yok; terminal
  resize ile yükseklik değişimi art arda geldiğinde de aynı.

### 4. Dokunulmayanlar

Paste davranışı (`insert_str`, CRLF→LF, Enter'a dönüşmez), kitty protokolü, bracketed paste,
status satırı içeriği, tema renkleri, `> ` / `  ` öneki, submit değeri (trim), locked mod.

## Test

`editor.rs` unit (önce kırmızı):
1. Shift+Enter / Ctrl+J yeni satır; Enter gönderir ve sıfırlar; boş Enter → `None`.
2. ↑ çok satırlı metinde 2. satırdayken imleci 1. satıra taşır, history'ye gitmez; 1. satırda
   history'ye gider.
3. Düzenlenmiş recall üstünde ↑ hareket etmez.
4. Ctrl+A/E/K/U/W, Alt+F/B, Ctrl+- / Ctrl+. beklenen değeri üretir.
5. `desired_height`: 1 satır → 1; 3 satır → 3; 20 satır → 8; kelime sarması "aaa bbb" dar alanda
   kelimeden bölünür; Türkçe/geniş karakterde imleç sütunu doğru.
6. Render: kural satırı 0, `> ` öneki, imleç konumu.

`term.rs`/`page.rs` (TestBackend):
7. `set_height` büyüme/küçülme sonrası ekran tamponunda eski kutu kalıntısı yok.
8. Mevcut resize pin testleri geçer.

## Doğrulama

`cargo test`, `cargo clippy -- -D warnings`, ardından Anil elle: uzun mesaj yaz, Shift+Enter,
↑/↓, pencereyi daralt/genişlet.

## Branch temizliği (Anil isteği)

Merge sonrası: `multiline-input` branch silinir, `cargo clean` ile `target/` silinir (Rust build
çıktısı GB'larca yer tutuyor). Ayrıca merge edilmiş eski yerel branch'ler listelenir, Anil onayıyla
silinir.

## Kapsam dışı

Paste özeti (`[Pasted ~N lines]`), kalıcı history, `/` popup, `@` mention, harici editör.
