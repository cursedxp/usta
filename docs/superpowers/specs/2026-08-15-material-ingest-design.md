# Tasarım — Materyal Yutma (Roadmap #5)

**Tarih:** 2026-08-15
**Kapsam:** Kullanıcı ders materyalini (kitap/notlar/kurs metni) görünür `materials/` klasörüne koyar; konu tanışmasında Usta materyali keşfeder, müfredat haritasını **materyalin bölümlerine demirleyerek** kurar. md/txt çekirdek; PDF, sistemde `pdftotext` varsa otomatik metne çevrilir.
**Durum:** Onaylandı (Anil, 2026-08-15: "md/txt çekirdek + pdftotext opsiyonel" + "materials/ klasörü + otomatik keşif") → writing-plans
**Bağımlılık:** Yok.

## Amaç

Roadmap #5: "Kullanıcı PDF/kitap/kurs verir, müfredat onun etrafına kurulur." Bugün müfredat web-araştırmadan türüyor; kullanıcının elindeki asıl kaynak (kitap, kurs notu) devrede değil.

**Kritik kısıt:** Usta'nın oturum içi lokal dosya-okuma aracı YOK (yalnız web araştırma + kayıt-tetikli watcher). Materyal içeriği modele tek yoldan akabilir: kabuğun ürettiği **digest** (başlık iskeleti + bölüm başı alıntılar, boyut-sınırlı) tanışma turuna enjekte edilir. Tam metin okuma İSTENMİYOR zaten — pedagojik duruş: **okumayı kullanıcı yapar**, Usta müfredatı materyale demirler, bölüm atar, sorar, sınar. Usta özetleyici değil.

Kararlar:
- **Format:** `.md` + `.txt` çekirdek. `.pdf`: `pdftotext` PATH'teyse yanına `.txt` üretilir (cache: `.txt` PDF'ten yeniyse dönüştürme atlanır); yoksa tek satır bilgi: "PDF found but pdftotext missing — convert to text or `brew install poppler`".
- **Konum:** görünür `materials/` klasörü, scaffold kurar (mentor/, exercises/ deseni).
- **Otomatik keşif:** yeni konu tanışmasında `materials/` boş değilse digest'ler tanışma turuna eklenir; Usta kullanıcıya SORAR ("bu materyali müfredata demirleyeyim mi?") — dayatmaz (birden çok konu aynı projede olabilir, materyal başka konunun olabilir).
- **Demirleme kalıcı:** müfredat haritası maddelerine bölüm referansı yazılır (`— kaynak: <dosya> §<bölüm>`); harita zaten persist, sonraki oturumlar digest'e ihtiyaç duymaz. Web araştırma kuralı kalkmaz — materyalin kapsamadığı kritik konu haritaya web'den eklenir (kapsam bekçiliği).
- **Digest saf ve deterministik** (kabuk üretir, LLM değil): md → başlık ağacı + her bölümün ilk ~200 karakteri; txt → dosya başı ~1000 karakter + satır/boyut bilgisi. Sınırlar: dosya başına ~8KB, toplam ~16KB (aşan kırpılır + `[truncated]` notu — sessiz kırpma yok).

## Davranış

### 1. Modül: `src/materials.rs` (yeni)

- `pub struct Material { pub name: String, pub digest: String }`
- `pub fn scan(project_root: &Path) -> Vec<Material>` — `materials/` altında (rekürsif, gizli/`.gitkeep` hariç) `.md`/`.txt` dosyaları; `.pdf` için önce dönüştürme denenir (aşağıda), üretilen `.txt` normal akışa girer (aynı isimli `.txt` zaten varsa PDF atlanır — çift sayım yok). Alfabetik sıra, deterministik.
- `pub fn digest_md(content: &str, cap: usize) -> String` — `#`-başlık satırları hiyerarşiyle listelenir; her başlığın altına ilk ~200 karakter (tek satıra indirilmiş) alıntı. `cap` aşılırsa kırpılır + `\n[truncated]`.
- `pub fn digest_txt(content: &str, cap: usize) -> String` — ilk ~1000 karakter + `\n[... N lines, M KB total]`.
- `pub fn convert_pdfs(dir: &Path) -> Vec<String>` — `pdftotext` PATH'te mi (`which` benzeri kontrol — mevcut `claude_on_path` deseni); her `.pdf` için yanına `.txt` (`pdftotext -layout x.pdf x.txt`); `.txt` mevcutsa ve mtime'ı PDF'ten yeniyse atla. Dönüş: kullanıcıya gösterilecek bilgi satırları ("converted: x.pdf → x.txt" / "PDF found but pdftotext missing ...").
- Sabitler: `PER_FILE_CAP = 8_000`, `TOTAL_CAP = 16_000` (byte değil karakter; UTF-8 sınırı güvenli kırpma — `char_indices` ile).

### 2. Tanışma entegrasyonu

- `onboarding_prompt` (`src/progress.rs`) yeni parametre: `materials: Option<&str>` — Some ise blok eklenir:

```text
[COURSE MATERIAL FOUND]
The user has material under materials/ — outline digests below. ASK whether to
anchor this topic's curriculum to this material (they may be for another topic).
If yes: build the curriculum map FROM its chapters/sections — each map item
carries a source ref (`— kaynak: <file> §<section>`); assign reading from it
(the USER reads — you don't summarize the book); still add critical items the
material lacks, from web research (scope guarding). If no: proceed normally.
---
<digests>
---
```

- Çağrı yerleri (TUI `src/tui/run.rs` + plain `src/main.rs`): yeni-konu yolunda `materials::convert_pdfs` bilgi satırları notice olarak basılır, ardından `materials::scan` → digest'ler `name` başlıklarıyla birleştirilir → `onboarding_prompt`'a geçilir. Mevcut konu (resume/opening) yolunda materyal enjeksiyonu YOK (harita zaten demirli).
- `opening_prompt` DEĞİŞMEZ.

### 3. Kapanış kuralı (`closing_prompt` — curriculum kuralına tek ek)

Curriculum kural cümlesine: "If the map was anchored to course material, KEEP the source refs (`— kaynak: <file> §<section>`) on every item; new items from web research are marked (`— kaynak: web`)."

### 4. Scaffold

`write_project_scaffold`: `materials/` + `.gitkeep` (mentor/exercises deseniyle birebir; sayaç testi +1).

### 5. TEACHING.md — `## Course Material` bölümü

- Materyal = kullanıcının okuyacağı kaynak; Usta bölüm atar, okunanı drill'le sınar, egzersizi bölüme bağlar (exercise loop ile birleşir: "read §3, then write exercises/<konu>/ch3-notes.md").
- Özetleme yasağı: "your job is anchoring and questioning, not summarizing the book into the chat".
- Materyal-web dengesi: materyal ana omurga, web tamamlayıcı.

### 6. Dokümantasyon

SPEC.md yeni § (sıradaki numara, v0.14) · README Highlights satırı (İngilizce) · ROADMAP #5 ✅ + Tamamlananlar · Cargo.toml `0.14.0` + tag `v0.14.0` (sürüm politikası: madde bitti → minor bump).

## Test

- `digest_md`: başlıklar hiyerarşik listelenir, bölüm alıntıları tek satır, cap kırpması `[truncated]` ekler, UTF-8 (Türkçe içerik) panic'siz.
- `digest_txt`: baş alıntı + satır/boyut dip notu; cap.
- `scan`: tmpdir'de md+txt bulur, `.gitkeep`/gizli atlar, pdf'in yanında `.txt` varsa pdf'i saymaz, alfabetik, `materials/` yoksa boş vec.
- `convert_pdfs`: `pdftotext` YOKKEN "missing" satırı döner, `.txt` üretmez (CI-güvenli — pdftotext'li yol elle doğrulamada).
- `onboarding_prompt`: `materials=Some` → `COURSE MATERIAL FOUND` + digest içerir + "ASK whether to anchor"; `None` → blok yok. Mevcut çağrı yerleri `None`/gerçek değerle güncellenir.
- `closing_prompt`: `kaynak:` ref-koruma kuralını içerir.
- Scaffold: `materials/` + `.gitkeep`.
- Sürüm assert: `0.14.0`.

## Kapsam dışı

- Oturum-ortası materyal ekleme/yeniden-ingest (yeni dosya sonraki konu tanışmasında keşfedilir; mevcut konuya elle demirleme v2).
- Gömülü PDF kütüphanesi; EPUB/DOCX.
- Materyal içeriğinin system prompt'ta kalıcı taşınması (yalnız tanışma turu).
- `/ingest` komutu.
- Digest'in `.usta/` altına persist edilmesi (tanışma turu transcript'te zaten yaşıyor; harita demirleme kalıcılığı yeterli).

## Açık sorular

Yok.
