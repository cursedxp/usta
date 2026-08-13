# Tasarım + Plan — Görsel Motor Çakışma Çözümü (Engine-Level Anti-Overlap)

**Tarih:** 2026-08-13 · **Yürütücü:** ayrı session (Opus) · **Yöntem:** superpowers subagent-driven-development (task başına taze implementer + task review + final whole-branch review)
**Branch:** `usta-overlap-engine` (main'den) · **Merge/push YOK** — Anil onayı beklenir.

## Problem (kanıtlı — 2026-08-13 gerçek /show çıktıları)

Prompt'taki NO OVERLAP kuralları (d5eaee3) istatistiksel iyileştirme; model yine çakışık koordinat üretebiliyor. Ekran görüntülerinde üç yapısal motor eksiği:

1. **Çarpışma çözümü yok:** model `note`'u node/daire üstüne koyunca motor aynen çiziyor ("tek giris noktasi" note'u kök dairesini örttü; "senin klasorun: home" note'u etc/home node'larının altına bindi).
2. **Ok uçları sabit pad:** `arrow` op'u merkezden sabit 46px kısaltıyor. Geniş elemanlarda (ör. 240px'lik note) uç eleman SINIRININ İÇİNDE kalıyor → ok kutunun altından/içinden çıkmış görünüyor.
3. **Bare `text` ölçüsüz:** metin AABB'si hesaplanmıyor; "bağlanır" yazısı ok gövdesiyle üst üste.

Çözüm ilkesi: **model önerir, motor garanti eder.** Deterministik, hafif bir yerleşim emniyeti — tam layout motoru DEĞİL (YAGNI).

## Dosya

Tek dosya: `src/visual_skeleton.html` (motor JS). Rust tarafında yalnız test fixture'ı eklenir (`src/visual.rs` testleri). Değişmezler HER GÖREVDE geçerli:
- Placeholder'lar ×3 (`/*__ROUGH__*/`, `/*__ANIME__*/`, `/*__SCENES__*/[]`) birer kez
- ID'ler: `prev/play/next/caption/counter/stage/root/detail-btn/detail-panel` + `el-` öneki
- Class'lar: `node/circ/hl/lbl/arrow/alabel/notebox/packet`
- Sıfır ağ isteği; anime.min.js + rough.min.js dokunulmaz; deterministik replay (rough seed, transient op skip) bozulmaz
- Sahne JSON şeması DEĞİŞMEZ (geriye uyum — eski görseller açılabilir kalır)

---

## Görev 1 — Geometri katmanı: AABB + metin ölçümü

`src/visual_skeleton.html` motor JS'ine saf yardımcılar:

```js
// Her elemanın eksen-hizalı sınır kutusu (mevcut meta/orig kayıtlarından + spec'ten).
// node: {x,y,w,h} → doğrudan. circle: cx±r. note: mevcut w hesabı (chars*7.2+24) ± h 36.
// text: TAHMİN — width = text.length * fontSize * 0.62, height = fontSize * 1.4, (x,y) merkez.
// GERÇEK ölçüm: element DOM'a eklendikten sonra getBBox() ile düzeltilir (rAF gerekmez,
// SVG'de senkron çalışır) — tahmin yalnız ekleme ÖNCESİ çarpışma testinde kullanılır.
function aabbOf(id) -> {x1,y1,x2,y2} | null       // görünür eleman değilse null
function aabbFromSpec(op) -> {x1,y1,x2,y2}         // henüz eklenmemiş elemanın öngörü kutusu
function inflate(box, pad) -> box                   // 24px clearance için
function intersects(a, b) -> bool
```

- `meta`/`orig` kayıtları AABB üretecek kadar zenginleştirilir (w/h saklanır — mevcut cx/cy'ye ek; `move` sonrası AABB güncel kalmalı).
- Ok/paket/altyazı AABB kapsamına GİRMEZ (yalnız kutu-tipi elemanlar: node, circle, text, note).
- Test: motor JS'i cargo ile test edilemez → Görev 4'teki torture fixture + headless doğrulama bu katmanın kanıtı. Rust tarafında değişiklik yok.

## Görev 2 — Ok uçları: kenar kesişimi (sabit pad yerine)

`arrow` op'unda uçlar, merkez-merkez doğrusunun kaynak/hedef **AABB kenarıyla kesişiminden + 12px boşluk**:

```js
function edgePoint(fromBox, toCenter) -> {x,y}  // box merkezinden toCenter'a giden ışının box kenarını kestiği nokta
```

- x1,y1 = edgePoint(kaynakAABB, hedefMerkez) + 12px dışarı; x2,y2 = simetrik.
- Görünmez düz guide path AYNI uçları kullanır → paket animasyonu tutarlı (anime.path).
- Ok etiketi (`alabel`) orta noktada kalır; etiket AABB'si ok gövdesinden 8px yukarı ofsetli (mevcut -8 korunur).
- Dejenere durum (kutular iç içe / merkezler çakışık): mevcut 46px pad davranışına düş — kırılma yok.
- Rough gövde + el çizimi ok ucu iki çizgi mevcut yapıda; yalnız uç KOORDİNATLARI değişir.

## Görev 3 — Ekleme anında çarpışma çözümü (auto-nudge)

`add` (node/circle/text) ve `note` op'larında, ekleme ÖNCESİ:

```js
function resolveCollision(box) -> {dx,dy}
// Mevcut görünür elemanların 24px şişirilmiş AABB'leriyle kesişiyorsa:
// deterministik arama — 8px adımlarla sırasıyla: aşağı, yukarı, sağ, sol, sonra
// spiral (aşağı-sağ, aşağı-sol, ...) — en fazla 40 adım. İlk boş konum kazanır.
// Sahne sınırı: 40px kenar payı içinde kalınır (800×450). Boş konum yoksa
// en az kesişimli aday seçilir (asla vazgeçme — eleman HER ZAMAN çizilir).
```

- Nudge sonucu spec koordinatlarına delta olarak uygulanır; `meta`/`orig` NİHAİ konumla kaydedilir → sonraki oklar/move'lar doğru merkezden hesaplar.
- DOM'a ekledikten sonra `getBBox()` ile gerçek kutu alınır; tahminden büyükse (uzun metin) İKİNCİ bir düzeltme turu yapılır (tek seferlik, aynı arama).
- `move` op'u nudge YAPMAZ (model kastı korunur — anlatı hareketi bozulmaz; spec'e not).
- Deterministik replay: arama tamamen deterministik (sabit sıra, sabit adım) → aynı sahne dizisi her replay'de aynı yerleşimi üretir.
- Not defterine: nudge tetiklendiğinde `console.debug("[nudge]", id, dx, dy)` — sahada teşhis için (kullanıcı görmez).

## Görev 4 — Torture fixture + doğrulama + teslim

1. `src/visual.rs` testlerine `TORTURE` fixture: görüntü-7'yi taklit eden kasıtlı çakışık sahne dizisi (note tam node üstünde; daire note arkasında; geniş note'tan çıkan ok; node'a bitişik bare text). Test: `build_visual_html(TORTURE)` başarılı + çıktı dosyaya yazılır (demo deseni — `usta-visual-torture.html`, `--nocapture` ile yol basılır).
2. Headless Chrome (iki tema): torture sayfasında — hiçbir kutu-tipi eleman çifti kesişmiyor (screenshot + görsel kontrol; mümkünse sayfada `intersects` üzerinden konsol assert scripti), oklar kutu kenarından başlıyor, paket ok boyunca akıyor, replay (◀ sonra ▶|) aynı yerleşimi veriyor. Ekran görüntüleri scratchpad'e.
3. `cargo test -p usta && cargo build -p usta` yeşil, uyarısız. Mevcut 206 test kırılmaz.
4. `visual_system()` prompt'una tek satır ek: "The engine nudges overlapping elements apart as a safety net, but good composition is still YOUR job — nudged layouts look worse than planned ones." (test needle: `safety net`).
5. Spec güncelle (`2026-08-12-visual-explainer-design.md` Tasarım Dili bölümüne motor-emniyet paragrafı).
6. Commit'ler görev başına (Türkçe konu + `Co-Authored-By: Claude Opus 4.8 (1M context) <noreply@anthropic.com>`). Merge/push YOK — bitişte rapor: commit listesi, test özeti, torture ekran görüntüleri.

## Kapsam dışı (YAGNI)

Tam layout motoru (force-directed vb.) · `move` nudge'ı · Rust tarafında sahne doğrulama/retry döngüsü · şema değişikliği · overlap'ta model'e geri bildirim.

## Riskler

- **getBBox zamanlaması:** SVG'de senkron ama element `display:none` ata altındaysa 0 döner — elemanlar doğrudan `#root`'a ekleniyor, sorun beklenmez; yine de 0-kutu gelirse tahmine düş.
- **Nudge + kümülatif sahneler:** replay'de op sırası aynı → nudge deterministik; ama BİR sahnede nudge edilen eleman sonraki sahnede `move` hedefi olursa move MUTLAK koordinata gider (mevcut davranış) — kabul, spec'e not.
- **Ok ucu değişimi eski görselleri etkilemez** (dosyalar kendi kendine yeten HTML — motor gömülü, retroaktif değişiklik yok).
