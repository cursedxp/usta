# Spec — Brain Bölünmesi: Müdahale-Haritalı Modüler Davranış Dosyaları

**Tarih:** 2026-08-08
**Durum:** Onaylandı (Anil: "bu şekilde olması daha doğru" + USER.md yeniden adlandırması)
**Ön koşul:** `2026-08-08-user-echo-profile-reset` planı main'e MERGE edilmiş olmalı (o paket `defaults.rs`, `progress.rs` kapanış akışı ve profil yollarına dokunuyor — bu spec aynı bölgeleri yeniden düzenler).

## 1. Sorun

Davranışın tamamı tek `USTA.md`'de (~12KB, 14 farklı bölüm). Dosya büyüdükçe müdahale maliyeti artıyor: "tonu düzelteceğim / yanlış davranışı düzelteceğim / öğretme biçimini düzelteceğim" için her seferinde tüm dosyayı okuyup ilgili cümleyi aramak gerekiyor. Karmaşıklığın kaynağı yönetilebilirlik değil, **anlaşılırlık**: hangi sorun için nereye bakılacağı belli değil.

## 2. Tasarım İlkesi

**Dosya sınırı = müdahale türü sınırı.** Bölme estetik ya da tema bazlı değil; "ne tür bir sorunu düzeltirken oraya bakarsın" sorusuna göre. Yanlış eksende bölme kompleksiteyi ARTIRIR (çapraz referans + "hangi dosyadaydı" belirsizliği) — bu spec'in ekseni müdahale haritasıdır.

## 3. Yeni Yapı

### Dosyalar (global kök, `~/.config/usta/`)

| Dosya | İçerik (mevcut USTA.md bölümlerinden birebir taşınır) | Sahiplik | Yükleme |
|---|---|---|---|
| `USTA.md` | **İndeks olur** — müdahale haritası tablosu + yükleme sırası + "içerik değişikliği ilgili dosyada yapılır" notu. Modele YÜKLENMEZ (insan haritası). | Code | ASLA |
| `SOUL.md` | Kimlik girişi ("Sen Usta'sın…"), Türkçe kuralı, Persona, Anlatım Dili | Code | Her zaman |
| `RULES.md` | Sert Kurallar (1-6), Canlı Belgeler (dosya sözleşmesi) | Code | Her zaman |
| `TEACHING.md` | Çalışma Kadansı, Açılış Drilli, Anlat-Modu (Feynman), İpucu Merdiveni, Tahmin Protokolü, Yeni Konu Tanışması, Kapsam Bekçiliği, Meta-beceri, Domaine göre yaklaşım | Code | Her zaman |
| `GOAL.md` | Hedefli Öğrenme (6 kural) | Code | **KOŞULLU**: yalnız konu approach'unda `## Hedef` varsa |
| `USER.md` | `learner/profile.md`'nin yeni adı ve yeri (kök) — kullanıcı kimliği | **User** | Her zaman |

- `approaches/`, `learner/progress|curriculum`, `learner/index.md` aynen kalır.
- Toplam davranış içeriği DEĞİŞMEZ — bu spec yalnız yeniden yerleşim; cümle ekleme/çıkarma ayrı iştir. (Tek istisna: USTA.md'nin indeksleşen yeni gövdesi + dosyalardaki `learner/profile.md` atıflarının `USER.md`'ye güncellenmesi.)

### Müdahale haritası (USTA.md'nin yeni gövdesinin çekirdeği)

| Belirti | Dosya |
|---|---|
| Ton/kişilik/anlatım anlaşılmıyor, bağlamıyor | `SOUL.md` |
| Yanlış davranış: kod yazdı, uydurdu, mekaniği anlattı, dosya ezdi | `RULES.md` |
| Öğretme biçimi: drill, ipucu zamanlaması, spek kadansı, tanışma | `TEACHING.md` |
| Sınav/hedef takibi, tempo, format pratiği | `GOAL.md` |
| Kullanıcı hakkında yanlış/eksik bilgi | `USER.md` (ya da `usta reset --profile`) |

### Yükleme sırası (`brain.rs`)

```
SOUL.md → RULES.md → TEACHING.md → [GOAL.md, yalnız approach'ta "## Hedef" varsa]
→ approaches/(software|_default).md → approaches/<konu>.md
→ USER.md → progress → curriculum → [BUGÜN bölümü mevcut haliyle]
```

`USTA.md` yüklenmez. Koşullu GOAL: approach içeriği zaten okunuyor — `## Hedef` araması ek IO istemez.

## 4. USER.md Geçişi

- `learner/profile.md` → kök `USER.md` (içerik aynı; gömülü jenerik şablon da taşınır).
- **Migrasyon** (`ensure_scaffold`): global kökte `learner/profile.md` VAR ve `USER.md` YOK ise dosya taşınır (rename) — mevcut kullanıcı verisi kaybolmaz. Sonrasında `learner/profile.md` referansı kodda kalmaz.
- Güncellenecek referanslar: `brain.rs` yükleme, `defaults.rs` listesi (`USER.md`, User-owned), TUI welcome isim okuma, `usta reset --profile` hedefi (+ `.bak` yolu), kapanış flush'ındaki `profile` → `USER.md` hedefi, `profile_is_generic` karşılaştırması, MEET_BLOCK/closing prompt'taki "profiline" ifadeleri (metin aynı kalabilir — kullanıcıya dosya adı söylenmiyor), README/SPEC.
- Komut adı DEĞİŞMEZ: `usta reset --profile` kalır (kullanıcı-yüzü kavram "profil"; dosya adı iç detay).

## 5. Sync Davranışı

- Yeni dosyalar (`SOUL/RULES/TEACHING/GOAL.md` + indeks-USTA.md) Code-owned → mevcut senkron mekanizması ilk çalıştırmada global köke yazar; eski büyük USTA.md içeriği otomatik indeksle değişir (Code-owned güncelleme — bilinen davranış).
- `USER.md` User-owned: ilk-kez yazılır, asla ezilmez; migrasyon taşıması sync'ten ÖNCE koşar (yoksa boş şablon yazılıp taşıma "var" sanılabilir — sıra: migrate → sync).

## 6. Kapsam Dışı

- Davranış içeriğinde herhangi bir değişiklik (bölme sırasında cümle eklenmez/çıkarılmaz/yumuşatılmaz).
- approaches/ yapısının değişmesi; proje-yerel `.usta/` düzeni.
- Kullanıcıya SOUL özelleştirme arayüzü (ileride; bu spec sadece zemini kurar).

## 7. Test Stratejisi

- **İçerik sadakati:** bölme sonrası `SOUL+RULES+TEACHING+GOAL` birleşimi, eski USTA.md'nin başlık envanteriyle karşılaştırılır (plan adımı: her `##` başlığın tam bir kez, tek dosyada yaşadığını doğrulayan script/diff). Kayıp/çift bölüm = FAIL.
- `brain.rs`: sistem promptu SOUL/RULES/TEACHING içerir; indeks-USTA.md içeriği İÇERMEZ; GOAL yalnız `## Hedef`li approach'ta girer (iki yönlü test).
- `defaults.rs`: yeni dosya listesi + sahiplikler (Code×5 → indeks dahil; USER.md User).
- Migrasyon: temp kökte `learner/profile.md` → `ensure_scaffold` → `USER.md` taşınmış, içerik aynı, eski yol yok; USER.md zaten varsa dokunulmaz.
- `reset --profile`, `profile_is_generic`, kapanış `profile` hedefi: yeni yola karşı mevcut testler güncellenir.
- Mevcut tüm testler yeşil.

## 8. Elle Doğrulama

1. Rebuild + ilk açılış: global kökte yeni dosyalar oluşmuş; eski profil içeriği `USER.md`'de (migrasyon).
2. Normal oturum: davranış regresyonu yok — kod yazmıyor (RULES), drill soruyor (TEACHING), isimle hitap (USER).
3. Hedefsiz konu: system prompt'ta GOAL yok (bağlam göstergesi ~1.5KB daha düşük); hedefli konu (approach'ta `## Hedef`): tempo satırı geliyor.
4. `usta reset --profile` → `USER.md` jenerik + `.bak`.
5. USTA.md'yi aç: harita tablosu — "tonu değiştireceğim" → SOUL.md'yi bul-düzelt-rebuild akışı çalışıyor.

## 9. Başarı Ölçütü

Elle doğrulama 1-5 geçer; "neyi düzeltmek için nereye bakarım" sorusunun cevabı tek tablo; davranış birebir korunmuş.
