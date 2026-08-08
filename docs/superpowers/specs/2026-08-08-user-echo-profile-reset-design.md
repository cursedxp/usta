# Spec — Kullanıcı Echo Görünürlüğü + Profil Yaşam Döngüsü (reset · tanıma · canlı profil)

**Tarih:** 2026-08-08
**Durum:** Onaylandı (Anil — canlı geri bildirim; tanıma+canlı profil eklemesi aynı gün)
**Taban:** `5b16897` sonrası main.

## 1. Sorunlar

### S1 — Kullanıcının yazdıkları scrollback'te görünmüyor

Kullanıcı raporu: "Benim yazdığım yazılar çıkmıyor, sadece onun yazdıklarını görebiliyorum."

Kök neden (`src/tui/run.rs:466`): gönderilen satır `\x1b[2m│ > {line}\x1b[0m` (DIM) ile basılıyor — birçok terminal temasında dim metin zemine karışıp fiilen görünmez. Ek eksikler:
- Konu girişinde yazılan satır HİÇ echo edilmiyor (yalnız "konu: X" notice'ı var).
- Çok satırlı gönderimde (yapıştırma) `│ > ` öneki yalnız ilk satırda; devam satırları çıplak.

### S2 — Profil sıfırlanamıyor

Usta `learner/profile.md`'yi (global, tüm konularda paylaşılan kullanıcı profili) system prompt'a yükleyip kullanıcıyı "tanıyarak" cevap veriyor. Kullanıcı bu bilgiyi silmek/sıfırlamak istediğinde bir yol yok — hem test amaçlı ("tanımadan nasıl davranıyor?") hem kullanıcı hakkı olarak ("isteyen profilini iptal edebilmeli"). `usta reset <konu>` ve `usta reset --factory` var; profil hedefi yok.

### S3 — Profil hiç dolmuyor: Usta kullanıcıyı tanımaya çalışmıyor

Profil (`learner/profile.md`, global) yalnız ELLE düzenlenirse değişir: tanışma akışı konuyu sorar, kullanıcıyı sormaz; kapanış flush'ı progress/approach/curriculum yazar, profile dokunmaz. Sonuç: sıfırdan kuran (veya `reset --profile` yapan) herkes için Usta sonsuza dek jenerik profille çalışır — "seni tanıyarak destek ayarlama" vaadi (USTA.md Persona + Anlatım Dili) boşta kalır. Ç2'deki reset bu boşluğu görünür kılar: sıfırlayınca geri dolma yolu olmalı.

## 2. Çözümler

### Ç1 — Kullanıcı echo'su: belirgin ve her yerde

**Görsel dil:** Usta bloğu `●` (turuncu) + markdown ise, kullanıcı bloğu şu olur:

```
❯ kullanıcının yazdığı metin
  devam satırı (çok satırlıysa, 2 boşluk girinti)
```

- `❯` turuncu (208), **metin NORMAL renk — DIM YOK** (görünürlük sorununun kökü).
- Çok satırlı gönderimde ilk satır `❯ `, sonraki satırlar 2 boşluk girinti (satır sonları korunuyor — yapıştırma yapısı görünür).
- Üstünde tek boş satır (Usta bloğu ritmiyle aynı).

**Kapsam — echo basılan yerler:**
1. Ana döngü Submit (mevcut yer — stil değişir).
2. Konu girişi: kullanıcı konu yazıp gönderince, notice'lardan önce aynı formatta echo.
3. Onay cevapları (tek tuş) echo EDİLMEZ — kapsam dışı (görsel gürültü).

**Uygulama biçimi:** Saf yardımcı `user_echo_text(line: &str) -> ratatui::text::Text<'static>` (test edilebilir: önek, girinti, renk-modifier'ında DIM olmadığı) + `page_user_echo(tui, line)` sarmalayıcı. Mevcut `\x1b[2m│ > ` format satırı kalkar.

### Ç2 — `usta reset --profile`

- `parse_command`: `usta reset --profile` → `ResetTarget::Profile` (mevcut `--factory` kalıbıyla tutarlı; `--profil` yazımı da kabul edilir — Türkçe alışkanlık toleransı).
- Davranış: onay sorusu → global `learner/profile.md` gömülü jenerik şablonla (defaults.rs'teki kişiliksiz default) ÜZERİNE yazılır. Öncesinde tek kopya yedek: `learner/profile.md.bak` (yanlışlıkla sıfırlayan geri alabilsin — `write_atomic` zaten bu deseni kullanıyor).
- Onay metni: `"Profil sıfırlanacak — Usta seni tanımadan başlayacak (yedek: profile.md.bak). Devam? [e/H]"`.
- Konu progress'lerine DOKUNULMAZ — yalnız profil. (Konular için `reset <konu>` / `--factory` zaten var.)
- Kullanım satırı güncellenir: `usta reset <konu> | --factory | --profile`.
- LLM gerekmez; TUI dışı tek-atımlık komut (mevcut reset'ler gibi).

### Ç3 — Profil yaşam döngüsü: tanı → kullan → güncelle → (istenirse) sıfırla

#### Ç3a — İlk tanışma (profil jenerikken)

- **Tespit:** `profile_is_generic(disk_içeriği)` — global `learner/profile.md` içeriği gömülü jenerik şablonla (defaults) AYNI ise profil "boş" sayılır. Elle doldurulmuş profil şablondan farklıdır → tanışma tetiklenmez.
- **Davranış:** Profil boşken açılış turn'lerine (hem yeni-konu tanışması hem devam drilli) koşullu blok eklenir:

  > "[PROFİL BOŞ] Kullanıcıyı henüz tanımıyorsun. Sohbetin başında kısaca tanış — adı, bu alanla geçmişi, nasıl öğrenmeyi sevdiği. En fazla 1-2 soru, form değil; konuya girmeyi geciktirme. Öğrendiklerin oturum kapanışında profiline yazılacak."

- Anlatım Dili kurallarıyla uyumlu: jargonsuz, kısa. Konu-intro'su varsa (kullanıcı zaten kendini anlatmışsa) model tekrar sormaz — mevcut "tekrar sorma" talimatı geçerli.

#### Ç3b — Profil canlı belge (kapanışta güncelleme)

- Kapanış flush'ına 4. dosya eklenir: `===DOSYA: profile===` — hedef **GLOBAL** `learner/profile.md` (progress/approach/curriculum proje-yerel; profile farklı köke yazılır — mevcut "bilinmeyen dosya adı atlanır" güvenliği korunur, yalnız `profile` adı global yola eşlenir).
- **Üretim kuralları (kapanış promptuna eklenecek):**
  - YALNIZ kişi hakkında kalıcı gözlem: ad, geçmiş/deneyim, öğrenme tarzı, tercihler, tekrarlayan güçlü/zayıf yönler.
  - KONU BİLGİSİ YAZILMAZ (o progress'in işi) — "Rust'ta ownership öğrendi" profile GİRMEZ; "örnek üzerinden öğrenmeyi seviyor" GİRER.
  - Yalnız YENİ/DEĞİŞEN bilgi varsa üretilir (her kapanışta değil); mevcut profildeki geçerli bilgi korunur (kullanıcı elle düzenlemiş olabilir — ezme).
  - Kısa tut: ~1 sayfa tavan; eskiyen/yinelenen satırları birleştir.
- Kapanış promptuna mevcut profil içeriği de eklenir (model korusun/güncellesin diye) — `closing_prompt` imzası genişler.
- Yazım `write_atomic` ile (`.bak` yedeği bedava geliyor). Defaults sahipliğiyle çelişki yok: profile `User`-owned — kod senkronu ezmez, LLM/kabuk yazabilir.
- **Döngü tamamlanır:** kullandıkça tanır → `reset --profile` unutturur → ilk tanışma yeniden doldurur.

## 3. Kapsam Dışı

- Profilin oturum içinden "unut şunu" tarzı seçici düzenlenmesi (tam sıfırlama var; seçici unutma ileride).
- Onay cevaplarının echo'su.
- Plain yolda echo değişikliği (plain'de rustyline yazılanı zaten ekranda bırakır — sorun TUI'ye özgü).
- Konu-üstü bilgi köprüsü (ayrı fikir — havuzda bekliyor; profile KONU bilgisi taşımak bu spec'te açıkça yasak).

## 4. Test Stratejisi

- `user_echo_text`: ilk satır `❯ ` öneki + turuncu span; metin span'ında DIM modifier YOK; çok satırlıda devam girintisi. Unit test.
- `parse_command`: `reset --profile` ve `reset --profil` → `ResetTarget::Profile`; mevcut `reset <konu>`/`--factory` regresyonu.
- Profil reset mantığı: `run_reset_profile(global: &Path)` yol-parametreli saf çekirdek — temp dizinde: dolu profil → reset → içerik jenerik şablona eşit + `.bak` eski içeriği taşıyor. Onay katmanı elle doğrulamada.
- `profile_is_generic`: gömülü şablonla birebir → true; tek karakter fark → false. Unit test.
- Açılış promptları: profil boşken `[PROFİL BOŞ]` bloğu var + "1-2 soru" + "kapanışta profiline yazılacak"; doluyken blok YOK. Hem onboarding hem opening için test.
- `closing_prompt`: profile bölümü — "KONU BİLGİSİ YAZILMAZ" + "yalnız değiştiyse" + mevcut profil içeriği gömülü. `split_files` zaten ad-bazlı (regresyon testi mevcut); `profile` adının GLOBAL yola eşlendiği flush testi (temp global+proje kökleriyle).
- Mevcut 155+ test yeşil.

## 5. Elle Doğrulama

1. `usta` oturumunda mesaj yaz → gönderdiğin metin scrollback'te NET görünür (dim değil), Usta yanıtının üstünde.
2. Çok satırlı yapıştır + gönder → tüm satırlar girintili görünür.
3. Konu girişinde cümle yaz → cümlen echo'lanır, sonra "konu: …" notice'ı gelir.
4. `usta reset --profile` → onay → `~/.config/usta/learner/profile.md` jenerik; `.bak` eski hali; sonraki oturumda Usta isimsiz/tanımadan selamlar.
5. `usta reset --profile` → `h` → hiçbir dosya değişmez.
6. `usta reset <konu>` ve `--factory` eski davranışında (regresyon).
7. Reset sonrası yeni oturum → Usta konuya girmeden kısaca tanışıyor (ad + öğrenme tarzı, 1-2 soru); cevap ver → `/quit` → `~/.config/usta/learner/profile.md` dolmuş (ad + gözlemler), KONU bilgisi içermiyor.
8. Dolu profille yeni oturum → tanışma sorusu YOK, Usta isimle/tarza göre davranıyor.
9. Profili elle düzenle → sonraki kapanış elle yazılanı EZMİYOR (koruma kuralı).

## 6. Başarı Ölçütü

Elle doğrulama 1-9 geçer; döngü çalışır: Usta kullanıcıyı tanır → kullandıkça profili günceller → kullanıcı tek komutla unutturabilir → tekrar tanışır. Kullanıcı kendi mesajlarını her zaman görür.
