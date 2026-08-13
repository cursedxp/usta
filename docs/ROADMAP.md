# Usta Roadmap

> Karar tarihi: 2026-08-12. Sıra bilinçli — her madde kendi spec → plan → implementasyon döngüsüyle gider.

| # | Özellik | Durum | Özet |
|---|---|---|---|
| 1 | **Görsel anlatım** (`/show`) | ✅ tamamlandı (2026-08-13) | Gömülü HTML iskelet + model sahne senaryosu üretir → tarayıcıda animasyonlu anlatım. Proaktif öneri: anlaşılmadı sinyalinde Usta `/show` önerir. |
| 2 | **Egzersiz/artefakt döngüsü** | beklemede | Watcher'ı kod dışına genelleme: Usta teslimat atar, kullanıcı `exercises/` altına yazar, aynı feedback döngüsü her domain'de çalışır. |
| 3 | **Spaced repetition** | beklemede | Geri-çağırma sorularına due-date (basitleştirilmiş SM-2); açılışta "bugün X madde vadesi geldi". |
| 4 | **Dağıtım/onboarding** | beklemede | Prebuilt binary (brew/release), API key'siz yol, ilk-çalıştırma sihirbazı. "Herkes kullansın" ön koşulu. |
| 5 | **Materyal yutma** | beklemede | Kullanıcı PDF/kitap/kurs verir, müfredat onun etrafına kurulur. |
| 6 | **İlerleme özeti/motivasyon** | beklemede | Haftalık özet: harita % değişimi, oturan maddeler, streak. ADHD: görünür ilerleme = yakıt. |
| 7 | **Deneme sınavı üretici** | beklemede | GOAL modunda `/exam`: haritadan zamanlı deneme, skor `## Hedef Durumu`na işlenir. |

## Tamamlananlar

- 2026-08-13: Görsel anlatım tam paket — /show + anime.js oynatıcı + Excalidraw tasarım dili (rough.js/Excalifont) + cam notch + detay paneli + [[show:]] doğal dil tetiklemesi + retention.

- 2026-08-12: UX paketi — çok-satırlı girdi (Ctrl+J), tek Esc iptal, `/watch` toggle, İngilizce ana dil + dil aynası, `/help`.
