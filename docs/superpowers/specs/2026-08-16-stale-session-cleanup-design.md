# Tasarım — Yarım Kalmış Oturum Kaydı: Onaylı Kurtarma (v4 — FINAL)

**Tarih:** 2026-08-16 (v4 — Anil son karar: "salvage kalabilir" → karma: sor, default kurtar)
**Kapsam:** Açılışta işaretsiz session kaydı (`.usta/sessions/*.jsonl`) bulununca TTY'de TEK soru: `recover N unflushed session(s)? [Y/n]` — Enter/evet = salvage (transcript kapanış flush'ına verilir, progress geriye dönük yazılır, kayıt `.done`), `n` = hepsi silinir + tek bilgi satırı. Pipe'ta mevcut warn birebir (LLM sürprizi de sessiz silme de yok).
**Durum:** Onaylandı → implement

## Gerekçe

- Çökme senaryosu: öğrenme kurtarılmalı → default salvage (Enter yeter).
- İptal/reset senaryosu: konuşma bitmiştir → `n` tek tuşla temizler.
- Niyeti kabuk bilemez — tek soruyla kullanıcıya sorulur; default, kayıpsız taraf.

## Davranış

- Tarama noktası (main.rs ~95, backend seçiminden SONRA duracak şekilde): kayıt listesi warn YERİNE tek soruya gider (dosya adları önce dim listelenir).
- TTY (`stdin+stdout is_terminal`):
  - Soru: `recover {N} unflushed session(s)? [Y/n] ` — kabul kümesi ters-default: boş girdi/`e`/`evet`/`y`/`yes` = KURTAR; `n`/`no`/`h`/`hayır` = SİL; başka girdi → soru tekrarı değil, güvenli taraf: kurtar. (Mevcut `confirm` default-hayır çalışıyorsa bu akış için default-evet varyantı gerekir — küçük yardımcı.)
  - KURTAR: her kayıt için: `topic_from_record` + `read_history` (transcript jsonl parse) → o konu için `load_system_prompt` ile system kur → `flush_core` (flush_progress'ten session-bağımsız refactor) → başarıda `mark_done` (`x.jsonl` → `x.done.jsonl`) + `notice("recovered: <topic>")`. Parse/LLM/yazım hatası → warn + kayıt yerinde (sonraki açılışta tekrar) + akış devam. Boş transcript (hiç user turn) → LLM'siz sessizce `.done`. Kayıtlar arası `backend.reset_session()`.
  - SİL: `transcript::delete_unflushed` (f0b2ff4) → `notice("cleaned {N} stale session record(s)")`; hata → warn.
- Pipe/TTY-değil: bugünkü warn davranışı BİREBİR.
- Açılış hiçbir hata durumunda bloklanmaz.

## Test

- v2 yardımcıları: `read_history` round-trip (Recorder formatı), `topic_from_record` (tireli konu + timestamp soyma + uymayan → None), `mark_done` (rename sonrası unflushed bulmaz), `delete_unflushed` (yalnız verilenler, hata toplama — mevcut).
- Default-evet confirm yardımcısı: boş → true, n/h → false, y/e → true.
- Boş-transcript yolu: LLM çağrısız `.done`.
- Pipe yolu: davranış değişmedi (kod incelemesi).
- LLM'li uçtan uca: elle doğrulama (Anil, stagit'teki 2 gerçek kayıt).

## Kapsam dışı

- Kayıt-başına ayrı soru (batch tek soru yeter) · factory reset'in katalog-dışı proje bulması · transcript'ten sohbeti sürdürme (yalnız flush kurtarılır).
