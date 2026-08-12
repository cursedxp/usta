# Handoff v2 — Görsel Anlatım Tasarım Dili: Excalidraw Whiteboard Yönü

**Tarih:** 2026-08-12 (v2 — v1'i tamamen değiştirir) · **Yürütücü:** implementasyon session'ı (Opus) · **Araçlar:** Claude Design (mcp__claude-design__*) + repo
**Rol dağılımı:** Planlama bu dokümanda bitti. Tasarım sistemi Claude Design'da kurulur, implementasyon repoda yapılır — ikisi de yürütücü session'ın işi. Anil ara onayları verir.
**Bağlam:** `/show` çalışıyor (ekran görüntüsüyle doğrulandı) ama görünüm jenerik kutu-ok; tasarım dili oturmamış. Anil kararı: kanıtlanmış bir tasarım stili referans alınacak.

---

## Tasarım Yönü Kararı (planlama çıktısı — bağlayıcı)

**Referans stil: Excalidraw whiteboard estetiği** (el çizimi/sketchy görünüm).

Gerekçe:
1. **Persona uyumu:** Usta = yanında oturan mentor. Mentor beyaz tahtaya ÇİZER — parlak SaaS diyagramı yapmaz. Sketchy stil bu metaforun görsel dili.
2. **Eğitim-yerlisi:** Excalidraw stili tech eğitim içeriğinin fiili standardı (YouTube açıklamaları, sistem tasarım mülakatları, blog diyagramları). Öğrenen göz bu dile "ders" olarak koşullanmış.
3. **Affedici:** El çizimi estetik, otomatik üretilen yerleşimdeki küçük hizalama kusurlarını doğal gösterir — LLM-üretimi sahnelerde büyük avantaj (pixel-perfect stil kusuru büyütür, sketch gizler).
4. **Teknik olarak kanıtlanmış + lisans temiz:**
   - [rough.js](https://roughjs.com/) — el çizimi SVG primitifleri, **MIT, <9KB gzip** ([GitHub](https://github.com/rough-stuff/rough))
   - [Excalifont](https://plus.excalidraw.com/excalifont) — Excalidraw'un resmi el yazısı fontu, **OFL-1.1** (gömme serbest)
5. **Ayrışma:** jenerik "AI diyagramı" değil, tanınır bir Usta imzası.

**Değerlendirilen alternatifler (reddedildi):** 3Blue1Brown/Manim paleti (matematik animasyonu için altın standart ama cilalı-vektörel stil mentor-tahta metaforuna uzak; ikinci seçenek olarak not edildi) · Linear/Stripe (SaaS-jenerik, eğitim hissi yok) · Lottie/After Effects tarzı (model üretemez).

**Offline kuralı revizyonu (v1'den fark):** "web fontu yok" kuralı → **"AĞ isteği yok"** olarak netleşti. Excalifont'un Latin subset'i **woff2 data-URI olarak iskelete gömülür** (~15-25KB beklenir; latin-extended Türkçe karakterleri kapsamalı — ç ğ ş ö ü ı İ test et). Sayfa yine tek dosya, sıfır ağ isteği. Fallback stack: `'Excalifont', 'Comic Sans MS', 'Segoe Print', cursive, system-ui`.

---

## Görev 1 — Claude Design'da tasarım sistemini kur

Claude Design'da proje aç. Excalidraw yönünde bir tasarım sistemi oluştur ve örnek anlatıcı sayfasıyla iterate et (örnek içerik: "internet nasıl çalışır" — 2 node, ok, paket, not, altyazı, kontrol barı; İKİ TEMA).

Dondurulacak token'lar:
- **Zemin:** açık tema = sıcak kağıt (saf beyaz DEĞİL; hafif krem/doku hissi), koyu tema = tahta/antrasit (mevcut ekran görüntüsündeki gibi ama kağıt dokusuyla uyumlu). 
- **Mürekkep:** çizgi/yazı renkleri (açıkta koyu mürekkep, koyuda tebeşir-beyazı).
- **Vurgu paleti:** 3-4 "marker/fosforlu kalem" rengi — Usta turuncusu (`#e8862e/#f09040` ailesi) birincil vurgu olarak KORUNUR (TUI kimliğiyle akrabalık); yanına 2-3 ikincil marker (ör. mavi, yeşil) highlight/pulse çeşitliliği için.
- **Tipografi:** Excalifont — caption/etiket/not hiyerarşisi (boyut+ağırlık); sayaç/kontroller system-ui kalabilir (UI chrome ≠ tahta içeriği ayrımı).
- **Bileşen eskizleri:** node (rough dikdörtgen, hafif roughness), circle, arrow (rough çizgi + el çizimi ok ucu), packet (dolgulu marker noktası), note (fosforlu zemin + mürekkep yazı — ekran görüntüsündeki turuncu-çerçeve note hissi iyi, rafine et), caption barı, kontrol butonları (sketch çerçeveli), sahne sayacı.
- **Motion timing:** anime.js easing adlarıyla — sketch dünyasına uygun hafif organik (örn. `easeOutBack` girişler, `easeInOutSine` paket); süre ölçeği: hızlı 300 / normal 600 / paket 1100ms.
- **Roughness ölçeği:** rough.js `roughness` (öneri 1-1.5) + `bowing` değerleri — TUTARLI tek ayar; her element aynı elden çıkmış görünmeli. `seed` sabitlenir ki replay'de şekil titremesin (rough.js seed parametresi — deterministik çizim).

Çıktı: token listesi (CSS custom properties + JS sabitleri) — Anil'e Claude Design önizlemesiyle göster, ONAY AL, dondur.

## Görev 2 — Vendor + iskelete taşı

1. **Vendor:** `src/vendor/rough.min.js` (MIT başlığı korunur; roughjs bundled UMD). Excalifont latin subset woff2 → data-URI olarak doğrudan skeleton CSS `@font-face`'e (ayrı dosya gerekmez; base64 satırı uzun olur, sorun değil).
2. **İskelet:** `src/visual_skeleton.html` — token'ları uygula; `/*__ROUGH__*/` placeholder'ı ekle, `build_visual_html` üç enjeksiyon yapar (rough + anime + scenes; `replacen` sırası testlerle sabitlenir).
3. **Motor değişikliği:** `makeElement`/`arrow`/`note` şekilleri rough.js generator ile üretir (`rc.rectangle/ellipse/line`). **KRİTİK TEKNİK NOT:** `packet` animasyonu `anime.path()` ile TEMİZ bir path ister; rough çizgisi çok-parçalı sketch path'i üretir. Çözüm: okun altına **görünmez düz `<path>`** (stroke:none) koy — anime.path onu izler, rough path sadece görsel. Ok ucu da rough çizgilerle elle çizilir (marker-end yerine — marker'a rough uygulanamaz).
4. **Değişmezler (testler korur, KIRMA):** placeholder'lar (`/*__ANIME__*/`, `/*__SCENES__*/[]` + yeni `/*__ROUGH__*/`), element ID'leri (`prev/play/next/caption/counter/stage/root`, `el-` öneki), class isimleri (`node/circ/hl/lbl/arrow/alabel/notebox/packet`), `prefers-color-scheme` iki tema, sıfır ağ isteği (`<script src`/`<link `/`fetch(` yok), anime.js + rough.js MIT başlıkları, deterministik replay semantiği.
5. Test güncellemeleri: skeleton marker testine `/*__ROUGH__*/` + `@font-face` + `Excalifont`; build testine rough başlık kontrolü.

## Görev 3 — Kompozisyon kuralları (Katman 2, v1'den taşındı)

`src/visual.rs::visual_system()` prompt'una **Composition (binding)** bölümü:

```
Composition (binding):
- Align to an 8px grid: all x/y/w/h values are multiples of 8.
- Keep at least 40px margin from stage edges; distribute elements, don't cluster.
- Flow direction: left→right for processes, top→down for hierarchies. Never zigzag.
- Same kind = same size (e.g. all server nodes share identical w/h).
- Labels are at most 3 words; longer explanations go in captions or notes, never inside nodes.
- One focal point per scene: at most one pulse or new highlight at a time.
```

Test needle'ları: `"8px grid"`, `"focal point"`, `"3 words"` — prompt↔test birebir.

## Görev 4 — Doğal dil tetiklemesi: `[[show: …]]` marker'ı

Kullanıcı sohbette açıkça görsel isterse ("göster", "çiz", "show me", "animasyonla anlat") `/show` yazmak zorunda kalmasın — kullanıcı istedi, izin sorunu yok.

- **Mekanizma:** Model, YALNIZ kullanıcı açıkça görsel istediğinde, cevabının SON satırına `[[show: <kısa konu>]]` koyar. Kabuk cevabı basmadan marker'ı ayıklar (görünen metinden SİLİNİR), sonra mevcut `/show <konu>` akışını otomatik koşar (mini-oturum → HTML → tarayıcı).
- **Davranış kuralı** (SOUL/TEACHING'e tek paragraf): marker yalnız açık kullanıcı talebiyle; Usta kendi kararıyla koymaz — proaktiflik "istersen /show yaz" önerisi olarak kalır.
- **Kod:** `pub fn extract_show_marker(reply: &str) -> (String, Option<String>)` (visual.rs; temiz metin + konu) — son satırda `[[show: ...]]` arar, case-insensitive, birden fazlaysa sonuncusu geçerli hepsi silinir. İki loop'ta `page_reply`/`print_reply` ÖNCESİ uygulanır; marker varsa yanıt basıldıktan sonra görsel akışı tetiklenir. Görsel akışı zaten iki loop'ta var — ortak yardımcıya çekmek serbest (parite şart).
- **Test:** marker'lı yanıt → temiz metin + Some(konu); marker'sız → aynen + None; ortada geçen `[[show:` (son satır değil) → dokunma; iki marker → tek tetik.
- **Sınır:** marker'lı otomatik tetik de `reset_session` paritesine uyar; Esc iptali çalışır; başarısız JSON'da aynı "try /show again" bildirimi.

## Görev 5 — Görsel dosya yaşam döngüsü (retention)

- Konu başına **son 10** görsel: `visual_path` yazımından önce aynı klasördeki eski dosyaları tarih sırasıyla buda (11.+ sil).
- Scaffold `.usta/visuals/.gitignore` yazar (`*` içerikli) — görseller diskte kalır, kullanıcının git repo'suna sızmaz.
- `usta reset <konu>` artık `visuals/<konu>/` klasörünü de siler (onay mesajında belirtilir). `--factory` zaten `.usta`'yı komple siliyor — değişiklik yok.
- Test: prune (12 dosya → 10 kalır, en yeniler), gitignore scaffold, topic-reset temizliği.

## Görev 6 — Doğrula + teslim

1. `cargo test -p usta && cargo build -p usta` yeşil, uyarısız.
2. Demo: `cargo test -p usta --lib visual::tests::demo_html_for_manual_check -- --nocapture` → tarayıcıda İKİ temada Claude Design onaylı tasarımla karşılaştır; Türkçe karakterler Excalifont'ta düzgün mü (ç ğ ş ı) — değilse subset'i genişlet.
3. Spec güncelle (`2026-08-12-visual-explainer-design.md`): "## Tasarım Dili" bölümü + dondurulmuş token listesi + marker + retention davranışı.
4. Feature branch `usta-visual-design` → task başına commit → Anil onayı → main merge → `cargo install --path .` → push.

## Kapsam dışı

Tema seçici/config · TUI (ratatui) yeniden tasarımı · yeni op/element tipi · rough.js'i canvas modunda kullanmak (SVG kalır) · Excalifont dışında ek font.

## Riskler / dikkat

- **rough path + anime.path çatışması** → Görev 2 madde 3'teki görünmez-path çözümü zorunlu; atlanırsa paket animasyonu kırılır.
- **rough seed sabitlenmezse** her replay'de şekiller farklı titrer → deterministik replay bozulmuş GÖRÜNÜR. Seed'i element id'sinden türet (aynı element hep aynı çizim).
- **Font subset Türkçe kapsamı** — demo'da mutlaka Türkçe caption dene.
- **Dosya boyutu** — rough (~9KB gzip) + font (~20KB) + anime (17KB) ≈ görsel başına ~50-60KB; kabul edilebilir, retention (Görev 5) birikmeyi sınırlar.
