# Tasarım — Reflow politikası girdisi: kör sayıyı bırak, terminali tanı (v0.31.0)

**Tarih:** 2026-09-02
**Kapsam:** `Screen`'in silme aritmetiği tek bir kör sayı olmaktan çıkar; terminalin yeniden-sarma davranışı bir GİRDİ haline gelir ve her politika için silme TAM hesaplanır.
**Durum:** Onaylandı → implement (Anil: "A", 2026-09-02)
**Önceki:** `docs/superpowers/specs/2026-09-02-screen-model-harness-design.md` (koşum, Task 1–3 tamamlandı). K1–K5 (`2026-09-01-relative-render-design.md`) aynen geçerlidir.

## Neden — ölçülmüş imkânsızlık

Koşum üç senaryoyu kırmızıya çevirdi ve düzeltme denemesi (`ff036a6`) kanıtladı ki tek sayıyla çözülmüyor:

- **`Reflow`** politikasında bloğu temizlemek için **en az** `Σ ceil(w_i / yeni_genişlik)` satır silinmeli.
- **`NoReflow`** politikasında metin kaybetmemek için **en fazla** `painted` satır silinebilir.
- Daraltmada birinci ikinciyi aşar. Aynı bit-özdeş durumdan iki politika **zıt** davranış istiyor.

Ölçülen bedel: `ff036a6` kırpmayı kaldırınca `Reflow` düzeliyor ama `NoReflow`'da 200→60 daralmasında transcript imhası 2 satırdan **4 satıra** çıkıyor. Bekçi de kördü — iki `Reflow` senaryosunun bloğun üstünde gerçek metni yok, yalnız boş satır var; fazla silme onlara görünmüyor.

**Sonuç: politika tahmin edilemez, BİLİNMELİDİR.**

## Karar

**P1 — Politika bir girdidir.** `Screen` kurulurken belirlenir ve silme aritmetiği ona göre TAM hesaplanır. Kırpma (`painted..=painted*2`) tamamen kalkar — kırpma, bilinmeyeni ortalama etme denemesiydi; politika bilinince ortalamaya gerek yok.

- `Reflow`: `rewrapped = Σ ceil(w_i / new_width)`, `descend` bugünkü formül.
- `NoReflow`: her mantıksal satır bir fiziksel satır kalır → `rewrapped = painted`, `descend = cursor_up`.

**P2 — Politika ortamdan okunur, kullanıcıya sorulmaz.** Saf fonksiyon:

```
detect_reflow(get: impl Fn(&str) -> Option<String>) -> ReflowPolicy
```

Sıra:
1. `USTA_TERM_REFLOW` = `1`/`true` → `Reflow`; `0`/`false` → `NoReflow`. (Kaçış kapısı ve test kancası; her şeyin önünde.)
2. **Çoklayıcı içindeysek → `NoReflow`**, dıştaki terminal ne olursa olsun: `TERM` `screen`/`tmux` ile başlıyorsa veya `TMUX` tanımlıysa. Izgarayı çoklayıcı yönetir, dıştakinin davranışı geçersizdir.
3. Bilinen sarıcılar → `Reflow`: `TERM_PROGRAM` ∈ {`vscode`, `iTerm.app`, `Apple_Terminal`, `ghostty`, `WezTerm`, `Hyper`} · `TERM` ∈ {`xterm-kitty`, `xterm-ghostty`} · `KITTY_WINDOW_ID` / `WEZTERM_PANE` / `VTE_VERSION` / `WT_SESSION` tanımlı.
4. Aksi halde → **`NoReflow`**.

**P3 — Varsayılanın yönü savunulabilir olmalı.** Bilinmeyen terminalde `NoReflow` seçilir çünkü iki hatanın bedeli eşit değil:
- Eksik silme → **kalıntı**: çirkin, ama geri alınabilir; yeni çıktı geldikçe yukarı kayar.
- Fazla silme → **kullanıcının metnini yok eder**: geri gelmez.
Anil bunu iki kez söyledi (v0.29.1 ve v0.30.1 redleri). Şüphede metin korunur.

**P4 — Yanlış tespitin bedeli ÖLÇÜLÜR ve yazılır.** Uyuşmazlık vakaları teste girer: model `NoReflow` + Screen `Reflow` → metin kaybı; model `Reflow` + Screen `NoReflow` → kalıntı. Bunlar sessiz risk değil, belgeli beklenti olur. Varsayılanın yönü sayesinde sahada gerçekleşmesi muhtemel olan ikincisidir.

**P5 — `ff036a6` KORUNUR, kapıya alınır.** Kırpmayı kaldırması `Reflow` için doğruydu; yanlış olan onu her politikaya uygulamaktı.

## Davranış

- `enum ReflowPolicy { Reflow, NoReflow }` — üretim tipi (test-only `screen_model::ResizePolicy` ile aynı kavram, ayrı tip; model test tarafında kalır).
- `Screen::new(out, size, policy)`; `term::setup()` `detect_reflow(|k| std::env::var(k).ok())` ile çağırır.
- `rewrapped_rows` ve `descend_rows` politikayı parametre alır; `painted*2` kırpması silinir.
- Politika bir kez, kurulumda belirlenir; oturum boyunca değişmez (terminal değişmez).
- Kullanıcıya görünen hiçbir çıktı eklenmez — tespit sessizdir. `/context` veya benzeri bir yüzeye satır EKLENMEZ.

## Test

- Birim `detect_reflow` (sahte `get`): override iki yönde · `TMUX` tanımlıyken bilinen sarıcı terminal → `NoReflow` (çoklayıcı önceliği) · `TERM=screen-256color` → `NoReflow` · `TERM_PROGRAM=vscode` → `Reflow` · `KITTY_WINDOW_ID` → `Reflow` · boş ortam → `NoReflow` · override çoklayıcıyı da ezer.
- Birim: `rewrapped_rows`/`descend_rows` her iki politikada; `NoReflow`'da sırasıyla `painted` ve `cursor_up` döner; hiçbir yerde `painted*2` yok.
- **Senaryo matrisi (koşum):** beş senaryo × eşleşen politika (model `Reflow`+Screen `Reflow`, model `NoReflow`+Screen `NoReflow`) → **hepsi yeşil, `#[ignore]` kalmaz.**
- **Uyuşmazlık testleri (P4):** iki çapraz kombinasyon, beklenen bozulma açıkça assert edilir — kaybın/kalıntının ölçüsü teste yazılır ki ileride sessizce büyümesin.
- Kaynak-pin: üretim kaynağında `saturating_mul(2)` YOK; K3 taraması temiz; her `paint` `Clear(FromCursorDown)` ile biter.
- Elle doğrulama (Anil): **önce VS Code entegre terminali** (bozulduğu yer) → taze oturum, ekran ortasında blok, daralt/genişlet/sürükle → kural satırı tam 2, üstteki metin duruyor. Sonra `USTA_TERM_REFLOW=0 usta` ile aynı testler → kalıntı görülebilir ama **metin kaybı OLMAMALI** (varsayılanın yönünün doğrulaması).

## Kapsam dışı

- Çalışma anında politika tespiti (ekran okunamıyor; CPR yasağı sürüyor)
- Terminal listesinin genişletilmesi — liste büyüyen bir şey, ihtiyaç çıktıkça eklenir
- `screen_model` test modelinin üretime taşınması
