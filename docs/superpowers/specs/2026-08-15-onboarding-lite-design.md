# Tasarım — Onboarding-Lite: Backend Sihirbazı + Sürüm Hizalama (Roadmap #4'ün ilk yarısı)

**Tarih:** 2026-08-15
**Kapsam:** (a) Backend bulunamayınca çıplak hatayla ölmek yerine yönlendiren hafif ilk-çalıştırma sihirbazı; (b) Cargo.toml sürümünün SPEC ile hizalanması (0.1.0 → 0.13.0) + sürümleme politikası.
**Durum:** Onaylandı (Anil, 2026-08-15: dağıtım kanalı ertelendi — "şimdilik atla"; hafif sihirbaz + 0.13.0 hizası seçildi) → writing-plans
**Bağımlılık:** Yok.

## Amaç

Bugün `backend::select()` (`src/backend.rs:46`) backend bulamayınca `bail!` ile ölüyor — yeni kullanıcı ne yapacağını hata metninden söküyor. "Herkes kullansın" yolunun ilk yarısı: hata yerine adım adım yönlendirme, kurulum tamamlanınca **aynı süreçte devam**. İkinci yarı (prebuilt binary/brew/CI) bilinçli ERTELENDİ — roadmap'te bekliyor.

Kararlar:
- **Hafif sihirbaz** (Anil seçimi): yalnız backend-yok durumunda devreye girer; mevcut otomatik seçim (CLI varsa CLI, yoksa API key) aynen korunur. Tam-sihirbaz (dil/isim akışı) YOK — tanışma Usta'nın işi, çifte onboarding olmaz.
- **Plain stdin/stdout akışı** — TUI açılmadan önce çalışır (backend, TUI'den önce seçiliyor; ratatui'ye gerek yok, `ui::` yardımcılarıyla düz satır).
- **TTY yoksa davranış değişmez:** mevcut `bail!` mesajı kalır (pipe/CI sihirbaza takılmaz).
- **API key girişi süreç-içi:** kullanıcı sihirbaza key yapıştırabilir → yalnız süreç env'ine yazılır (`std::env::set_var`), DİSKE YAZILMAZ; kalıcılaştırma kullanıcıya söylenir ("add to your shell profile"). Girilen key ekrana geri yazdırılmaz.
- **Sürüm:** Cargo.toml `0.13.0` (SPEC §4.13 ile hizalı). Politika: her tamamlanan roadmap maddesi minor bump (ROADMAP notu). `v0.13.0` git tag'i atılır (ileride release'lerin çıpası; release workflow'u yok — ertelendi).

## Davranış

### Sihirbaz akışı (`src/backend.rs` + `src/main.rs:60`)

`main`'de `backend::select()` `Err` dönerse VE stdout+stdin TTY ise (`std::io::IsTerminal`):

```
No LLM backend found. Usta needs one of these:

  1. Claude Code CLI (recommended — uses your subscription, no API key)
     Install: https://claude.com/claude-code   (then just press Enter here)

  2. Anthropic API key
     Paste it below (starts with sk-ant-...), or add to your shell first:
     export ANTHROPIC_API_KEY=sk-ant-...

Press Enter to re-check · paste your API key · or type q to quit
> 
```

Girdi yorumu (saf fonksiyon `wizard_action`):
- boş satır → **Recheck**: `select()` yeniden denenir; başarılıysa "backend found: <label>" + normal akış devam; değilse aynı prompt tekrar (sonsuz döngü değil sıkıcı döngü — kullanıcı `q` ile çıkar).
- `q` / `quit` (case-insensitive) → **Quit**: sihirbaz mesajıyla temiz çıkış (exit code 1).
- `sk-ant-` ile başlayan satır → **Key**: trim'lenir, `ANTHROPIC_API_KEY` süreç env'ine yazılır, `select()` yeniden denenir (API yolu artık bulur). Başarı mesajının yanında tek satır kalıcılaştırma hatırlatması: "tip: add `export ANTHROPIC_API_KEY=...` to your shell profile to skip this next time". Key değeri hiçbir çıktıya yazılmaz.
- Diğer her girdi → kısa uyarı + aynı prompt.

TTY değilse: mevcut `bail!` (backend.rs:62-66) aynen.

`USTA_BACKEND` zorlaması geçersiz değere sahipse (`bail!` satır 50) sihirbaz DEVREYE GİRMEZ — o bir konfigürasyon hatası, eksik-backend değil.

### Sürüm (`Cargo.toml` + tag)

- `version = "0.13.0"`; welcome kutusu `env!("CARGO_PKG_VERSION")` ile zaten otomatik gösterir.
- ROADMAP başlığının altına tek satır politika notu: "Sürümleme: her tamamlanan roadmap maddesi minor bump (SPEC §'ü ile hizalı); tag `vX.Y.Z`."
- İş sonunda `git tag v0.13.0` + `git push --tags`.

## Test

- `wizard_action("")` → Recheck; `"q"`/`"Q"`/`" quit "` → Quit; `"sk-ant-abc123"` → Key("sk-ant-abc123") (trim'li); `"garbage"` → Invalid.
- `wizard_guidance()` metni: `claude.com/claude-code`, `ANTHROPIC_API_KEY`, `sk-ant-`, `q to quit` içerir; key değeri formatlamada YOK (statik metin zaten).
- Key akışı: `set_var` sonrası `select()` API yolu döner (test: env korumalı — mevcut `resolve_key` test desenindeki env hijyenine uy; paralel test env çakışmasına dikkat, gerekirse `#[serial]` yerine tek testte birleşik senaryo).
- TTY-değil yolu: mevcut `bail!` davranışı korunur (kod incelemesi — TTY simülasyonu unit test edilmez).
- Sürüm: `CARGO_PKG_VERSION == "0.13.0"` assert'i (defaults/welcome testine tek satır).

## Kapsam dışı

- Prebuilt binary, GitHub Releases, Homebrew tap, CI release workflow — ERTELENDİ (roadmap #4'te "dağıtım" olarak bekler).
- API key'in diske/keychain'e kalıcılaştırılması.
- Tam sihirbaz (dil/isim/tanışma akışı).
- Model seçimi sihirbazı.

## Açık sorular

Yok.
