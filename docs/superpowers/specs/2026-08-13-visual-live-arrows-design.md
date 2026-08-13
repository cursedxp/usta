# Tasarım + Plan — Canlı Oklar (Live Arrow Connections)

**Tarih:** 2026-08-13 · **Yöntem:** SDD (task başına taze implementer + reviewer + final review) · **Branch:** `usta-live-arrows` (main'den) · Merge/push YOK — kontrolör raporundan sonra ana session bağımsız review + merge yapar.

## Problem (kanıtlı — 2026-08-13 gerçek /show çıktısı, özet sahnesi)

Oklar çizim ANINDAKİ merkez koordinatlarına kakılı statik çizgiler:
1. **Move sonrası kopukluk:** endpoint elemanı `move` edilince ok eski yerde kalıyor — boşluğu gösteren ok yığını (ekran görüntüsü: 8 ok çapraz uçuşuyor, hedefleri taşınmış).
2. **Remove kaskadı yok:** eleman silinince ona bağlı oklar sahnede kalıyor.
3. Prompt'taki "remove what is no longer needed" statistiksel — özet sahnelerinde model unutuyor.

Çözüm ilkesi: **ok = canlı bağlantı.** from/to id'leri kayıtlı; geometri her zaman GÜNCEL merkezlerden türetilir.

## Dosya

`src/visual_skeleton.html` (motor JS) + `src/visual.rs` (torture fixture genişletme + prompt satırı + test needle). Değişmezler her görevde: placeholder ×3, id/class seti, sıfır ağ, vendor'lar dokunulmaz, şema DEĞİŞMEZ (geriye uyum), deterministik replay korunur, mevcut 207 test yeşil kalır.

---

## Görev 1 — Ok geometrisi tek fonksiyona: `drawArrowGeometry`

- Ok kaydı zenginleşir: `meta[arrowId] = { from, to, label, ... }` (from/to element ID'leri saklanır — şu an yalnız uç koordinatları var).
- Mevcut ok çizimi (kenar-kesişim uçlar + crossing-guard fallback + rough gövde + 2 çizgi ok ucu + görünmez guide path + alabel) **saf bir yeniden-çizim fonksiyonuna** çekilir: `drawArrowGeometry(arrowId)` — GÜNCEL `meta` merkezlerinden okur, `el-<arrowId>` grubunun İÇERİĞİNİ yeniden kurar (grup ve id sabit kalır — highlight class'ı, referanslar bozulmaz). Rough seed = arrowId'den (mevcut kural) → aynı bağlantı hep aynı çizim.
- İlk `arrow` op'u bu fonksiyonu kullanır (davranış birebir aynı kalmalı — refactor, görünür fark yok).
- `packet` guide path'i grup içinden bulmaya devam eder (`m.path` referansı yeniden-çizimde güncellenir).

## Görev 2 — Move retarget

- `move` op'u tamamlandığında (animate=true'da tween bitince; animate=false'ta anında): taşınan ID'yi endpoint olarak kullanan TÜM oklar `drawArrowGeometry` ile yeniden çizilir (anında, tween'siz — spec kararı: ok "snap" eder, ayrıca animasyonu YAGNI).
- `meta` merkez güncellemesi zaten move'da yapılıyor (mevcut) — retarget ondan SONRA çalışır.
- Auto-nudge'ın getBBox ikinci-düzeltmesi de merkez kaydırabiliyor (mevcut) — o yol da eleman eklendikten sonra çalışıyor ve o anda bağlı ok olamaz (ok sonradan çizilir) → retarget gerekmez; spec notu yeter.
- Replay: op sırası deterministik → retarget deterministik.

## Görev 3 — Remove kaskadı

- `remove` op'u eleman X'i silerken from==X veya to==X olan okları da siler. animate=true'da ok, elemanla AYNI fade süresinde söner; animate=false'ta anında.
- Silinen okların `meta` kayıtları temizlenir (sızıntı yok; sonraki aynı-id'li arrow op'u temiz başlar).
- Bir OKUN kendisi `remove` edilirse mevcut davranış (yalnız ok gider) — kaskad tek yönlü (elemandan oka).
- Kenar durumu: `packet` yalnız kendi op'u sırasında yaşar (transient) — kaskadla çakışamaz; spec notu.

## Görev 4 — Prompt + torture + doğrulama + teslim

1. `visual_system()` Composition bölümüne tek satır: *"Arrows stay attached: they follow moved elements and disappear with removed ones. For a summary scene, prefer removing old arrows and drawing a fresh clean layout."* (test needle: `stay attached`).
2. `TORTURE` fixture'a iki sahne eklenir: (a) bağlı oku olan node `move` edilir → ok yeni konumu göstermeli; (b) bağlı okları olan node `remove` edilir → oklar da gitmeli. Test: build başarılı + torture HTML yazılır.
3. Headless Chrome (iki temadan biri yeter): move sonrası okun her iki ucu da güncel kutu kenarlarında (koordinat kontrolü konsol assert'iyle: guide path uçları hedef AABB kenarına ≤16px); remove sonrası sahnede kaskadlanan ok YOK; replay iki kez aynı sonuç. Ekran görüntüsü scratchpad'e.
4. `cargo test -p usta && cargo build -p usta` yeşil, uyarısız; mevcut 207 test kırılmaz.
5. Spec güncelle (`2026-08-12-visual-explainer-design.md` motor-emniyet paragrafına canlı-ok cümlesi).
6. Görev başına commit (Türkçe konu + `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`). Merge/push YOK — rapor: commit listesi, test özeti, headless bulgular, ekran görüntüsü yolları.

## Kapsam dışı (YAGNI)

Ok taşınmasının tween'li animasyonu · okların kendi kendine yeniden-yönlendirilmesi (obstacle avoidance/orthogonal routing) · şema değişikliği · eski üretilmiş görsellerin retroaktif düzeltilmesi (motor gömülü — imkânsız zaten).

## Riskler

- **Grup içeriği yeniden kurulurken highlight durumu:** `hl` class grup üstünde — içerik değişse de class korunur; reviewer kontrol etsin.
- **Aynı elemana çok ok:** retarget hepsini döngüyle yeniden çizer — O(ok sayısı), sahnede ≤~10 ok, sorun değil.
- **drawArrowGeometry refactor'u mevcut görünümü değiştirmemeli** — Görev 1 review'ında torture-öncesi görsel fark olmadığı headless karşılaştırmayla doğrulanmalı (aynı SAMPLE, önce/sonra screenshot diff gerekmez; grup yapısı + uç koordinatlar aynı olmalı).
