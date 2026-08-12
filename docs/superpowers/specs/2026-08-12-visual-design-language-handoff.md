# Handoff — Görsel Anlatım Tasarım Dili (Claude Design ile)

**Tarih:** 2026-08-12 · **Yürütücü:** /show'u implemente eden session (Opus) · **Araç:** Claude Design (mcp__claude-design__*) + DesignSync
**Bağlam:** `/show` özelliği main'de merge'li (d3a6465). Görsel dil şu an "çalışan default" — Anil kararı: tasarım dili önemli, kurallaştırılacak. Çerçeve: **iki katman.**

## İki Katmanlı Çerçeve (bağlayıcı ilke)

1. **Katman 1 — Görsel dil (KOD, zorunlu):** renk, tipografi, spacing, motion timing, bileşen stilleri `src/visual_skeleton.html` CSS'inde yaşar. Model stil SEÇEMEZ — sadece sahne JSON'u üretir. Bu görev: bu katmanı Claude Design ile tasarlayıp iskelete taşımak.
2. **Katman 2 — Kompozisyon (PROMPT, model davranışı):** yerleşim kuralları `src/visual.rs::visual_system()`'e eklenir. Bu görev: aşağıdaki kuralları prompta işlemek.

## Görev 1 — Tasarım dilini Claude Design'da üret

Claude Design'da bir proje aç; görsel anlatıcının örnek bir sayfasını tasarla (örnek içerik: "internet nasıl çalışır" — 2 node, 1 ok, 1 paket, 1 not, altyazı + kontrol barı). İterasyonla şunları netleştir:

- **Palet:** zemin/panel/çizgi/vurgu/not renkleri — koyu + açık tema çifti. Mevcut turuncu vurgu (`#e8862e/#f09040`) Usta kimliği: koru veya bilinçli revize et (revize edersen TUI turuncusuyla — `Color::Indexed(208)` — akraba kalsın).
- **Tipografi:** başlık/altyazı/etiket/not hiyerarşisi. **SERT KISIT: web fontu YOK** — yalnız `system-ui` yığını (offline şartı; sayfa hiçbir ağ isteği yapamaz). Ağırlık/boyut/spacing ile hiyerarşi kur.
- **Bileşen stilleri:** node (kutu), circle, arrow (+ok ucu), packet, note (callout), caption barı, kontrol butonları (prev/play/next), sahne sayacı.
- **Motion timing:** fade/move/arrow-draw/packet süreleri + easing'ler — tutarlı bir zaman ölçeği (örn. hızlı=300ms, normal=600ms, vurgu=1100ms). anime.js easing adlarıyla ifade et.

Çıktıyı **design token** listesi olarak dondur: CSS custom property seti + timing sabitleri.

## Görev 2 — İskelete taşı (`src/visual_skeleton.html`)

Tasarımı iskeletin `<style>` bloğuna ve gerekiyorsa markup'a uygula. **Değişmezler (testler bunları korur, kırma):**
- Placeholder'lar aynen: `<script>/*__ANIME__*/</script>` ve `const SCENES = /*__SCENES__*/[];`
- Element ID'leri: `prev`, `play`, `next`, `caption`, `counter`, `stage`, `root` + `el-` öneki
- Class isimleri motor tarafından kullanılıyor: `node`, `circ`, `hl`, `lbl`, `arrow`, `alabel`, `notebox`, `packet`
- `prefers-color-scheme` iki tema; hiçbir `<script src`/`<link `/`fetch(`/web fontu YOK
- anime.js vendor dosyasına ve MIT başlığına dokunma
- Motor JS'ine yalnız timing sabitlerini token'lardan okuyacak kadar dokun (davranış değişmez)

## Görev 3 — Kompozisyon kuralları (`visual_system` + test)

`src/visual.rs::visual_system()` prompt'una **Composition (binding)** bölümü ekle:

```
Composition (binding):
- Align to an 8px grid: all x/y/w/h values are multiples of 8.
- Keep at least 40px margin from stage edges; distribute elements, don't cluster.
- Flow direction: left→right for processes, top→down for hierarchies. Never zigzag.
- Same kind = same size (e.g. all server nodes share identical w/h).
- Labels are at most 3 words; longer explanations go in captions or notes, never inside nodes.
- One focal point per scene: at most one pulse or new highlight at a time.
```

`visual_system_carries_schema_and_pedagogy` testine needle'lar ekle: `"8px grid"`, `"focal point"`, `"3 words"`. Prompt ile test birebir anlaşsın.

## Görev 4 — Belgele + doğrula + teslim

1. Spec'e bölüm ekle (`docs/superpowers/specs/2026-08-12-visual-explainer-design.md`): "## Tasarım Dili" — iki katman ilkesi + dondurulmuş token listesi.
2. `cargo test -p usta && cargo build -p usta` yeşil (skeleton marker testleri dahil).
3. `cargo test -p usta --lib visual::tests::demo_html_for_manual_check -- --nocapture` → demo'yu tarayıcıda AÇ, iki temada kontrol et (Claude Design'daki tasarımla eşleşiyor mu).
4. Feature branch (`usta-visual-design`) → commit'ler → Anil onayıyla main'e merge. `cargo install --path .`.

## Kapsam dışı

TUI (ratatui) tasarımı · yeni op/element tipi · tema seçici/config · web fontu gömme (subset dahi olsa v1'de yok).
