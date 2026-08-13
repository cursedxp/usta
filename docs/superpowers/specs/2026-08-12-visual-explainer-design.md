# Tasarım — Görsel Anlatım (`/show`)

**Tarih:** 2026-08-12
**Kapsam:** Roadmap #1. Usta bir kavramı animasyonlu, kendi kendine yeten bir HTML sayfasıyla anlatır. Gömülü iskelet + model sahne senaryosu üretir. `/show` komutu + anlaşılmadı-sinyalinde proaktif öneri.
**Durum:** Onaylandı (brainstorm 2026-08-12) → plan hazır, başka session'da koşulacak.

## Amaç

Bazı kavramlar görerek anlaşılır ("internet nasıl çalışır?"). Usta sözel anlatımın yetmediği yerde animasyonlu görsel üretir. Kalite tutarlılığı için serbest HTML üretimi DEĞİL: binary'ye gömülü, elle yazılmış **iskelet** (oynatıcı + animasyon motoru) + modelin ürettiği **deklaratif sahne JSON'u**.

Brainstorm kararları:
- Mimari: **gömülü iskelet + sahne senaryosu** (serbest HTML reddedildi — kalite zar atışı).
- Tetik: **`/show [konu]`** slash komutu + **proaktif öneri** (Usta anlaşılmadı sinyalinde `/show` önerir; kendisi izinsiz tarayıcı AÇMAZ).
- Roadmap sırası: bu özellik önce (docs/ROADMAP.md).

## Mimari

### 1. İskelet — `src/visual_skeleton.html` (gömülü, `include_str!`)

Kendi kendine yeten tek HTML dosyası: inline CSS + JS, **CDN/ağ erişimi YOK** (offline çalışır).

**Animasyon katmanı: vendorlanmış [anime.js v3.2.2](https://animejs.com) (MIT, ~17KB, `src/vendor/anime.min.js`, repoya commit'li).** Karar gerekçesi (2026-08-12 revizyonu): tween mekaniği (easing, kesinti, SVG path-following) el yapımı rAF yerine kanıtlanmış kütüphaneye bırakılır — "paket ok boyunca akar" animasyonu anime.js'in `anime.path()` çekirdek özelliği. Kabuk `build_visual_html` içinde `/*__ANIME__*/` placeholder'ına kütüphaneyi enjekte eder; çıktı yine tek dosya + offline. MIT lisans başlığı dosyada korunur. Oynatıcı kabuğu (sahne yönetimi, prev/next/play, deterministik replay) uygulama mantığıdır ve bizim kodumuz olarak kalır. İçerik:
- **Sahne alanı:** SVG stage (viewBox `0 0 800 450`), responsive.
- **Altyazı barı:** aktif sahnenin `caption`'ı.
- **Kontroller:** ◀ önceki · ▶ oynat/duraklat · ▶| sonraki · sahne sayacı (`3/8`).
- **Tema:** `prefers-color-scheme` ile koyu/açık.
- **Animasyon motoru** (~200-300 satır JS): `const SCENES = /*__SCENES__*/[];` placeholder'ını çalıştırır. Sahneler kümülatif (state sonraki sahneye taşınır); "önceki" deterministik replay ile çalışır (op'lar animasyonsuz uygulanır).

### 2. Sahne JSON şeması (model çıktısı — kontrat)

Model YALNIZ bir JSON dizisi üretir (fence'li olabilir — `clean_markdown_reply` soyar). Kabuk `serde_json` ile doğrular, geçersizse dosya yazılmaz + hata bildirimi.

```json
[
  {
    "caption": "The browser prepares a request",
    "duration": 4000,
    "ops": [
      {"op":"add","el":{"id":"b","type":"node","x":80,"y":200,"w":140,"h":70,"label":"Browser"}},
      {"op":"add","el":{"id":"s","type":"node","x":580,"y":200,"w":140,"h":70,"label":"Server"}},
      {"op":"arrow","id":"a1","from":"b","to":"s","label":"GET /"},
      {"op":"packet","along":"a1","label":"GET"},
      {"op":"pulse","id":"s"},
      {"op":"highlight","id":"s","on":true},
      {"op":"move","id":"b","x":100,"y":100},
      {"op":"remove","id":"a1"},
      {"op":"note","x":400,"y":60,"text":"DNS = the internet's phone book","id":"n1"}
    ]
  }
]
```

- **Element tipleri:** `node` (yuvarlatılmış kutu + etiket), `circle` (`cx,cy,r,label`), `text` (`x,y,text,size?`).
- **Op'lar (v1):** `add` (fade-in), `remove` (fade-out), `move` (yumuşak taşıma), `arrow` (iki element arasına animasyonlu ok, opsiyonel `label`), `packet` (ok boyunca hareket eden nokta — ağ/akış anlatımının bel kemiği), `pulse` (dikkat çekme), `highlight` (kalıcı vurgu aç/kapa), `note` (kısa açıklama balonu; `text` elementinin sugar'ı olabilir).
- Sahne başına `duration` (oynat modunda bekleme, default 3500ms).

### 3. Rust tarafı — yeni `src/visual.rs`

```rust
pub fn visual_system() -> String            // model sistem promptu: şema + pedagoji kuralları
pub fn parse_show_command(line: &str) -> Option<Option<String>>
    // "/show" → Some(None) (son anlatılan kavram); "/show tcp handshake" → Some(Some("tcp handshake")); değilse None
pub fn build_visual_html(scenes_json: &str) -> anyhow::Result<String>
    // JSON'u doğrula (serde_json::Value dizi mi + her sahnede caption var mı), iskeletteki /*__SCENES__*/ placeholder'ına enjekte et
pub fn visual_path(project_root: &Path, topic: &str, concept_slug: &str) -> PathBuf
    // .usta/visuals/<topic>/<YYYY-MM-DD-HHMMSS>-<slug>.html (chrono; benzersiz ad, üzerine yazma derdi yok)
pub fn open_in_browser(path: &Path) -> bool
    // macOS `open`, Linux `xdg-open`; spawn başarısızsa false — çağıran yolu bildirir
```

`visual_system()` pedagoji kuralları: 6-12 sahne; sahne başına TEK fikir; caption'lar kullanıcının diliyle (dil aynası — konuşma hangi dildeyse o); somut benzetme kullan; SOUL seviye kalibrasyonu geçerli; element sayısını az tut (sahnede ≤6 görünür element).

### 4. Akış — `/show`

1. İki loop'ta da (`/watch` deseniyle) intercept — satır LLM oturum geçmişine GİRMEZ.
2. Kavram: argüman verildiyse o; verilmediyse **oturumdaki son asistan mesajı** bağlam olarak mini-oturuma verilir ("bunu görselleştir"). Oturum boşsa (hiç yanıt yok): bildirim "explain something first, or use /show <topic>".
3. **Mini-oturum** (slug deseniyle birebir): `system = visual_system()`, tek user mesajı = kavram + (varsa) son anlatım alıntısı. TUI'de `ask_live` (spinner + Esc iptal çalışır), plain'de `ask_usta`. Bitince `backend.reset_session()` — ana oturum kirlenmez (spec B1 paritesi).
4. Yanıt → `clean_markdown_reply` → `build_visual_html` → `visual_path`'e yaz (`create_dir_all` + `fs::write`).
5. `open_in_browser`; başarısızsa da her durumda yol bildirilir: `visual saved: <path>`.
6. Ana oturum geçmişine hiçbir turn eklenmez (v1 kararı — gelecekte "kullanıcı animasyonu izledi" sinyali değerlendirilebilir, şimdilik YAGNI).

### 5. Proaktif öneri — SOUL.md (tek satır)

"Anlaşılmadı sinyali" maddesine ek: farklı benzetmeyle yeniden anlatmanın yanına — *"if the concept is visual/spatial (flows, architectures, protocols), offer the animation: 'want me to show this visually? type /show'"*. Usta kendisi tarayıcı açmaz, komutu önerir.

### 6. help.rs

`/show [topic]` satırı In-session commands bölümüne eklenir + test substring'i.

## Tasarım Dili

Görev 1-5'in çıktısı (uygulandı). İki katmanlı bir ayrım: **stil kod'da donmuş, kompozisyon modelde serbest.** Model hiçbir zaman renk/font/çizim-stili SEÇMEZ — sadece Bölüm 2'deki şema ile sahneyi *kurar* (hangi element nerede, hangi op ne zaman).

### Katman 1 — Kod'da donmuş token'lar (model görmez, değiştiremez)

Kaynak: Claude Design projesi "usta-visual-explainer" (Görev 1, Anil onaylı — `.superpowers/sdd/frozen-tokens.md`). `visual_skeleton.html` içine gömülü; `visual_system()` promptunda bu değerlerden tek kelime geçmez.

```
LIGHT (warm paper):
--bg:#efe7d6; --paper:#f7f1e3; --ink:#33302b; --ink-soft:#6f685c; --line:#3d3730;
--accent:#e8862e; --marker-blue:#1e6fd9; --marker-green:#2f9e44; --marker-violet:#9c36b5;
--note-bg:#ffedcf; --note-border:#e8862e; --paper-dot:rgba(51,48,43,.05); --shadow:rgba(51,42,20,.14);

DARK (charcoal board):
--bg:#14161a; --paper:#1e2024; --ink:#ece7dd; --ink-soft:#a39a8b; --line:#d9d3c7;
--accent:#f09040; --marker-blue:#5b9bff; --marker-green:#5bc46e; --marker-violet:#cc7ee8;
--note-bg:#33291a; --note-border:#f09040; --paper-dot:rgba(236,231,221,.045); --shadow:rgba(0,0,0,.45);

Motion (anime.js): TIMING={fast:300,normal:600,pulse:700,packet:1100}ms
                   EASE={enter:'easeOutBack',move:'easeInOutQuad',pulse:'easeInOutSine',packet:'easeInOutSine'}

Çizim (rough.js): roughness:1.2 · bowing:1.1 · strokeWidth(node/note:2, arrow:2.5, highlight:3) · fillStyle:'solid'
                  seed = FNV-1a(element id) % 10000 → element başına deterministik (replay'de titreme yok)
                  node stroke=--ink · arrow (2 rough çizgi, marker-end YOK) stroke=--line ·
                  note stroke=--note-border fill=--note-bg · packet r=8 stroke+fill=--accent ·
                  highlight = 6px pad'li ek rough rect, stroke=--accent
                  buton çerçevesi: border-radius:255px 14px 225px 14px/14px 225px 14px 255px (el çizimi hissi)

Tipografi: Excalifont (latin-extended woff2 data-URI, OFL-1.1, ağ isteği yok) — fallback 'Comic Sans MS','Segoe Print',cursive,system-ui
           Boyutlar: caption 21 · node 22 · note 18 · arrow-label 17px · UI chrome system-ui kalabilir
```

Tema seçimi tarayıcının `prefers-color-scheme`'ine bırakılır — Usta hangi temada olduğunu bilmez/sormaz.

### Katman 2 — Promptta kompozisyon kuralları (`visual_system()`, `src/visual.rs`)

Model'e verilen **bağlayıcı** kurallar stil değil, yerleşim ve pedagojidir:
- 8px grid'e hizala (tüm x/y/w/h 8'in katı); kenarlardan ≥40px boşluk; elemanları dağıt, kümeleme.
- Akış yönü: süreçler için soldan-sağa, hiyerarşiler için yukarıdan-aşağı — asla zigzag.
- Aynı tür = aynı boyut (ör. tüm server node'ları aynı w/h).
- Etiket ≤3 kelime; uzun açıklama caption'a veya note'a gider, node içine değil.
- Sahne başına tek odak noktası (aynı anda en fazla bir pulse/yeni highlight).
- 6-12 sahne, sahne başına TEK fikir, kümülatif inşa; caption kullanıcının diliyle; ≤6 görünür element; anlam taşıyan hareket (akış=packet, tepki=pulse, hatırla=highlight); somut benzetme bir note'ta; kapanışta özet sahnesi.

Model bu kuralları JSON `ops` diziyle uygular (Bölüm 2'deki şema) — `add/arrow/packet/pulse/highlight/move/remove/note`. Renk, font, çizgi kalınlığı, easing gibi hiçbir görsel karar modelin elinde değildir; bunlar Katman 1'de sabittir.

### Katman 3 — Motor emniyeti (anti-overlap)

Katman 2 modelin niyetidir; motor `2026-08-13-visual-overlap-engine-design.md`'de tanımlı ikinci bir emniyet katmanı taşır — model önerir, motor garanti eder:
- Her kutu-tipi eleman (node/circle/note/text) için AABB (eksen-hizalı sınır kutusu) hesaplanır.
- Ok uçları artık sabit pad yerine kaynak/hedef AABB kenarından + 12px boşlukla başlar.
- `add`/`note` op'unda eleman eklenmeden önce mevcut AABB'ler 24px şişirilerek çarpışma test edilir; çakışırsa deterministik bir arama (sabit yön sırası, 8px adım) ilk boş konumu bulur ve spec koordinatına delta olarak uygulanır.
- DOM'a eklendikten sonra `getBBox()` ile gerçek kutu alınır; tahminden büyükse (ör. uzun metin) tek seferlik ikinci bir düzeltme turu çalışır.
- `move` op'u kasıtlı olarak nudge YAPMAZ — modelin anlatı hareketi bozulmasın diye.
- Arama tamamen deterministik olduğundan replay (◀ / ▶|) her seferinde aynı yerleşimi üretir.

### `[[show: …]]` işaretçisi (Görev 4 — doğal dil tetikleyici)

Açık komut (`/show [konu]`) dışında, normal sohbet yanıtı içinde model `[[show: <konu>]]` yazarak görselleştirmeyi kendi önerebilir — ama SADECE kullanıcının açık isteği üzerine (ör. "bunu çizer misin", "görsel göster"); Usta kendiliğinden görsel dayatmaz. Davranış (`extract_show_marker`, `src/visual.rs`):
- İşaretçi yalnızca yanıtın **son satırında**, tek başına ise tanınır (baştaki/sondaki boşluk tolere edilir, `show` case-insensitive). Metin ortasında veya son satır değilse dokunulmaz.
- Tanınırsa: yanıttaki TÜM tekil işaretçi satırları temizlenir (birden fazla varsa sonuncusunun konusu kazanır), kullanıcı işaretçiyi asla görmez, oturum geçmişine de girmez.
- **Marker-only yanıt** (temizlik sonrası metin boş kalırsa): boş asistan mesajı Messages API'yi bozacağı için, `(visual explainer: <konu>)` sentetik bir yer tutucu yanıt yerine geçer — hem boş-mesaj hatasını önler hem "burada bir görsel gösterildi" bağlamını gelecekteki turn'lere taşır.
- Tetiklenince: mevcut `/show` akışının birebir aynısı çalışır (izole mini-oturum → JSON → HTML → tarayıcı) ve `backend.reset_session()` her çıkış yolunda çalışır (bkz. Görev 6 carry-forward düzeltmesi, `src/tui/run.rs`/`src/main.rs`).

### Saklama politikası (Görev 5)

- **Konu başına son 10:** her `/show` sonrası `prune_visuals` `.usta/visuals/<topic>/` altındaki `.html` dosyalarını dosya adı (=zaman damgası) sırasına göre sıralar, en eski fazlalıkları siler — yazma tamamlandıktan SONRA çalışır, yani "10" her zaman diskteki gerçek son sayıdır.
- **Git'e girmez:** proje scaffold'u `.usta/visuals/.gitignore` (`*`) yazar — üretilen görseller repo'ya asla commit edilmez, brain notu değil geçici çıktıdır.
- **`usta reset <topic>`:** progress dosyasıyla birlikte `.usta/visuals/<topic>/` tamamen silinir (`remove_topic_visuals` — dizin hiç oluşmamışsa da hatasız geçer, idempotent).
- **`usta reset --factory`:** katalogdaki her projenin `.usta/` klasörünü (dolayısıyla tüm konuların tüm görsellerini) siler — Usta'yı fabrika ayarına döndürür, seçici değildir.

## Hata durumları

- Model çıktısı geçerli JSON değil → bildirim `visual generation failed (invalid scene data) — try /show again`, dosya yazılmaz. Retry YOK (v1) — kullanıcı tekrar dener.
- `open` başarısız → yol bildirilir, kullanıcı elle açar.
- Esc ile iptal → slug iptaliyle aynı: `backend.reset_session()`, bildirim.
- Yazma hatası → `error: {e}` bildirimi, oturum yaşar.

## Test

- `parse_show_command`: `/show`, `/show tcp`, `  /show  x  `, negatifler (`/showx`, `show`, `/watch`).
- `build_visual_html`: geçerli JSON → çıktı `<!doctype`/`<svg` + sahne verisi içerir, placeholder kaybolur; geçersiz JSON → `Err`; caption'sız sahne → `Err`; boş dizi → `Err`.
- `visual_path`: doğru dizin + slug + `.html`.
- İskelet statik kontrol: `visual_skeleton.html` `/*__SCENES__*/`, kontrol butonları, `prefers-color-scheme` içerir.
- help testine `/show` substring'i.
- Elle duman (insan): `/show how does the internet work` → tarayıcıda oynatıcı; prev/next/play; koyu/açık tema; paket animasyonu akıyor.

## Kapsam dışı (v1 — YAGNI)

- `usta visuals` listeleme komutu (dosyalar diskte, yeterli).
- Görsel geçmişinin ana oturuma sinyali.
- JSON hatasında otomatik retry.
- Ses/etkileşimli quiz sahneleri.
- İskeleti aşan özel JS üretimi (hibrit reddedildi).

## Açık sorular

Yok.
