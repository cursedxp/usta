# Tasarım — İngilizce Protokol Token'ları (v0.20.0)

**Tarih:** 2026-08-16
**Kapsam:** Kabuğun parse ettiği/yazdığı tüm iç protokol token'ları (bölüm başlıkları, harita durumları, format işaretçileri) Türkçe'den İngilizce'ye geçer + mevcut kullanıcı verisi için tek seferlik deterministik migration. Kullanıcıya görünen dil davranışı değişmez.
**Durum:** Onaylandı (Anil, 2026-08-16) → writing-plans
**İlke (bağlayıcı):** Claude Code modeli — iç makine token'ları her zaman İngilizce, kullanıcıyla konuşma dili SOUL.md dil kilidine göre serbest. Token sayımı/parse güvenilirliği dil-bağımsız olur; İngilizce token LLM için de daha stabil.

## 1. Token haritası (bağlayıcı — tek kaynak)

| # | Türkçe (eski) | İngilizce (yeni) | Parse/yazım yerleri (bilinen) |
|---|---|---|---|
| T1 | `görülmedi` | `not seen` | history.rs:182, main.rs:2164, tui/welcome.rs (sayım), TEACHING.md kuralları |
| T2 | `görüldü` | `seen` | aynı |
| T3 | `oturdu` | `settled` | aynı |
| T4 | `derinleşildi` | `deepened` | aynı |
| T5 | `## Kayıtlar` | `## Records` | index.rs:11,146 (katalog) |
| T6 | `## Hedef` | `## Goal` | brain.rs:104,114; main.rs:1042; progress.rs (çok yer) |
| T7 | `## Hedef Durumu` | `## Goal Status` | progress.rs:492 civarı; GOAL.md kuralları |
| T8 | `## Tercihler` | `## Preferences` | main.rs:991 (set_game_pref), brain.rs (gamification koşullu yükleme) |
| T9 | `Kapatılanlar` | `Retired` | progress.rs:102–136,411; TEACHING.md:87 |
| T10 | `Geri çağırma soruları` | `Recall questions` | tui/welcome.rs:86–99,518,524; progress.rs |
| T11 | `Açık egzersiz` | `Open exercise` | TEACHING.md:87; progress/welcome tarafı |
| T12 | `===DOSYA:` | `===FILE:` | progress.rs:449,534 (kapanış flush ayracı) |
| T13 | `[ARA KAYIT]` | `[CHECKPOINT]` | session.rs:95–106 (yarım oturum kaydı) |
| T14 | `- kaynak:` | `- source:` | progress.rs:133,135,293,581,633–634 (müfredat kaynak referansı) |
| T15 | `genel` (default konu slug'ı) | `general` (yalnız yeni emit) | main.rs:793,810,827; kabul tarafı zaten iki dilli (main.rs:1074) |

Not: `## Hedef Durumu` `## Hedef`ten ÖNCE eşlenmeli (prefix çakışması); migration ve parser sıralaması buna göre.

**Dokunulmayanlar (bilinçli):**
- `evet/hayır/e/h` kullanıcı girdisi kabulü — UI, protokol değil; `yes/no` zaten kabul. Aynen kalır.
- `Çırak → Usta` seviye adları — marka/kimlik (Anil kararı, 2026-08-16). GAMIFICATION.md'de kalır.
- Konu slug'ları ve mevcut dizin adları (`genel` dahil) — kullanıcı verisi; rename edilmez, kabul tarafı `genel`i tanımaya devam eder.
- `mentor/` dosyaları (PROJECT.md, PROGRESS.md) — serbest metin, protokol token'ı yok; migration dokunmaz.
- `due:` / `ivl:` kuyrukları — zaten İngilizce.
- `[YENİ KONU — TANIŞMA]` — kodda yok (SPEC kalıntısıydı); kapsam dışı.

## 2. `src/tokens.rs` — tek kaynak modülü

- Yeni modül: tüm protokol token'ları `pub const` olarak burada (`MAP_STATE_SETTLED`, `H_PREFERENCES`, `FILE_DIVIDER` …).
- Bugün literaller ~7 dosyaya saçılmış (brain, main, index, history, progress, session, tui/welcome). Hepsi bu modülden okur — gelecekte token değişimi tek nokta. (Roadmap #10'un yapısal ön hazırlığı — verdict kanalı da buraya token ekleyecek.)
- Eski Türkçe token'lar da `mod migration` altında sabit olarak yaşar — YALNIZ migration kodu kullanır; parser'lar asla.

## 3. Brain dosyaları (embedded md)

- TEACHING.md: harita durum adları (T1–T4), `Retired` (T9), `Open exercise` (T11) — kural metinleri İngilizce token'a döner.
- GOAL.md: `## Goal` / `## Goal Status` / ölçüm log formatı.
- USTA.md, SOUL.md: `## Hedef` referansları.
- learner/index.md şablonu: `## Records`.
- approaches/ şablonları: varsa `## Hedef` iskeleti.
- SPEC.md + README: "Türkçe kaldı" glos ve parantezleri temizlenir; doc = runtime, ayrışma biter (harita durumları zaten İngilizce anlatılıyordu).

## 4. Migration — tek seferlik, deterministik, kabukta

**Ne zaman:** Her komut girişinde (start/topics/stats/reset), dosya okumalarından ÖNCE `migrate_tokens()`. LLM görmez, prompt'a girmez.

**Nereye bakar:** global root (`~/.config/usta/`): USER.md, learner/*.md, approaches/*.md · proje `.usta/`: learner/*, approaches/*, sessions/ altı yarım kayıtlar (T13 içerebilir).

**Bağlam-kilitli kurallar (serbest metin korunur — "oturdu" kelimesi cümle içinde geçebilir):**
- Başlık token'ları (T5–T8): yalnız satır başında, tam satır eşleşmesi (`^## Hedef Durumu\s*$` önce, sonra `^## Hedef\s*$` …).
- Harita durumları (T1–T4): yalnız harita satırı formatında — `- <madde>: <durum>` satır sonunda veya `| due:` kuyruğu öncesinde.
- `Kapatılanlar` (T9): yalnız başlık/bölüm etiketi konumunda.
- `===DOSYA:` (T12), `[ARA KAYIT]` (T13), `- kaynak:` (T14): kendi kalıpları zaten benzersiz.
- `Geri çağırma soruları` / `Açık egzersiz` (T10–T11): yalnız başlık konumunda.

**Güvenlik:** dosya başına atomik yazım (temp + rename); dosya ilk kez değişiyorsa yanına `.bak` (varsa üzerine yazılmaz — ilk hal korunur). İdempotent: ikinci koşuda no-op, maliyet birkaç dosya okuması. Değişen dosya olduysa tek satır bilgi: `· migrated N file(s) to English protocol tokens (backup: .bak)`.

**Kapsam dışı:** dual-read yok — parser'lar migration sonrası YALNIZ İngilizce token bilir. Eski binary + yeni dosya kombinasyonu desteklenmez (tek kullanıcı gerçekliği; sürüm notuna yazılır).

## 5. Test

- Mevcut 319 test: Türkçe token fixture'ları İngilizce'ye güncellenir (davranış değişmez, sadece token).
- Yeni migration testleri: (a) karışık gerçekçi dosya tam dönüşüm, (b) idempotens (ikinci koşu no-op), (c) prose-koruma ("oturdu" cümle içinde — dokunulmaz), (d) `## Hedef Durumu`/`## Hedef` prefix sırası, (e) `.bak` ilk-hal koruması, (f) atomiklik (temp+rename yolu).
- Kapanışta black-box tur: izole HOME'da Türkçe-token'lı sahte veri → migration → oturum aç/kapa → dosyalar İngilizce, davranış aynı.

## 6. Sürüm ve kayıt

- v0.20.0, tag. Roadmap'e madde: "English protocol tokens" (bu spec'e link), Completed bölümüne özet.
- Canlı veri: `~/.config/usta/` + `~/Documents/Work/Practice/stagit/.usta/` ilk açılışta migrate olur — Anil tarafında elle iş yok.
