# Link okuma — WebFetch / web_fetch izni

**Tarih:** 2026-09-26 · **Kapsam:** iki backend'in araç listesi (`src/backend.rs`, `src/anthropic.rs`)

## Sorun

Anil usta'ya bir site verip "şuna bak" dediğinde model "erişim izni gelmiyor" diyor.

**Kök neden (doğrulandı, 2026-09-26):** CLI backend `claude -p`'yi `--allowedTools WebSearch` ile
çağırıyor. Model link'i açmak için `WebFetch` istiyor; `-p` modunda izin penceresi yok → istek
sessizce reddediliyor, model bunu "izin yok" diye aktarıyor. Birebir tekrar:

| `--allowedTools` | Sonuç (`https://example.com`, h1 sorusu) | `permission_denials` |
|---|---|---|
| `WebSearch` | "Bu oturumda web sayfasına erişim için gerekli izinler verilmemiş…" | `WebFetch`, `Bash` |
| `WebSearch,WebFetch` | "Example Domain" | yok |

API backend'de de aynı boşluk var: istekte sadece `web_search_20260209` aracı tanımlı; sayfa okuma
aracı yok.

## Tasarım

WebFetch salt-okuma bir araç — Hard Rule 1'i ("Usta dosyaya dokunmaz") bozmaz; Hard Rule 2'yi
("uydurma, araştır") güçlendirir. `Bash`/`Edit`/`Write` kapalı kalır.

**CLI (`src/backend.rs`):**
- `--allowedTools` değeri `"WebSearch"` → `"WebSearch,WebFetch"` (virgüllü tek argüman — yukarıdaki
  tabloda gerçek `claude` ile doğrulandı).
- Değer bir modül sabitine çıkar (`CLI_ALLOWED_TOOLS`) ki test edilebilsin; test: `WebSearch` ve
  `WebFetch` var, `Bash`/`Edit`/`Write` yok.
- Modül doc comment'i (`--allowedTools WebSearch both enables research…`) güncellenir.

**API (`src/anthropic.rs`):**
- `MessageRequest::new` araç listesine ikinci eleman: `type: "web_fetch_20260209"`, `name: "web_fetch"`
  (claude-api skill'i: `_20260209` web fetch, `claude-opus-4-8` destekli, beta başlığı yok; sadece
  konuşmada geçen URL'leri çeker).
- `used_web_search` sonuç blok tiplerine `web_fetch_tool_result` eklenir (UI "web kullanıldı"
  ipucu link okumada da yansın; `server_tool_use` zaten yakalıyor, bu kesinlik için).
- `pause_turn` döngüsü değişmez — web_fetch aynı server-tool mekanizması.

**Dokunulmayanlar:** backend seçimi, `--setting-sources ""`, model, system prompt/brain dosyaları,
TUI.

## Test

- CLI: `CLI_ALLOWED_TOOLS` sabiti testi (yukarıda).
- API: `request_serializes_expected_shape` genişler — `tools[1].type == "web_fetch_20260209"`,
  `tools[1].name == "web_fetch"`; `used_web_search` testi `web_fetch_tool_result` bloğunu da tanır.
- Önce kırmızı, sonra değişiklik.

## Doğrulama

- `cargo test` yeşil (bilinen `materials::…pdftotext` hatası hariç), clippy temiz, fmt sadece
  düzenlenen dosyalarda.
- **Elle (bağlayıcı):** `cargo install --path .` → `usta` aç → bir URL ver, "şu sayfaya bak" de →
  sayfa içeriğinden bahsediyor, "izin yok" demiyor.

## Sürüm

Patch bump (v0.31.2 → v0.31.3) + SPEC §6 backend satırı + ROADMAP. `welcome_tests.rs`'teki sürüm
pini de aynı commit'te güncellenir (v0.31.2'de unutulup final review'da yakalandı). Bekleyen
`2026-09-04-rustls-portability` planı buna göre bir sonraki patch'i alır.
