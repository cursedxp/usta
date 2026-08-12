# Tasarım — /help komutu + welcome ipucu

**Tarih:** 2026-08-12
**Kapsam:** Oturum içi `/help` komutu (klavye kısa yolları + slash komutları + CLI komutları) + welcome kutusuna keşif ipucu.
**Durum:** Onaylandı → writing-plans

## Amaç

Kullanıcı kısa yolları (Ctrl+J newline, Esc durdur, /watch, vb.) keşfedemiyor. `/help` oturum içinde hepsini listeler; welcome kutusundaki tek satır ipucu `/help`'in varlığını duyurur.

Kararlar (brainstorm):
- Form: `/help` slash komutu (iki loop) + welcome ipucu satırı.
- İçerik: klavye kısa yolları + slash komutları + CLI komutları.
- Kapsam dışı (YAGNI): CLI `usta help`/`--help` subcommand YOK — `/help` oturum içi zaten CLI komutlarını referans listeler.

## Mimari

Tek kaynak — yeni `src/help.rs`:
- `pub fn help_text() -> &'static str` — İngilizce formatlı yardım bloğu (içerik aşağıda).
- `pub fn is_help_command(line: &str) -> bool` — `line.trim() == "/help"`.
- `pub const HELP_HINT: &str = "Type /help for shortcuts and commands.";`

`src/main.rs` `mod help;` ile modülü ekler.

### Slash intercept (mevcut `/watch` deseniyle birebir — LLM'e GİTMEZ)
- `src/tui/run.rs` `run` ana döngüsü, `Action::Submit(line)` kolunda, `/watch` kontrolünün yanında: `if crate::help::is_help_command(&line)` → `page_user_echo` + help bloğunu `page`/`page_notice` ile bas, `continue`. `session.push_user` YOK.
- `src/main.rs` `run_plain_loop` `InputEvent::Line` kolunda, `parse_watch_command` kontrolünün yanında: `if help::is_help_command(&line)` → `println!("{}", help::help_text())`, `let _ = ready_tx.send(())`, `continue`.

### Welcome ipucu
- `src/tui/welcome.rs` iki render fonksiyonunun (`render_welcome_identity` ve `render_welcome`) altına tek dim satır: `help::HELP_HINT`. İki giriş yolu da (`usta` interaktif ve `usta start <topic>`) ipucunu görür. welcome.rs `crate::help::HELP_HINT`'i kullanır (tek kaynak).

## Yardım içeriği (help_text)

```
Usta — shortcuts & commands

Keyboard
  Enter            send message
  Ctrl+J           new line   (also Shift+Enter / Alt+Enter on modern terminals)
  Esc              stop Usta mid-reply
  Ctrl-C / Ctrl-D  quit
  ↑ / ↓            previous / next message

In-session commands
  /watch on|off    file-feedback companion (on by default)
  /help            this help
  /quit            end the session

Terminal commands
  usta                    start — asks for the topic
  usta start <topic>      start a specific topic
  usta topics             list what you're learning where
  usta reset <topic>      reset a topic's progress in this project
  usta reset --profile    reset only your profile
  usta reset --factory    reset everything
```

## Test

- `is_help_command`: `/help`, ` /help ` → true; `/help me`, `help`, `/quit` → false.
- `help_text()` içerik kontrolü: `"Ctrl+J"`, `"Esc"`, `"/watch on|off"`, `"/quit"`, `"usta reset --factory"` içerir (vacuous olmayan).
- Slash LLM'e gitmez: `/help` gönderildiğinde `session.push_user` çağrılmaz (mevcut `/watch` garantisiyle aynı; kod incelemesiyle doğrulanır — döngü testi yok).

## Kapsam dışı

- CLI `usta help` / `--help` subcommand.
- Kısa yol tuş atamalarını değiştirmek (yalnız belgeleme).
- Kalıcı footer ipucu (welcome tek-sefer ipucu tercih edildi).

## Açık sorular

Yok.
