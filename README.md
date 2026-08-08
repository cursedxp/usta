# Usta

> Terminal'de çalışan, seni **yaparak öğreten** Socratic mühendislik mentoru.
> Kod yazmaz, uydurmaz, senin yerine düşünmez — **sorar, sınar, yol gösterir.**

Usta bir "kod tamamlayıcı" değil. Yanında oturan bir usta: sen gerçek işi yaparken
seni yetiştirir. Amaç senin kodun değil, **senin gelişmen.**

Domain-agnostik — Rust, JavaScript, marketing, ne öğreniyorsan. İlk kullanıcı: yazarı.

---

## Felsefe

- **Yaparak öğrenme.** Pasif ders yok. Gerçek projeyi inşa ederken, o akışın içinde öğrenirsin.
- **Sıfır otonom aksiyon.** Usta neyin hatalı olduğunu ve nasıl yaklaşman gerektiğini gösterir — ama **kodu sen yazarsın.** Kopyala-yapıştır çözüm vermez.
- **Uydurmaz.** Bilmiyorsa web'de araştırır, sonra öğretir.
- **Senior gibi.** Ölçeğe duyarlı mimari (1 kişilik vs 1000 kişilik), teknoloji seçimi, kod kalitesi.
- **"İnce kabuk, kalın beyin."** Davranış Rust'ta değil, düzenlenebilir markdown dosyalarında yaşar (`USTA.md`, `learner/`, `approaches/`).

## Öne çıkanlar

| | |
|---|---|
| 🧠 **Kalıcı hafıza** | Oturum kapanışında ne öğrendiğini `progress/<konu>.md`'ye yazar. Sonraki oturum bunu bilir — tekrar anlatmaz, eksiği hedefler. |
| ⚡ **Gerçek proaktiflik** | Dosyayı kaydettiğin an feedback gelir — Enter'a basmana gerek yok (debounce + `tokio::select!`). İlk görüşte tam içerik, sonra unified diff. |
| 🎓 **Pedagoji katmanı** | Açılış drilli (geri çağırma), anlat-modu (Feynman), ipucu merdiveni (fading), tahmin protokolü (`cargo check` sonucunu söylemez — önce tahmin ettirir). |
| 🔎 **Araştırma** | Bilmediğini web'de arar (WebSearch) — uydurma yok. |
| 🌍 **Her konu** | Rust'a özgü değil. Yeni konuda (Linux güvenliği, GTM, ne olursa) Usta yaklaşımı **tanışmayla türetir**, web-araştırmalı **müfredat haritası** çıkarır (`görülmedi → görüldü → oturdu → derinleşildi`). Kapsam bekçiliği: havada hiçbir şey kalmaz. |
| 🎨 **Terminal arayüzü** | Claude Code tarzı inline TUI (ratatui): açılışta çift kolonlu hoş-geldin kutusu (öğrenme durumu + sırada ne var), canlı dört-kenarlı girdi kutusu, yapışık durum satırı (spinner + bağlam göstergesi). Akış normal scrollback'te kalır — yukarı kaydır, kopyala. Pipe/`NO_COLOR`'da otomatik düz mod (script'ler bozulmaz). |
| 🗂️ **Yönetim** | `usta topics` nerede ne öğrendiğini gösterir; `reset` konuyu veya her şeyi sıfırlar. |

## Kurulum

```bash
git clone https://github.com/cursedxp/usta
cd usta
cargo build --release
# opsiyonel: cargo install --path .
```

**LLM backend** (ikisinden biri yeter):

1. **Claude CLI (default, önerilen)** — [Claude Code](https://claude.com/claude-code) PATH'te ise Usta onu kullanır. Mevcut aboneliğin, **API key gerekmez**.
2. **Anthropic API** — `export ANTHROPIC_API_KEY=sk-ant-...`

`USTA_BACKEND=cli|api` ile zorlayabilirsin.

## Kullanım

```bash
usta                    # başlat — konuyu sorar (en kısa yol, bunu yaz)
usta start rust         # konuyu baştan ver — konu argümanı 'start' ister
usta topics             # nerede ne öğreniyorum? (katalog)
usta reset rust         # bu projedeki Rust progress'ini sıfırla (onaylı)
usta reset --profile    # yalnız profilini sıfırla — Usta seni tanımadan başlar (yedek: profile.md.bak)
usta reset --factory    # her şeyi sıfırla — Usta seni hiç tanımamış gibi (kelime onaylı)
usta init               # sadece iskeleti kur (opsiyonel — start zaten kurar)
```

> **Not:** Konuyu argümanla vermek istersen `start` şart — `usta rust` "bilinmeyen komut" verir (ilk arg komut sanılır). Konusuz `usta` ise oturumu açıp konuyu sorar. Herhangi bir projede çalıştır: proje `.usta/` yoksa sessizce kurulur.

Bir öğrenme oturumu:

```
usta start gtm            # rust olabilir, gtm olabilir, ne olursa
  → yeni konu (progress yok) → TANIŞMA: Usta yaklaşımı + müfredat haritasını türetir
  → (varsa) açılış drilli: haritadan 2-3 hatırlama sorusu + "neredeyiz, sırada ne var"
  → çalışırsın (kod / plan.md / ne olursa), kaydedersin
  → Usta proaktif Socratic feedback verir (kodu/işi yapmadan)
  → cargo check hatası varsa: söylemez, "nerede patlar?" diye tahmin ettirir
  → /quit → progress + approach + curriculum güncellenir, katalog yenilenir
```

## Arayüz

Etkileşimli terminalde Usta **ratatui inline-viewport TUI**'si açar: alt bölgede canlı girdi kutusu + durum satırı yaşar, kalıcı akış (Usta yanıtları, dosya feedback'i) normal **scrollback**'e basılır — yukarı kaydırıp geçmişi okuyabilir/kopyalayabilirsin. Alternate screen kullanılmaz; terminal geçmişin korunur.

Konusuz `usta` çalıştırıldığında (Claude Code tarzı): önce **kimlik-welcome** kutusu üstte görünür (logo + kayıtlı konuların), altındaki girdi kutusu konuyu sorar — kısa yaz ya da cümleyle anlat (cümleyi model kısa bir slug'a indirir). `usta start <konu>` ise **tam-mod welcome** (öğrenme durumu: seviye, harita %, sırada ne var) gösterip doğrudan başlar.

TTY yoksa veya `NO_COLOR=1` ise TUI hiç açılmaz — mevcut düz satır moduna düşer (pipe/CI/script güvenli).

## Nasıl çalışır — "ince kabuk, kalın beyin"

Rust yalnızca kabuk: CLI, LLM client, dosya izleyici (`notify`), `cargo check` koşucusu, markdown yükleyici. **Zekâ ve kişilik markdown'da** yaşar:

```
~/.config/usta/          # GLOBAL beyin (bir kez kurulur, tüm projelerde paylaşılır)
  USTA.md                #   çekirdek davranış + pedagoji kuralları
  USER.md                #   sen kimsin (öğrenme tarzın) — canlı belge, aşağıya bak
  learner/index.md       #   ## Kayıtlar — konu | proje | tarih kataloğu
  approaches/            #   software.md, _default.md — domaine göre yaklaşım

<proje>/.usta/           # PROJE (her projede ayrı)
  learner/progress/<konu>.md      #   seviye, gap'ler, geri çağırma soruları, hata günlüğü
  learner/curriculum/<konu>.md    #   web-araştırmalı müfredat haritası (durum etiketli)
  approaches/<konu>.md            #   Usta'nın türettiği konuya özel yaklaşım (canlı belge)
```

Davranışı değiştirmek = markdown'ı düzenle, Rust'a dokunma. (Global davranış dosyaları güncellenince — scaffold var olanı ezmez — yenilemek için: `rm ~/.config/usta/USTA.md ~/.config/usta/approaches/_default.md` + bir kez `usta`; ya da `usta reset --factory`.)

**Profil yaşam döngüsü:** `USER.md` canlı belgedir — Usta seni tanımıyorken (profil jenerik) oturum başında kısaca tanışır (ad, öğrenme tarzı, 1-2 soru), oturum kapanışında öğrendiği kişi-hakkında bilgiyi (konu bilgisi değil) profiline işler. Kullandıkça seni daha iyi tanır. `usta reset --profile` ile tek komutta unutturursun (eski hali `USER.md.bak`'ta) → sonraki oturumda yeniden tanışır. Profili elle düzenlersen Usta yazdığını ezmez.

## Durum

v0.6 · Rust 2021 · 125 birim test. Tasarım kararları: [`SPEC.md`](SPEC.md). Çekirdek davranış: [`USTA.md`](USTA.md).

Yol haritası fikirleri: streaming, çoklu terminal sağlamlaştırma, kendi-sağlık-denetimi (link/tutarlılık), tech-notes cache.
