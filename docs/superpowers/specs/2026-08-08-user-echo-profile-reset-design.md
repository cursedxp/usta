# Spec — Kullanıcı Echo Görünürlüğü + Profil Reset

**Tarih:** 2026-08-08
**Durum:** Onaylandı (Anil — canlı geri bildirim)
**Taban:** `5b16897` sonrası main.

## 1. Sorunlar

### S1 — Kullanıcının yazdıkları scrollback'te görünmüyor

Kullanıcı raporu: "Benim yazdığım yazılar çıkmıyor, sadece onun yazdıklarını görebiliyorum."

Kök neden (`src/tui/run.rs:466`): gönderilen satır `\x1b[2m│ > {line}\x1b[0m` (DIM) ile basılıyor — birçok terminal temasında dim metin zemine karışıp fiilen görünmez. Ek eksikler:
- Konu girişinde yazılan satır HİÇ echo edilmiyor (yalnız "konu: X" notice'ı var).
- Çok satırlı gönderimde (yapıştırma) `│ > ` öneki yalnız ilk satırda; devam satırları çıplak.

### S2 — Profil sıfırlanamıyor

Usta `learner/profile.md`'yi (global, tüm konularda paylaşılan kullanıcı profili) system prompt'a yükleyip kullanıcıyı "tanıyarak" cevap veriyor. Kullanıcı bu bilgiyi silmek/sıfırlamak istediğinde bir yol yok — hem test amaçlı ("tanımadan nasıl davranıyor?") hem kullanıcı hakkı olarak ("isteyen profilini iptal edebilmeli"). `usta reset <konu>` ve `usta reset --factory` var; profil hedefi yok.

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

## 3. Kapsam Dışı

- Profilin oturum içinden düzenlenmesi/"unut şunu" komutu (ileride konu-üstü bilgi köprüsüyle birlikte düşünülür — fikir havuzunda).
- Onay cevaplarının echo'su.
- Plain yolda echo değişikliği (plain'de rustyline yazılanı zaten ekranda bırakır — sorun TUI'ye özgü).

## 4. Test Stratejisi

- `user_echo_text`: ilk satır `❯ ` öneki + turuncu span; metin span'ında DIM modifier YOK; çok satırlıda devam girintisi. Unit test.
- `parse_command`: `reset --profile` ve `reset --profil` → `ResetTarget::Profile`; mevcut `reset <konu>`/`--factory` regresyonu.
- Profil reset mantığı: `run_reset_profile(global: &Path)` yol-parametreli saf çekirdek — temp dizinde: dolu profil → reset → içerik jenerik şablona eşit + `.bak` eski içeriği taşıyor. Onay katmanı elle doğrulamada.
- Mevcut 155+ test yeşil.

## 5. Elle Doğrulama

1. `usta` oturumunda mesaj yaz → gönderdiğin metin scrollback'te NET görünür (dim değil), Usta yanıtının üstünde.
2. Çok satırlı yapıştır + gönder → tüm satırlar girintili görünür.
3. Konu girişinde cümle yaz → cümlen echo'lanır, sonra "konu: …" notice'ı gelir.
4. `usta reset --profile` → onay → `~/.config/usta/learner/profile.md` jenerik; `.bak` eski hali; sonraki oturumda Usta isimsiz/tanımadan selamlar.
5. `usta reset --profile` → `h` → hiçbir dosya değişmez.
6. `usta reset <konu>` ve `--factory` eski davranışında (regresyon).

## 6. Başarı Ölçütü

Elle doğrulama 1-6 geçer; kullanıcı kendi mesajlarını görür ve profilini tek komutla sıfırlayabilir.
