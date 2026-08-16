# Tasarım — Yarım Kalmış Oturum Kaydı Temizliği

**Tarih:** 2026-08-16
**Kapsam:** Açılışta bulunan yarım kalmış session kayıtları (`.usta/sessions/*.jsonl`, `.done` işaretsiz) için tek-seferlik silme önerisi. Bugün uyarı her açılışta sonsuza dek tekrarlanıyor, temizlik elle.
**Durum:** Onaylandı (Anil, 2026-08-16: stagit'te yaşandı — "şimdi onu fixleyelim") → writing-plans

## Amaç

İptal edilen oturumların transcript'i işaretsiz kalır (tasarım gereği — kurtarma değeri var). Ama uyarı (`main.rs:95`) kalıcı: kullanıcı elle `rm` yapana kadar her açılışta tekrar. ADHD-düşmanı kalıcı gürültü. Factory reset de katalog-dışı projeleri göremediği için temizleyemiyor (bilinen, kapsam dışı).

Karar: uyarının yanına **tek onay sorusu** — TTY'de "delete N half-finished session record(s)? [y/N]" (default: Hayır — güvenli taraf, kayıt kurtarılabilir değer taşıyabilir). Evet → listelenen dosyalar silinir + tek satır bilgi. Hayır/pipe → bugünkü davranış aynen (uyarı basılır, dokunulmaz).

## Davranış

- `main.rs:95` uyarı döngüsü: dosyalar tek tek warn edilir (mevcut), ardından TTY ise (`stdin+stdout is_terminal` — sihirbazdaki koşulun aynısı) mevcut `confirm(...)` yardımıyla tek soru: `delete {N} half-finished session record(s)? [y/N] ` — kabul kümesi `["e","evet","y","yes"]` (mevcut confirm konvansiyonu).
- Evet → her dosya `std::fs::remove_file`; hata tek tek warn edilir, akış devam eder; sonda `ui::notice("deleted {N} record(s)")`.
- Hayır veya TTY-değil → hiçbir dosyaya dokunulmaz.
- Silme mantığı saf/test-edilebilir: `transcript::delete_unflushed(project_root, files) -> (deleted: usize, errors: Vec<String>)` benzeri yardımcı (imza implementasyona esnek), fs kısmı tmpdir testli.
- SPEC'e tek cümle; sürüm 0.18.3 (patch) + tag.

## Test

- tmpdir: işaretsiz 2 + `.done` 1 dosya → unflushed 2 bulur (mevcut test var), delete yardımcısı yalnız verilenleri siler, `.done` dosyası yerinde kalır.
- Silme hatası (olmayan dosya) → error listelenir, panic yok.
- TTY-değil yolu: davranış değişmedi (kod incelemesi).

## Kapsam dışı

- Factory reset'in katalog-dışı projeleri bulması.
- Otomatik silme / yaş eşiği.
- Transcript'ten kurtarma akışı ("resume from record").
