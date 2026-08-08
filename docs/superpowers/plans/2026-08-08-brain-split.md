# Brain Bölünmesi — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.
>
> **ÖN KOŞUL — ÖNCE DOĞRULA:** `docs/superpowers/plans/2026-08-08-user-echo-profile-reset.md` planı main'e MERGE edilmiş olmalı (`git log`'da "profil yaşam döngüsü" / "reset --profile" commit'leri görünmeli; `profile_is_generic` ve kapanışta `profile` dosyası kodda mevcut olmalı). Değilse DUR, kullanıcıya söyle. Spec: `docs/superpowers/specs/2026-08-08-brain-split-design.md` — OKU.

**Goal:** USTA.md'yi müdahale-haritalı 5 dosyaya böl (SOUL/RULES/TEACHING/GOAL + indeks-USTA.md), `learner/profile.md`'yi kök `USER.md` yap (migrasyonlu), GOAL'ü koşullu yükle. Davranış içeriği BİREBİR korunur.

**Architecture:** Repo kökündeki markdown'lar bölünür; `defaults.rs` listesi genişler; `brain.rs` yükleme sırası yeni sete geçer (+`## Hedef` koşulu); `ensure_scaffold`'a migrasyon adımı (sync'ten ÖNCE). Kod mantığı minimal — asıl iş dikkatli metin taşıma.

**Tech Stack:** Mevcut. Yeni bağımlılık YOK.

## Global Constraints

- **İçerik sadakati kutsal:** bölme sırasında cümle EKLENMEZ/ÇIKARILMAZ/DEĞİŞTİRİLMEZ. İki istisna: (1) USTA.md'nin yeni indeks gövdesi, (2) `learner/profile.md` yol atıflarının `USER.md`'ye güncellenmesi. Şüphede: aynen taşı.
- Türkçe yorum/UI; `cargo test --quiet`; commit stili aynı.
- Sıra: migrasyon → sync (spec §5). `USER.md` User-owned; diğer beş dosya Code-owned.
- Mevcut tüm testler yeşil kalır (yol/parametre güncellemeleri hariç davranış değişikliği yok).

## Dosya Haritası

| Dosya | Değişiklik |
|---|---|
| `USTA.md` | Gövde → indeks (harita tablosu + yükleme sırası) |
| `SOUL.md`, `RULES.md`, `TEACHING.md`, `GOAL.md` | YENİ — içerik USTA.md'den birebir |
| `learner/profile.md` → `USER.md` | Taşıma (repo + gömülü default + migrasyon) |
| `src/defaults.rs` | Liste: 5 Code + USER.md (User) |
| `src/brain.rs` | Yükleme sırası + koşullu GOAL |
| `src/main.rs` | Migrasyon (`ensure_scaffold`), `reset --profile`/`profile_is_generic`/flush `profile` hedefi → USER.md |
| `src/tui/run.rs`, `src/tui/welcome.rs` | Profil okuma yolu → USER.md |
| `README.md`, `SPEC.md` | Yapı bölümü güncellenir |

---

### Task 1: Markdown bölünmesi + defaults listesi

**Files:**
- Create: `SOUL.md`, `RULES.md`, `TEACHING.md`, `GOAL.md`
- Modify: `USTA.md` (indeksleşir), `src/defaults.rs`
- Rename: `learner/profile.md` → `USER.md` (git mv; içerik aynı)

**Interfaces:**
- Produces: `defaults::global_defaults()` yeni liste — `("USTA.md", Code)`, `("SOUL.md", Code)`, `("RULES.md", Code)`, `("TEACHING.md", Code)`, `("GOAL.md", Code)`, `("USER.md", User)`, `approaches/*` (Code, aynen). `learner/profile.md` girdisi KALKAR. `learner/index.md` (User) aynen.

- [ ] **Step 1: Bölümleri taşı** — mevcut `USTA.md`'yi oku; spec §3 tablosuna göre BİREBİR kes-yapıştır:
  - `SOUL.md`: baştaki kimlik paragrafı ("Sen **Usta**'sın…") + "Kullanıcıyla **Türkçe**…" + `## Persona` + `## Anlatım Dili — seviyeye kalibre et`
  - `RULES.md`: `## Sert Kurallar (ihlal edilemez)` (6 madde) + `## Canlı Belgeler`
  - `TEACHING.md`: `## Çalışma Kadansı`, `## Açılış Drilli`, `## Anlat-Modu (Feynman)`, `## İpucu Merdiveni`, `## Tahmin Protokolü`, `## Yeni Konu Tanışması`, `## Kapsam Bekçiliği`, `## Meta-beceri`, `## Domaine göre yaklaşım`
  - `GOAL.md`: `## Hedefli Öğrenme` (tamamı)
  - Her yeni dosyanın başına tek satır H1 + tek cümle rol tanımı eklenebilir (İSTİSNA değil — başlık, davranış cümlesi değil).

- [ ] **Step 2: İçerik sadakati doğrulaması** — script ile: eski USTA.md'deki (git'ten: `git show HEAD:USTA.md`) her `##` başlığın yeni dosyalarda TAM BİR kez geçtiğini ve bölüm gövdelerinin (boşluk normalize edilmiş) birebir eşleştiğini kontrol et:

```bash
python3 - <<'EOF'
import subprocess, re
old = subprocess.check_output(["git","show","HEAD:USTA.md"], text=True)
new = "".join(open(f).read() for f in ["SOUL.md","RULES.md","TEACHING.md","GOAL.md"])
norm = lambda s: re.sub(r"\s+"," ", s).strip()
eksik = [b for b in re.split(r"\n(?=## )", old)[1:] if norm(b) not in norm(new)]
print("EKSİK BÖLÜM:", len(eksik))
for b in eksik: print("-", b.splitlines()[0])
assert not eksik, "içerik kaybı var — taşımayı düzelt"
print("SADAKAT OK")
EOF
```

(Yol atıfı güncellemeleri — `learner/profile.md` → `USER.md` — bu kontrolü kırarsa: önce birebir taşı+doğrula, atıf güncellemesini SONRAKİ ayrı adımda yap ve hangi satırların değiştiğini commit mesajında listele.)

- [ ] **Step 3: USTA.md'yi indeksleştir** — yeni gövde: kısa açıklama + spec §3'teki müdahale haritası tablosu + yükleme sırası + "Davranış değişikliği İLGİLİ dosyada yapılır; buraya davranış cümlesi YAZILMAZ. Değişiklik sonrası: `cargo install --path .`" notu.

- [ ] **Step 4: `defaults.rs` güncelle + failing test düzelt** — liste yeni sete geçer (`include_str!` yolları); testler: dosya sayısı, `USER.md` = User + `learner/` dışında da User olabildiği için sahiplik testi (`core_behavior_is_code_owned_learner_is_user_owned`) yeni kurala göre yeniden yazılır: Code = {USTA, SOUL, RULES, TEACHING, GOAL, approaches/*}; User = {USER.md, learner/index.md}. `shipped_profile_carries_no_personal_name` → `USER.md` içeriğine bakar.

- [ ] **Step 5: Derleme + testler + commit**

```bash
cargo test --quiet 2>&1 | tail -3   # brain.rs henüz eski yolları arıyorsa Task 2'ye kadar kırmızı kalabilir — kırmızıysa Task 2 ile birlikte commit'le
git add USTA.md SOUL.md RULES.md TEACHING.md GOAL.md USER.md src/defaults.rs
git rm learner/profile.md
git commit -m "brain: USTA.md müdahale-haritalı 5 dosyaya bölündü — içerik birebir, USTA.md indeks"
```

---

### Task 2: `brain.rs` — yeni yükleme sırası + koşullu GOAL

**Files:**
- Modify: `src/brain.rs`

**Interfaces:**
- Produces: `load_system_prompt` yeni sıra: `SOUL.md → RULES.md → TEACHING.md → [GOAL.md koşullu] → approaches/(software|_default) → approaches/<konu> → USER.md → progress → curriculum` (BUGÜN bölümü mevcut konumunda). `USTA.md` YÜKLENMEZ.
- Koşul: konu approach içeriği (zaten okunuyor) `"## Hedef"` içeriyorsa GOAL.md eklenir.

- [ ] **Step 1: Failing testler** — brain.rs test modülünde mevcut kurulum desenini (temp global + dosya yazımı) kullan:

```rust
    #[test]
    fn system_prompt_loads_split_files_not_index() {
        // kurulum: SOUL/RULES/TEACHING/GOAL/USTA.md ayrı içeriklerle yazılır
        // (mevcut testlerdeki fs::write deseni)
        // assert: sys "SOUL-İÇERİK","RULES-İÇERİK","TEACHING-İÇERİK" içerir;
        //         "İNDEKS-İÇERİK" (USTA.md gövdesi) İÇERMEZ.
    }

    #[test]
    fn goal_loaded_only_when_approach_has_hedef_section() {
        // approach dosyası "## Hedef" İÇERMEZKEN: sys "GOAL-İÇERİK" içermez.
        // approach "## Hedef\n2026-12-01" içerirken: içerir.
    }

    #[test]
    fn user_md_replaces_profile_in_prompt() {
        // global USER.md yazılır → sys içeriğini içerir; learner/profile.md yazılsa bile OKUNMAZ.
    }
```

(Gövdeleri mevcut brain.rs test desenine göre doldur — `read_section` çağrı başlıkları da `"SOUL.md"` vb. olur; eski `"USTA.md"` başlık assert'leri güncellenir.)

- [ ] **Step 2: FAIL → implementasyon** — `read_section` çağrı zinciri yeni sıraya geçer; koşul:

```rust
    // GOAL yalnız hedefli konuda yüklenir — hedefsiz oturumda ~1.5KB tasarruf,
    // model alakasız sınav-tempo kurallarını taşımaz (spec §3 koşullu satır).
    let approach_konu = std::fs::read_to_string(/* mevcut konu-approach yolu */).unwrap_or_default();
    if approach_konu.contains("## Hedef") {
        read_section(&global.join("GOAL.md"), "GOAL.md", &mut parts);
    }
```

(Gerçek yerleşimi mevcut fonksiyon akışına uydur — approach zaten okunuyorsa İKİNCİ kez okuma.)

- [ ] **Step 3: Tüm testler + commit**

```bash
cargo test --quiet 2>&1 | tail -3
git add src/brain.rs
git commit -m "brain: bölünmüş dosya yükleme sırası + GOAL koşullu — indeks modele gitmez"
```

---

### Task 3: USER.md geçişi — migrasyon + tüm referanslar

**Files:**
- Modify: `src/main.rs` (`ensure_scaffold` migrasyonu; `reset --profile`/`reset_profile_files`/`profile_is_generic`/flush `profile` hedefi), `src/tui/run.rs`, `src/tui/welcome.rs` (varsa yol atıfı), `src/progress.rs` (kapanış prompt metninde yol geçiyorsa)

**Interfaces:**
- Produces: `fn migrate_profile_to_user_md(global: &Path) -> Result<bool>` — `learner/profile.md` var + `USER.md` yok → rename, `true`; aksi no-op `false`. `ensure_scaffold` içinde `write_global_defaults`'tan ÖNCE çağrılır (spec §5 sıra şartı).
- Tüm profil yolu tüketicileri `global.join("USER.md")`'ye döner.

- [ ] **Step 1: Failing testler**

```rust
    #[test]
    fn migrate_moves_old_profile_once() {
        // temp kök: learner/profile.md("KIŞISEL") var, USER.md yok
        // → migrate true, USER.md=="KIŞISEL", eski yol yok.
        // İkinci çağrı → false, dosyalar aynı.
    }

    #[test]
    fn migrate_never_overwrites_existing_user_md() {
        // Hem eski profil hem USER.md("YENİ") varsa: USER.md dokunulmaz,
        // eski dosya yerinde bırakılır (veri kaybı riski alınmaz), false döner.
    }
```

Ayrıca mevcut `reset_profile_files_*` ve `profile_is_generic_*` testleri `USER.md` yoluna güncellenir (davranış aynı, yol değişti).

- [ ] **Step 2: FAIL → implementasyon** — migrasyon + tüm `learner/profile.md` string'lerinin `USER.md`'ye dönmesi:

```bash
grep -rn "profile.md" src/ | grep -v test   # sıfır kalmalı (yorumlar dahil gözden geçir)
```

`reset --profile` onay metni ve README'deki yollar `USER.md`'yi gösterir; komut adı DEĞİŞMEZ.

- [ ] **Step 3: Tüm testler + commit**

```bash
cargo test --quiet 2>&1 | tail -3
git add -A
git commit -m "brain: learner/profile.md → USER.md — migrasyonlu, tüm referanslar güncel"
```

---

### Task 4: README/SPEC + elle doğrulama + kurulum

**Files:**
- Modify: `README.md`, `SPEC.md` (dosya yapısı bölümleri: yeni brain seti + müdahale haritasına işaret)

- [ ] **Step 1: Doküman güncelle + tüm testler**

```bash
cargo test --quiet 2>&1 | tail -3
```

- [ ] **Step 2: Elle doğrulama (spec §8)**

1. `cargo install --path .` → `usta start <konu>` → ilk açılışta `~/.config/usta/`'da SOUL/RULES/TEACHING/GOAL/USER.md + indeks-USTA.md oluşmuş; ESKİ profil içeriği USER.md'de.
2. Normal oturum: davranış regresyonu yok — kod yazmıyor, drill soruyor, isimle hitap ediyor.
3. Hedefsiz konu vs hedefli konu (approach'ta `## Hedef`): tempo satırı yalnız ikincide; bağlam göstergesi hedefsizde bir tık düşük.
4. `usta reset --profile` → USER.md jenerik + `.bak`.
5. USTA.md'yi aç → haritadan SOUL.md'yi bul → küçük ton değişikliği → rebuild → davranışta görünüyor.

- [ ] **Step 3: Kurulum + commit**

```bash
cargo install --path .
git add -A
git commit -m "brain: bölünme v1 tamam — elle doğrulama geçti, dokümanlar güncel"
```

---

## Self-Review Notları

- **Spec kapsaması:** §3 bölme→Task 1, yükleme+koşul→Task 2, §4 USER.md→Task 3, §5 sıra (migrate→sync)→Task 3 ensure_scaffold yerleşimi, §7 sadakat→Task 1 Step 2 script'i, §8→Task 4. Boşluk yok.
- **En büyük risk içerik kaybı** — Task 1 Step 2'deki mekanik doğrulama zorunlu adım; atıf güncellemeleri ayrı adım/commit'te ki diff'te görünsün.
- **Sıra bağımlılığı:** Task 1 sonunda testler kırmızı kalabilir (brain.rs eski yolları arar) — Task 1+2 arka arkaya; commit sırası nota bağlandı.
- **Migrasyon güvenliği:** USER.md varken asla üzerine yazılmaz; eski dosya silinmez, taşınır (rename) — veri kaybı yolu yok.
