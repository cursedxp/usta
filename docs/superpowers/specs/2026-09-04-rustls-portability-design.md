# Tasarım — TLS'i rustls'e taşı: Linux'ta sistem bağımlılığı kalmasın (v0.31.2)

**Tarih:** 2026-09-04
**Kapsam:** `reqwest`'in TLS arka ucu `native-tls`'ten `rustls`'e geçer. Tek amaç: usta'nın Linux'ta OpenSSL geliştirme paketleri olmadan kurulabilmesi.
**Durum:** Onaylandı → implement (Anil: "A", 2026-09-04)

## Neden

Anil hem macOS hem Linux kullanıyor ve usta'nın ikisinde de kurulabilmesi gerekiyor. Bugün kurulu binary:

```
Mach-O 64-bit executable arm64
  /System/Library/Frameworks/Security.framework/...
```

TLS'i macOS'ta Apple'ın `Security.framework`'ü yapıyor. Linux'ta aynı iş `openssl-sys`'e düşüyor — `Cargo.lock` bunu doğruluyor (`native-tls 0.2.18`, `openssl-sys 0.9.117`). Sonuç: Linux'ta `cargo install` **derleme hatası** verir, önce `libssl-dev`/`openssl-devel` + `pkg-config` kurulması gerekir. Bu, "iki makinede de çalışsın" hedefinin önündeki tek gerçek engel.

`rustls` TLS'i Rust'ın kendi kütüphanesiyle yapar; C bağımlılığı ve sistem paketi gerektirmez.

## Karar

**T1 — Varsayılan özellikler elle geri konur.** `reqwest` 0.12.28'in `default` listesi (kaynaktan okundu): `default-tls`, `charset`, `http2`, `system-proxy`. Yalnızca `default-features = false` yazmak `http2` ve `charset`'i de düşürür — istek HTTP/1.1'e iner ve yanıt karakter-kümesi çözümü kaybolur. Bunlar istenmedi. Yeni satır:

```toml
reqwest = { version = "0.12", default-features = false, features = [
    "json", "charset", "http2", "system-proxy", "rustls-tls-native-roots",
] }
```

**T2 — Kök sertifika kaynağı DEĞİŞMEZ: `rustls-tls-native-roots`.** `rustls-tls` (= `rustls-tls-webpki-roots`) paketlenmiş Mozilla kök listesini kullanır; bu, bugünkü davranıştan sapmadır — `native-tls` her iki platformda da **işletim sisteminin** güven deposunu okuyor. `rustls-tls-native-roots` o davranışı korur (`rustls-native-certs` ile, OpenSSL'e bağlanmadan). İstenmeyen bir davranış değişikliği yapmamak esastır: kurumsal MITM proxy veya elle eklenmiş kök sertifika arkasındaki bir makinede webpki kökleri sessizce başarısız olurdu.

**T3 — Kapsam yalnızca bu.** Önceden derlenmiş binary, GitHub Releases, CI, musl statik derleme KAPSAM DIŞI (Anil "A" dedi; 2 ve 3 seçenekleri ayrı). Bu değişiklik onları ileride mümkün kılar, ama burada yapılmaz.

**T4 — Durum senkronu ayrı bir iştir.** İki makinede kullanım, `~/.config/usta` ve `<proje>/.usta/` hafızasının bölünmesi demektir. Bilinen ve kayıtlı bir eksik; bu tasarımın konusu DEĞİL.

## Davranış

Kullanıcıya görünen davranış değişmez: aynı istekler, aynı güven deposu, aynı proxy davranışı. Değişen tek şey TLS'in hangi kütüphaneyle yapıldığı.

Kurulum sonrası Linux'ta gereken tek şey bir Rust araç zinciri ve bir C derleyicisi olur; `libssl-dev`/`openssl-devel` **gerekmez**.

## Test

- `cargo tree -i openssl-sys` → **hiçbir şey döndürmemeli** ("package ID specification did not match any packages" beklenen çıktı).
- `cargo tree -i native-tls` → aynı şekilde boş.
- `cargo tree -i rustls` → reqwest üzerinden bağlı görünmeli.
- `cargo test` tümü yeşil (bilinen `pdftotext` ortam hatası hariç), `cargo clippy --all-targets` 0 uyarı.
- `otool -L $(which usta)` çıktısında `Security.framework` **kalmamalı** — kurulum sonrası elle bakılır.
- **Elle doğrulama (Anil, anahtar gerektirir):** `USTA_BACKEND` API yoluna zorlanıp `ANTHROPIC_API_KEY` ile tek bir tur koşulur. TLS'in gerçekten çalıştığının tek gerçek kanıtı budur — birim testleri ağ çağrısı yapmıyor. Varsayılan yol Claude CLI olduğu için bu adım atlanırsa değişiklik **sınanmamış** sayılır.
- **Elle doğrulama (Linux):** temiz bir Linux makinede `libssl-dev` KURMADAN `cargo install --git https://github.com/cursedxp/usta --locked` → derleme başarılı olmalı. Asıl hedef bu.

## Kapsam dışı

- Önceden derlenmiş binary / GitHub Releases / CI (seçenek 2)
- musl statik derleme
- Durum senkronu (seçenek 3, `~/.config/usta` ve `<proje>/.usta/`)
- `reqwest` dışındaki bağımlılıklar
