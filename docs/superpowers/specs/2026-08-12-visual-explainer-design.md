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

Kendi kendine yeten tek HTML dosyası: inline CSS + vanilla JS, **CDN/ağ erişimi YOK** (offline çalışır). İçerik:
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
