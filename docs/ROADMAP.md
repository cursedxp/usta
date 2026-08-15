# Usta Roadmap

> Karar tarihi: 2026-08-12. Sıra bilinçli — her madde kendi spec → plan → implementasyon döngüsüyle gider.
> Sürümleme: her tamamlanan roadmap maddesi minor bump (SPEC §'ü ile hizalı); tag vX.Y.Z.

| # | Özellik | Durum | Özet |
|---|---|---|---|
| 1 | **Görsel anlatım** (`/show`) | ✅ tamamlandı (2026-08-13) | Gömülü HTML iskelet + model sahne senaryosu üretir → tarayıcıda animasyonlu anlatım. Proaktif öneri: anlaşılmadı sinyalinde Usta `/show` önerir. |
| 2 | **Egzersiz/artefakt döngüsü** | ✅ tamamlandı (2026-08-15) | Watcher'ı kod dışına genelleme: Usta teslimat atar, kullanıcı `exercises/` altına yazar, aynı feedback döngüsü her domain'de çalışır. |
| 3 | **Spaced repetition** | ✅ tamamlandı (2026-08-15) | Geri-çağırma sorularına due-date (basitleştirilmiş SM-2); açılışta "bugün X madde vadesi geldi". |
| 4 | **Dağıtım/onboarding** | kısmen ✅ (2026-08-15: onboarding-lite + 0.13.0; dağıtım/brew ertelendi) | Prebuilt binary (brew/release), API key'siz yol, ilk-çalıştırma sihirbazı. "Herkes kullansın" ön koşulu. |
| 5 | **Materyal yutma** | ✅ tamamlandı (2026-08-15) | Kullanıcı PDF/kitap/kurs verir, müfredat onun etrafına kurulur. |
| 6 | **İlerleme özeti/motivasyon** | ✅ tamamlandı (2026-08-15) | Haftalık özet: harita % değişimi, oturan maddeler, streak. ADHD: görünür ilerleme = yakıt. |
| 7 | **Deneme sınavı üretici** | beklemede | GOAL modunda `/exam`: haritadan zamanlı deneme, skor `## Hedef Durumu`na işlenir. |
| 8 | **Gamification modu** | beklemede | `/game on\|off` toggle (kalıcılık USER.md profilinde). Müfredat durumları = XP, süreç-puanı (oturum/tahmin — doğruluk değil), gap kapatma = rozet. ADHD-safe: kırılan seride suçlama yok ("en uzun serin X" çerçevesi), puan performansa değil sürece. Overjustification'a dikkat. Roadmap #7 ile birleşir: sınav = boss fight. Çoğu markdown/prompt işi. |

## Tamamlananlar

- 2026-08-15: İlerleme özeti/motivasyon — global append-only `learner/history.md` (kapanış flush'ı başına bir satır: konu | map% | settled); `usta stats` haftalık özet (konu başına oturum + map/settled delta, güncel + en uzun streak); ADHD-safe ton — "current streak: 0" hiçbir yüzeyde yazılmaz, kırık seride yalnız en uzun streak pozitif çerçeveyle basılır; welcome kutusu "This week: N session(s) · streak M day(s)"; LLM'siz (kabuk sayar); v0.15.0.

- 2026-08-15: Materyal yutma — kullanıcı md/txt (pdftotext varsa PDF→txt) materyalini görünür `materials/` klasörüne koyar; yeni-konu tanışmasında kabuk deterministik digest (başlık iskeleti + alıntı, UTF-8 güvenli cap) üretip enjekte eder; Usta müfredatı materyalin bölümlerine `— kaynak: <dosya> §<bölüm>` referanslarıyla demirler (web kapsam bekçiliği korunur, eksik kritik konu `— kaynak: web`); scaffold `materials/` kurar; v0.14.0.

- 2026-08-15: Spaced repetition — geri çağırma soruları `| due: YYYY-MM-DD | ivl: <gün>` kuyruğu taşır; basitleştirilmiş SM-2 merdiveni (`1→3→7→16→35→90` gün, ease factor yok); açılış drilli yalnız vadesi gelenleri sorar (en fazla 3, en eski önce), vadeli yoksa atlanır; welcome kutusu "Reviews due today: N" / "No reviews due today" gösterir; `ivl: 90`'ı rahat geçen soru `Kapatılanlar`a emekli olur.

- 2026-08-15: Egzersiz/artefakt döngüsü — Usta teslimat atar, kullanıcı `exercises/` altına yazar → aynı Socratic feedback döngüsü (atamaya karşı, çözüm verilmez), `cargo check` atlanır; açık egzersiz progress'te yaşar + açılışta hatırlatılır; scaffold `exercises/` kurar; pedagoji TEACHING.md'de.

- 2026-08-13: Görsel anlatım tam paket — /show + anime.js oynatıcı + Excalidraw tasarım dili (rough.js/Excalifont) + cam notch + detay paneli + [[show:]] doğal dil tetiklemesi + retention.

- 2026-08-12: UX paketi — çok-satırlı girdi (Ctrl+J), tek Esc iptal, `/watch` toggle, İngilizce ana dil + dil aynası, `/help`.
