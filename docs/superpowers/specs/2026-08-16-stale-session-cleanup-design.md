# Tasarım — Yarım Kalmış Oturum: Otomatik Kurtarma Flush'ı (Salvage)

**Tarih:** 2026-08-16 (revize — ilk sürüm "onaylı silme" idi; Anil kararı: "dosyanın varlığını bilmek kullanıcıya bir şey vermiyor — direkt flush yapalım")
**Kapsam:** Açılışta bulunan yarım kalmış session kaydı (`.usta/sessions/*.jsonl`, `.done` işaretsiz) SORULMADAN kurtarılır: transcript kapanış çağrısına verilir → progress/approach/curriculum/mentor dosyaları geriye dönük yazılır → kayıt `.done` işaretlenir. Uyarı-gürültüsü biter, öğrenme kaybolmaz.
**Durum:** Onaylandı → writing-plans

## Amaç

İptal/çökme sonrası transcript diskte ama flush hiç koşmadı → öğrenme progress'e geçmedi. Bugünkü davranış: her açılışta sonsuz uyarı, kurtarma yolu yok. Karar: kayıt varsa **kurtarılabilir tek şey flush'tır** — o zaman direkt flush. Kullanıcıya soru yok (ADHD: karar yükü ekleme); tek satır bilgi yeter.

## Davranış

- Açılışta (`main.rs`'teki mevcut unflushed taraması, backend seçildikten sonra): her işaretsiz kayıt için:
  1. `ui::notice("recovering unflushed session: <dosya adı> — writing files…")`
  2. Transcript jsonl'ı parse edilir → `Vec<Message>` history + konu (dosya adından: `<topic>-<timestamp>.jsonl`).
  3. Mevcut kapanış akışının aynısı o history ile koşar (closing_prompt + split_files + flush_target yazımları + katalog + history.md kaydı — `flush_progress`'in yeniden kullanılabilir hali; oturumun system prompt'u o konu için yeniden kurulur `load_system_prompt` ile).
  4. Başarı → kayıt `.done.jsonl`'a rename edilir (mevcut düzgün-kapanış işaretiyle aynı konvansiyon) + `ui::notice("recovered: <konu>")`.
  5. Hata (parse bozuk / LLM erişilemez / yazım hatası) → `ui::warn` + kayıt OLDUĞU GİBİ bırakılır (sonraki açılışta tekrar denenir) + açılış AKIŞI ENGELLENMEZ.
- **TTY-değil (pipe/script) yolu:** salvage KOŞULMAZ — mevcut warn davranışı aynen (script'e sürpriz LLM çağrısı yok).
- Boş/anlamsız transcript (ör. 0 user turn): flush çağrılmaz, kayıt sessizce `.done` işaretlenir (kurtarılacak bir şey yok — gürültü de kalmasın).
- Sıra: salvage, YENİ oturumun konu seçiminden ÖNCE biter (kurtarılan progress, hemen ardından açılan oturumda görünür — "kaldığın yerden devam" gerçekten çalışır).

## Test

- Transcript parse: jsonl → history round-trip (transcript.rs'nin yazdığı formatı okuyan `read_history` yardımcısı; bozuk satır → hata).
- Konu çıkarımı: `kaynak-ingest-20260814-153309.jsonl` → `kaynak-ingest` (timestamp deseni sondan soyulur — konu adında tire olabilir).
- Boş-transcript yolu: user turn yoksa LLM çağrılmadan `.done` rename.
- Rename konvansiyonu: `x.jsonl` → `x.done.jsonl`, mevcut done-tespitiyle uyumlu (unflushed artık bulmuyor).
- Hata yolu: parse edilemeyen dosya → warn + dosya yerinde + akış devam (panic yok).
- LLM'li uçtan uca kısım elle doğrulamada (Anil, stagit'teki 2 gerçek kayıtla).

## Kapsam dışı

- Silme önerisi (önceki tasarım — iptal edildi; kullanıcı istemediği kaydı `usta reset <konu>` veya elle siler).
- Factory reset'in katalog-dışı projeleri bulması.
- Pipe modunda salvage.
- Transcript'ten oturumu "sohbet olarak" sürdürme (yalnız flush kurtarılır — konuşma değil).
