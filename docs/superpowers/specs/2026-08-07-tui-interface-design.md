# Spec — Claude Code Tarzı TUI Arayüzü (ratatui inline viewport)

**Tarih:** 2026-08-07
**Durum:** Onaylandı (Anil, 2026-08-07)
**Kapsam:** `usta start` etkileşimli arayüzünün ratatui'ye taşınması: açılış karşılama kutusu + canlı kenarlı girdi kutusu + yapışık durum satırı.

## 1. Amaç

Usta'nın etkileşimli oturumu Claude Code hissi versin:

1. **Açılış karşılama kutusu** — çift kolonlu, çerçeveli: sol tarafta logo + selamlama + oturum bilgisi, sağ tarafta dinamik "Öğrenme Durumu".
2. **Girdi kutusu** — kullanıcının yazdığı alan dört kenarı çizili, YAZARKEN canlı (sağ kenar dahil), uzun satırda kutu içinde wrap.
3. **Akış korunur** — Usta yanıtları ve bildirimler normal terminal scrollback'inde kalır; kaydırma, kopyalama, geriye bakma bozulmaz.

## 2. Kapsam Dışı (non-goals)

- Alternate screen TUI (tam ekran uygulama) — scrollback kaybı kabul edilemez.
- Yanıt bloğu görsel dilinin değişmesi — `●` + markdown render aynı kalır.
- Renk temasının değişmesi — turuncu 208 + yeşil 114 aynı.
- Mouse desteği, pane/split, syntax-highlight'lı girdi.
- `usta init` / `usta topics` gibi tek-atımlık komutların görünümü (dokunulmaz).

## 3. Mimari — Inline Viewport

```
┌─ terminal scrollback (normal akış, kaydırılabilir) ─┐
│  [açılış kutusu — bir kere basılır]                  │
│  ● Usta yanıtı (markdown)                            │
│  · bildirimler                                       │
│  │ > kullanıcının GÖNDERİLMİŞ satırı (tarihçe izi)   │
│  ● sonraki yanıt...                                  │
├─ inline viewport (ratatui, canlı, yapışık alt bölge)─┤
│  ╭──────────────────────────────────────────╮        │
│  │ > yazılmakta olan metin▌                 │        │
│  ╰──────────────────────────────────────────╯        │
│  ⠋ düşünüyor… · ▓▓░░░░░░ bağlam 42k/1000k            │
└──────────────────────────────────────────────────────┘
```

- `Terminal::with_options(Viewport::Inline(N))` — alt bölge girdi kutusu (dinamik 3+ satır) + 1 durum satırı.
- Kalıcı içerik **`terminal.insert_before()`** ile viewport'un ÜSTÜNE, gerçek scrollback'e yazılır.
- Kullanıcı Enter'a basınca: yazdığı satır `│ > …` izi olarak `insert_before` ile scrollback'e basılır, girdi kutusu boşalır.

### Olay döngüsü

Mevcut `main.rs` tokio `select!` döngüsü genişler; **`input.rs`'in rustyline thread'i ve `ready` el-sıkışması TUI yolunda kalkar**:

```
select! {
    tuş olayı   (crossterm EventStream)  → editöre uygula / Enter → turn başlat
    watcher olayı                        → mevcut davranış (feedback turn'ü)
    LLM yanıtı  (spawn edilmiş future)   → insert_before ile bas, durum satırını temizle
    tick        (spinner animasyonu)     → viewport yeniden çiz
}
```

LLM çağrısı sırasında girdi kutusu görünür kalır ama gönderim kilitlidir (yazılabilir, Enter kuyruğa alınmaz — tek turn ilkesi korunur; durum satırı "⠋ düşünüyor…" gösterir).

## 4. Bileşenler

Yeni modül: `src/tui.rs` (gerekirse `tui/` alt-modüllere bölünür: `welcome.rs`, `editor.rs`, `convert.rs`).

| Birim | Ne yapar | Girdi → Çıktı |
|---|---|---|
| `WelcomeData` | Açılış kutusu verisi | profil/progress/curriculum içerikleri → struct |
| `render_welcome(data, width)` | Kutu widget'ı | `WelcomeData` → ratatui `Text`/`Paragraph` |
| `InputBox` | Canlı girdi kutusu | `tui-input` state sarmalar; render + tuş uygulama |
| `LineHistory` | Up/Down girdi tarihçesi | `Vec<String>` + imleç; rustyline history'nin yerini alır |
| `ansi_to_text(s)` | termimad ANSI → ratatui | `ansi-to-tui` sarmalayıcı; hata → düz metin fallback |
| `StatusLine` | Spinner + bağlam göstergesi | durum enum + token → tek satır |
| `page(term, text)` | Kalıcı içerik basma | `insert_before` sarmalayıcı |

**Saf/IO ayrımı:** veri çıkarımı (aşağıda §5) ve render fonksiyonları saf — `TestBackend` + string girdiyle test edilir. Terminal/event IO sadece ana döngü sarmalayıcısında.

## 5. Açılış Kutusu

### Görünüm (onaylı)

```
╭─── Usta v0.1.0 ─────────────────────────────────────────────╮
│                                │ Öğrenme Durumu             │
│   ██  ██ ██████ ██████ ██████  │ Konu: rust · orta          │
│   ██  ██ ██       ██   ██  ██  │ Harita: %34                │
│   ██  ██ ██████   ██   ██████  │ ────────────────────────── │
│   ██████      ██   ██   ██  ██ │ Sırada                     │
│                                │ Ownership: lifetimes…      │
│   Tekrar hoş geldin, Anil!     │ Drill: 3 soru hazır        │
│   opus · cli                   │                            │
│   ~/Documents/…/usta           │                            │
╰─────────────────────────────────────────────────────────────╯
```

- Genişlik: `min(terminal_genişliği, 100)`. Logo turuncu (208).
- Bir kere, oturum başında `insert_before` ile basılır; mevcut tek satır `ui::banner()`'ın TUI yolundaki yerini alır.

### Veri kaynakları (hepsi best-effort; parse hatası = alan atlanır, asla panik/uyarı yok)

| Alan | Kaynak | Kural |
|---|---|---|
| Sürüm | `env!("CARGO_PKG_VERSION")` | başlıkta `Usta v{X}` |
| İsim | global `learner/profile.md` H1 | `# Öğrenci Profili — Anil` → em-dash sonrası; yoksa isimsiz "Tekrar hoş geldin!" |
| Model | `backend.label()` | mevcut fonksiyon |
| Dizin | proje kökü | `$HOME` → `~`; genişliğe sığmazsa ortadan `…` |
| Konu + seviye | progress `## Seviye` bölümü ilk dolu satır | progress yoksa sağ kolon = ilk-oturum modu |
| Harita % | curriculum satırlarında durum sayımı | `durum ≠ görülmedi` madde / toplam durumlu madde; curriculum yoksa satır atlanır |
| Sırada | curriculum'daki İLK `görülmedi` madde | madde metni kırpılır (`…`) |
| Drill | progress `## Geri çağırma soruları` soru sayısı | `Drill: N soru hazır` |

**İlk oturum modu:** progress yoksa sağ kolon tek mesaj: "İlk oturum — tanışmayla başlarız".

## 6. Girdi Kutusu

- Dört kenar canlı: `╭─╮ │ > metin▌ │ ╰─╯`. Kenar rengi soluk (DIM); odak hep bu kutuda.
- Wrap: satır kutu iç genişliğini aşınca alt satıra sarar, kutu yüksekliği büyür (viewport yüksekliği yeniden hesaplanır). Üst sınır ~10 satır; aşarsa iç kaydırma.
- Tuşlar: `tui-input` standardı (karakter, backspace/delete, ←/→, Home/End, kelime-silme) + Up/Down = `LineHistory`, Enter = gönder, Ctrl-C/Ctrl-D = kapanış akışı (mevcut kapanış davranışıyla birebir: progress flush vb.).
- Türkçe karakterler: genişlik hesabı `unicode-width` ile (ç/ğ/ş 1 hücre — ama doğru API'den).
- Enter sonrası: girdi `│ > …` soluk iz olarak scrollback'e basılır, kutu boşalır.
- Boş satır gönderilmez (mevcut davranış).

## 7. Kapanış ve Hata Yolları

- **Panik/erken çıkış:** ratatui guard (drop'ta terminal restore — raw mode kapat, imleç geri). Panic hook'a restore eklenir; bozuk terminal bırakılmaz.
- **Resize:** crossterm Resize olayı → viewport genişliği yeniden hesap, kutu yeniden çizilir. Açılış kutusu geriye dönük yeniden çizilmez (scrollback'te sabit — kabul).
- **Kapanış flush'ı** (progress/approach/curriculum üretimi): TUI kapatıldıktan SONRA mevcut satır-tabanlı çıktıyla koşar — spinner/notice davranışı bugünkü gibi.

## 8. Düşüş Yolu (plain mode)

`ui::is_plain()` (TTY değil / `NO_COLOR`) → **TUI hiç başlamaz**; bugünkü yol aynen: `input.rs` rustyline thread'i, `sen> ` promptu, düz satır çıktılar. `input.rs` bu yüzden SİLİNMEZ — plain yolun girdi katmanı olarak kalır. Pipe/test senaryoları değişmez.

## 9. Bağımlılıklar

| Crate | Neden | Not |
|---|---|---|
| `ratatui` | viewport + widget | `Viewport::Inline` + `insert_before` destekleyen güncel sürüm |
| `tui-input` | satır editörü state'i | imleç/edit; ratatui sürümüyle uyumlu seçilir |
| `ansi-to-tui` | termimad ANSI → `Text` | dönüşüm hatasında düz metin fallback |
| `unicode-width` | hücre genişliği | padding/kırpma hesapları |

Crossterm zaten termimad üzerinden ağaçta — **sürüm çakışması plan'ın ilk görevinde doğrulanır**; ratatui'nin beklediği crossterm ile termimad'ınki uyuşmazsa çözüm: ratatui'nin kendi re-export'u kullanılır, termimad sadece render'da kalır (event tarafına dokunmaz — çakışma pratikte izole).

`rustyline` bağımlılığı kalır (plain yol).

## 10. Test Stratejisi

- **Saf veri:** isim çıkarımı, seviye/harita %/sırada/drill parse'ları — string girdili unit testler; eksik dosya/bozuk içerik fallback'leri dahil.
- **Render:** ratatui `TestBackend` ile açılış kutusu (iki kolon hizası, kırpma, ilk-oturum modu) ve girdi kutusu (kenarlar, wrap, boş durum) buffer karşılaştırması.
- **Genişlik:** tüm kutu satırları eşit görünür genişlik (unicode-width toplamı) — Türkçe karakterli örneklerle.
- **Editör:** tuş dizisi → beklenen metin+imleç; history up/down.
- **Plain yol:** mevcut testler dokunulmadan geçmeli (regresyon kapısı).
- Elle doğrulama (plan sonunda): gerçek terminalde resize, uzun yanıt, watcher kesmesi, Ctrl-C.

## 11. Riskler

| Risk | Azaltma |
|---|---|
| crossterm sürüm çakışması (termimad vs ratatui) | Plan görev 1: `cargo tree` doğrulaması; gerekirse termimad'ı render-only izole et |
| `insert_before` + spinner tick etkileşimi (titreme) | durum satırı sadece tick'te çizilir; insert_before öncesi durum satırı temizlenir |
| Viewport yükseklik dinamiği (wrap büyümesi) | üst sınır + iç kaydırma; TestBackend testi |
| Watcher olayı yazım ortasında içerik basar | insert_before doğası gereği güvenli — girdi kutusu altta sabit kalır (bugünkü satır-karışması sorununu da ÇÖZER) |
| Kapanışta terminal restore edilemezse bozuk shell | drop-guard + panic hook |

## 12. Başarı Ölçütü

1. `usta start rust` → çift kolonlu açılış kutusu gerçek verilerle çiziliyor.
2. Yazarken girdi kutusunun dört kenarı sabit; wrap'te kutu büyüyor, bozulmuyor.
3. Usta yanıtları scrollback'te — terminalde yukarı kaydırıp okunabiliyor, kopyalanabiliyor.
4. LLM beklerken spinner + bağlam göstergesi durum satırında; girdi kutusu görünür.
5. `NO_COLOR=1 usta start rust` ve pipe'lı çalıştırma bugünkü davranışla birebir.
6. Ctrl-C/D → temiz kapanış: terminal restore + kapanış flush'ı çalışıyor.
