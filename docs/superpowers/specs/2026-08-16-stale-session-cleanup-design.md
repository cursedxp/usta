# Tasarım — Yarım Kalmış Oturum Kaydı: Otomatik Temizlik (v3)

**Tarih:** 2026-08-16 (v3 — v1 "onaylı silme" ve v2 "salvage flush" İPTAL; Anil netleştirdi: "iptal/reset edilmiş konuşma bitmiştir — silmemiz gerekir")
**Kapsam:** Açılışta bulunan işaretsiz session kaydı (`.usta/sessions/*.jsonl`, `.done` değil) SORULMADAN SİLİNİR + tek dim bilgi satırı. Kurtarma YOK, soru YOK.
**Durum:** Onaylandı → implement

## Gerekçe

- Yarım kayıt = iptal (kullanıcı bilinçli vazgeçti — "kaydetmesine izin vermemiştim") veya reset sonrası artık (o dönem kapandı) veya çökme. Üçünde de devam mekanizması yok; varlığını bilmek kullanıcıya hiçbir şey vermiyor, uyarı kalıcı gürültü.
- Salvage (v2) iptal niyetine AYKIRI: kullanıcı kaydedilmesin istedi, kurtarmak istediğini geri getirirdi.
- En sade doğru davranış: temizle, tek satır söyle, geç.

## Davranış

- Mevcut unflushed taraması (main.rs ~95): warn döngüsü SİLİNİR; yerine her kayıt `std::fs::remove_file`; sonunda tek satır `ui::notice("cleaned N stale session record(s)")` (N>0 ise; 0 ise hiçbir çıktı yok).
- Silme hatası → tek warn, akış devam (açılış asla bloklanmaz).
- TTY ve pipe AYNI davranış (kayıt tanımı gereği çöp — modda ayrım yok).
- `.done.jsonl` dosyalarına yapısal olarak dokunulmaz (unflushed zaten bulmuyor).
- v2 için yazılmış transcript-okuma yardımcıları eklenmişse kaldırılır (YAGNI); v1 `delete_unflushed` yardımcısı bu iş için KULLANILIR (zaten commit'li: f0b2ff4) — v1'in confirm akışı (8ed8179) sökülür.

## Test

- tmpdir: 2 işaretsiz + 1 `.done` → temizlik sonrası işaretsizler yok, `.done` duruyor; notice N=2.
- Kayıt yoksa çıktı yok.
- Silme hatası panic'sizce warn'lanır.

## Kapsam dışı

- Salvage/kurtarma (bilinçli iptal edildi) · onay sorusu · factory reset'in katalog-dışı proje bulması.
