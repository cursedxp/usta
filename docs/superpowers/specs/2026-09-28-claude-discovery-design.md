# `claude`'u her işletim sisteminde kendi bulsun

**Tarih:** 2026-09-28 · **Kapsam:** `src/backend.rs` (keşif + çağrı), dağıtım notları

## Sorun

usta, CLI backend'i seçmek için `claude_on_path()` ile PATH'te **uzantısız** `claude` dosyası arıyor
ve `Command::new("claude")` ile çağırıyor. Sonuç:

- **Windows:** dosya `claude.exe` (resmi yükleyici) veya `claude.cmd` (npm) → hiç bulunmuyor.
  Kullanıcıya `setx USTA_BACKEND "cli"` yazdırmak zorunda kalıyoruz (README + zip notları).
- **Her OS:** `claude` kuruluysa ama o terminalin PATH'inde değilse (yeni kurulum, GUI'den açılan
  terminal, wizard'da "Enter = tekrar dene") bulunmuyor — kurulum klasörüne hiç bakılmıyor.

## Tasarım

### Keşif — `find_claude() -> Option<PathBuf>`

Sırayla, ilk bulunan dosya kazanır:

1. **`USTA_CLAUDE`** ortam değişkeni (tam yol) — dosyaysa. Kaçış kapısı; README'de bir satır.
2. **PATH**, işletim sistemine göre adlarla: Unix `claude`; Windows `claude.exe`, sonra `claude.cmd`
   (exe önce — `.cmd` çağrısı daha kırılgan, aşağıda).
3. **Bilinen kurulum klasörleri** (aynı adlarla):
   - `~/.local/bin` — Claude Code resmi (native) yükleyicisi, her OS
   - `~/.claude/local` — eski "local install"
   - Windows: `%APPDATA%\npm` (`dirs::data_dir()`) — npm global
   - macOS/Linux: `/opt/homebrew/bin`, `/usr/local/bin` — PATH'i eksik terminaller için

Saf çekirdek test edilebilir olsun: `find_claude_in(explicit, path_var, known_dirs, names)` — gerçek
ortamı okuyan ince sarmalayıcı `find_claude()`. `claude_on_path()` silinir; `select()` →
`find_claude().is_some()`.

`USTA_BACKEND=cli` verilmiş ama `claude` bulunamıyorsa: bugünkü gibi CLI seçilir, çağrı `claude`
adıyla denenir (davranış korunur; hata çağrıda görünür).

### Çağrı — bulunan yol kullanılır

`Backend::Cli`'ye `bin: PathBuf` alanı eklenir; `select()` / wizard doldurur (bulunamadıysa
`PathBuf::from("claude")`). `run_claude_cli` `Command::new(bin)` ile çağırır.

**`.cmd` (npm, Windows):** Rust, `.cmd`/`.bat` çağrısında argümanları `cmd.exe` kurallarına göre
kaçışlar, kaçışlayamadığını reddeder. usta'nın sistem talimatı uzun ve çok satırlı — satır sonları
batch argümanında güvenli değil. Bu yüzden **`bin` `.cmd`/`.bat` ile bitiyorsa** sistem talimatı
argüman yerine geçici dosyadan verilir: `--append-system-prompt-file <tmp>` (Claude Code 2.1.x'te
var, doğrulandı). Dosya çağrı bitince silinir. `.exe` ve Unix yolu **değişmez** (bugünkü
`--append-system-prompt <metin>`, macOS'ta kanıtlı).

Kullanıcı girdisi zaten stdin'den gidiyor; diğer argümanlar (`-p`, model adı, araç listesi,
`--setting-sources ""`, session id) tek satır ve güvenli.

### Dağıtım notları

`packaging/windows/README.txt` + `OKUBENI.txt` ve README'nin Windows paragrafı: `USTA_BACKEND` şartı
kalkar → "Claude Code kuruluysa kendiliğinden bulunur; bulunamazsa `setx USTA_CLAUDE "C:\…\claude.exe"`".

## Test

- `find_claude_in` (tempdir'lerle): explicit yol kazanır · PATH'te bulunur · PATH'te yoksa bilinen
  klasörde bulunur · ad sırası (`claude.exe` > `claude.cmd`) · hiçbir yerde yok → `None` · klasör
  adında `claude` olan dizin dosya sayılmaz.
- `.cmd` kararı saf fonksiyon: `needs_prompt_file(&Path) -> bool` (`.cmd`/`.bat`, büyük/küçük harf
  duyarsız) → true; `.exe`, uzantısız → false.
- Mevcut testler (ör. `file_feedback.rs`'teki `Backend::Cli { .. }` kurulumu) yeni alana uyarlanır.

## Doğrulama

- `cargo test` (bilinen `materials::…pdftotext` hariç), clippy, fmt sadece düzenlenen dosyalar.
- **Mac (Claude ile):** `usta` normal açılıyor, cevap geliyor (yol değişmedi).
- **Windows (bağlayıcı, Anil'in eşi):** `USTA_BACKEND` **silinmiş** hâlde (`reg delete HKCU\Environment
  /v USTA_BACKEND /f` + terminali yeniden aç) `usta.exe start` → CLI backend bulunuyor, cevap geliyor.
  `.cmd` (npm) kurulumunda da denenirse ayrıca not edilir. **Doğrulanmadı:** `.cmd` yolu yalnız
  Windows'ta test edilebilir.

## Sürüm

v0.31.4 — bump + SPEC §6 selection satırı + ROADMAP + Windows/macOS paketleri + GitHub Release.
