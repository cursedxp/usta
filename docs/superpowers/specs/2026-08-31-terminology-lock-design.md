# Tasarım — TERMINOLOGY LOCK: dili aynala, alanın sözlüğünü çevirme (v0.29.2)

**Tarih:** 2026-08-31
**Kapsam:** Usta kullanıcının dilinde yanıt verirken alanın yerleşik terimlerini uyduruk sade karşılıklarla değiştiriyor; sonuç anlaşılmaz, aranamaz ve bazen olgusal olarak yanlış cümleler. Alan-bağımsız sorun — bugün Rust'ta görüldü, aynısı tıpta/psikolojide/hukukta olur.
**Durum:** Onaylandı → implement (Anil, 2026-08-31)

## Semptom (iki canlı oturum kanıtı)

**Kanıt 1 — `std::env::args()` anlatımı:**
> "Verilen **kelimelerin birincisi** senin yazdığın isim değil." … "Senin istediğin **sıfırıncı** değil, **birinci**."

`argument` → "kelime". Ve "birinci" dört cümle arayla önce index 0'ı, sonra index 1'i işaret ediyor. Anil: *"şurasından ben hiçbir şey anlamıyorum."*

**Kanıt 2 — cargo/stagit anlatımı:** Aynı yanıtın içinde kural hem tutuyor hem çöküyor.
- Tutuyor: `cargo`, `stagit`, `src/main.rs`, `cargo build --release`, `mentor/PROJECT.md` olduğu gibi; "atölye" ve "fırın/ekmek" analojileri isabetli.
- Çöküyor:
  - `argument` → "kelime", yine iki kez ("senin programına giden kelime", "senin yazacağın kelime").
  - Aynı kavram iki isimle: "tek bir stagit **dosyası**" ↔ "tek **binary** olarak dağıtılıyor".
  - Aynı kavram iki isimle: "makine diline **çevrilmesi**" ↔ "cargo sana geçici bir tane **derliyor**".
  - **Olgusal hata:** "Bu çeviri işini yapan alete cargo diyoruz" / "cargo → o metni çalıştırılabilir bir programa çeviren alet". Derleyen `rustc`; cargo build system, `rustc`'yi çağırır. Terimden kaçarken olgu bozulmuş.

## Kök neden

1. **Nasıl yazılacağını söyleyen kural yok.** `SOUL.md:7` LANGUAGE LOCK yalnız *hangi dilde* yazılacağını belirler. Sistem promptunun tamamı İngilizce (`SOUL/RULES/TEACHING/GOAL/USER`, artı v0.20.0'da bilinçli İngilizceye çevrilen `src/tokens.rs` protokol token'ları) — model İngilizce çerçevede kurup Türkçe render ediyor, calque cümle çıkıyor.

2. **Sadeleştirme kuralının karşı-ağırlığı yok.** `SOUL.md` Voice: *"a curious high-schooler should be able to follow"* + Jargon rule: *"the first time you use a new term, define it in one plain-language sentence"*. Terimi KORUMAYI söyleyen tek bir satır yok. Model de sadeleştirmeyi ismin üstüne uyguluyor.

3. **Modelin fiilî sınırı yanlış yerde.** Koruduğu şey **isme benzeyen** şey: özel ad, monospace, komut (`cargo`, `stagit`). Paraphrase ettiği şey **sıradan kelimeye benzeyen** terim (`argument`, `binary`, `compile`). Oysa koruma egzotik görünmekten değil, **alanın o şeye verdiği ad olmaktan** gelir.

## Karar

**K1 — Doğru terim "İngilizce olan" DEĞİL.** Alan-bağımsızlık burada kırılır: Türkçe tıp "kalp yetmezliği" der, "heart failure" demez; hukukta terimler zaten Türkçedir; yazılımda "argüman"/"index" ödünç kelimedir. Ölçüt tek: **o alanın uygulayıcısı, kullanıcının dilinde konuşurken hangi kelimeyi kullanıyor.**

**K2 — Alan-bağımsız test:** kullanıcı o kelimeyi (a) aratabiliyor mu, (b) alanın bir uygulayıcısına söylediğinde anlaşılıyor mu. "Verilen kelimeler" ikisini de geçemiyor; "argüman", "kalp yetmezliği", "maruz bırakma" geçiyor.

**K3 — Terim istikrarı oturum boyunca.** Bir kavram = bir kelime. Aynı kelime iki kavramı taşıyamaz ("birinci" hem index 0 hem index 1), aynı kavram anlatım ortasında kelime değiştiremez ("dosya"/"binary", "çevirme"/"derleme").

**K4 — Yanlış cümle kuran sadeleştirme, sadeleştirme değildir.** Bu üslup meselesi değil, `RULES.md` Rule 2 (*DON'T MAKE THINGS UP*) ihlalidir; bugüne kadar kimse ikisini birbirine bağlamamış. Basit cümle yanlışsa başka bir basit cümle bulunur, yanlış olan gönderilmez.

## Davranış

`SOUL.md`'ye, LANGUAGE LOCK paragrafının HEMEN ALTINA yeni bir blok girer (bağlayıcı metin — implementasyon bunu birebir yazar):

```
**TERMINOLOGY LOCK: mirroring the user's language does NOT mean translating the
field's vocabulary. Simplify the explanation, never the name.**

- **Use the term practitioners of that field actually use when speaking the
  user's language.** Sometimes that is a borrowed word (index, commit,
  deadlift), sometimes a native one (Turkish medicine says "kalp yetmezliği",
  not "heart failure"), sometimes both circulate. Pick what a colleague in that
  field would say out loud. NEVER invent a third, plainer word of your own: a
  paraphrase no practitioner uses is worse than jargon — the user cannot search
  it, cannot read a source with it, and is not understood when they repeat it.
- **A term is not protected by looking exotic.** Proper nouns, commands and
  monospace names survive on their own; the ones that get lost are the terms
  that look like ordinary words — argument, binary, compile, exposure,
  remission, consideration. Those are terms too. Name them, don't describe them.
- **Gloss once, then keep using the real term.** First appearance: the real term
  plus one plain sentence defining it. After that, the term alone. Explaining a
  concept simply and naming it correctly are two separate jobs — do both.
- **One concept, one word, for the whole session.** Never let one word carry two
  meanings, never swap words for the same concept mid-explanation. That is where
  an explanation stops meaning anything.
- **Precision outranks simplicity when they collide.** A simplification that
  makes a FALSE statement is not a simplification — it is a Rule 2 violation
  (don't make things up). "cargo compiles your code" is wrong: rustc compiles,
  cargo drives it. If the simple sentence is false, find another simple
  sentence; never ship the false one.
- **Write natively in the user's language; never compose in another language and
  translate.** A sentence that reads like a translation is a defect. Watch
  numbers, positions and orderings especially: say them the way that language
  actually says them, and check that the sentence still names exactly one thing.
```

Jargon rule (Voice bölümü) DEĞİŞMEZ — yeni blok onu daraltmaz, sınırını çizer: tanımla, ama ismi koru.

## Dağıtım

`SOUL.md` `Ownership::Code` (`src/defaults.rs:24`) — `usta init` mevcut global kuruluma üzerine yazar, **migration gerekmez**. Kullanıcı-sahipli dosyalara (`USER.md`, `learner/`) dokunulmaz.

## Test

- Pin (`src/defaults.rs` test modülü, mevcut `teaching_promise_matches_ride_along_watcher` emsali): `include_str!("../SOUL.md")` `TERMINOLOGY LOCK` başlığını, "Simplify the explanation, never the name" cümlesini ve beş çekirdek maddenin iğnelerini taşır. Amaç: ileride biri Voice bölümünü sadeleştirirken bloğu sessizce düşüremesin.
- Pin: blok LANGUAGE LOCK'tan SONRA gelir (iki başlığın byte konumu karşılaştırılır) — dil kuralı önce, sözlük kuralı hemen ardından.
- Prompt bütçesi: blok ~1.4KB; `SOUL.md` her turda yükleniyor. Bilinçli maliyet, prompt diet (v0.19.0) ilkesiyle çelişmiyor — bu kabuğun deterministik çözebileceği bir şey değil.
- Elle doğrulama (Anil): aynı `std::env::args()` sorusu tekrar sorulur → "argüman" geçmeli, "kelime" geçmemeli; index 0/1 tek anlamla adlandırılmalı. Ardından **teknoloji dışı bir alan** denenir (ör. bir sağlık/psikoloji konusu) → uydurma sade karşılık yerine alanın gerçek terimi + tek cümlelik açıklama.

## Kapsam dışı

- Sistem promptunun Türkçeleştirilmesi (protokol token'ları İngilizce kalır — v0.20.0 kararı)
- Kabuk tarafında terim denetimi / yasak kelime listesi (model işi, deterministik değil)
- `TEACHING.md`, `RULES.md` metinlerinde değişiklik
