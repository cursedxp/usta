# USTA — İndeks

Bu dosya davranış İÇERMEZ ve modele YÜKLENMEZ — insan haritasıdır. Bir şeyi düzeltmek istediğinde hangi dosyaya bakacağını burada bul.

## Müdahale Haritası

| Belirti | Dosya |
|---|---|
| Ton/kişilik/anlatım anlaşılmıyor, bağlamıyor | `SOUL.md` |
| Yanlış davranış: kod yazdı, uydurdu, mekaniği anlattı, dosya ezdi | `RULES.md` |
| Öğretme biçimi: drill, ipucu zamanlaması, spek kadansı, tanışma | `TEACHING.md` |
| Sınav/hedef takibi, tempo, format pratiği | `GOAL.md` |
| Kullanıcı hakkında yanlış/eksik bilgi | `USER.md` (ya da `usta reset --profile`) |

## Yükleme Sırası (`brain.rs`)

```
SOUL.md → RULES.md → TEACHING.md → [GOAL.md, yalnız approach'ta "## Hedef" varsa]
→ approaches/(software|_default).md → approaches/<konu>.md
→ USER.md → progress → curriculum → [BUGÜN bölümü mevcut haliyle]
```

Davranış değişikliği İLGİLİ dosyada yapılır; buraya davranış cümlesi YAZILMAZ. Değişiklik sonrası: `cargo install --path .`
