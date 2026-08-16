# Design — `main.rs` Modül Bölünmesi (v0.22.0)

**Problem:** `src/main.rs` 3045 satır (≈1980 üretim kodu + ≈1065 test). Dosya, temiz bir modüle ait olmayan her şeyin çekim merkezi olmuş: CLI dispatch, oturum kurulumu, kapanış flush'ı, slug türetme, slash-komut ayrıştırma, scaffold yazımı, reset akışları, dosya-geribildirim hattı ve plain-path döngüsü — hepsi aynı dosyada.

**Neden oldu:** hiçbir yerde modül boyut kuralı yok. SPEC'te turuncu disiplini, ADHD kuralları, token kuralları var; boyut üzerine tek satır yok. Review rubriğinde de yok — her task'ta spec uyumu ve doğruluk soruldu, "bu dosya çok mu büyüdü" hiç sorulmadı. Rubrikte olmayan uygulanmaz. Her tekil commit savunulabilirdi; boyut yalnız toplamda göründü.

**Bu iş ne DEĞİL:** yeniden tasarım değil. Hiçbir fonksiyonun gövdesi değişmiyor, hiçbir imza yeniden düşünülmüyor, hiçbir şey yeniden adlandırılmıyor. Saf taşıma.

---

## Ölçüm

| dosya | toplam | üretim | test |
|---|---|---|---|
| `src/main.rs` | 3045 | ~1980 | ~1065 |
| `src/tui/welcome.rs` | 1508 | ~720 | ~788 |
| `src/tui/run.rs` | 1051 | ~957 | ~94 |
| `src/progress.rs` | 767 | ~373 | ~394 |

İki ayrı problem var, ikisi ayrı çözülür:

1. **Test modülü şişkinliği** (`welcome.rs`, `progress.rs`) — üretim kodu makul, dosyayı testler büyütüyor. Rust'ta `#[cfg(test)] mod tests` aynı dosyada yaşar; bu idiomatik, kendi başına kir değil. Ama `#[path]` ile ayrı dosyaya alınabilir, private erişim ve sıfır release maliyeti korunarak.
2. **Gerçek modül yokluğu** (`main.rs`) — ~1980 satır üretim kodu, en az yedi ayrı sorumluluk. Asıl iş burada.

### Ulaşılan (Task 9 sonrası, `wc -l src/*.rs src/tui/*.rs`)

| dosya | satır | not |
|---|---|---|
| `src/main.rs` | 272 | `mod` bloğu + `use` bloğu + `MAX_FEEDBACK_BATCH` + `main()` — plandaki kısıt aynen |
| `src/setup.rs` | 574 | Task 7'nin ~527 üretimi + Task 9'da `#[cfg(test)]` yönlendirmesi; test gövdesi `setup_tests.rs`'e taşındı |
| `src/setup_tests.rs` | 529 | yeni — Task 1'in `#[path]` deseniyle |
| `src/tui/run.rs` | 1185 | **600 bütçesini hâlâ aşıyor** — bu planın kapsamı dışında (plan yalnız `main.rs`'i kapsıyordu); kendi başına ayrı bir bölme geçişinin adayı |
| `src/tui/welcome_tests.rs` | 1117 | test dosyası, bütçe dışı |
| `src/tui/welcome.rs` | 797 | Task 1 üretim kısmı + Task 8'de eklenen `show_request`/`last_assistant_text` |
| `src/visual.rs` | 739 | Task 2 + Task 8 taşımaları + üstbilgi güncellemesi |
| `src/progress.rs` / `src/progress_tests.rs` | 375 / 404 | Task 1 |
| `src/brain.rs` | 487 | dokunulmadı |
| `src/backend.rs` | 466 | dokunulmadı |

Toplam (tüm `src/*.rs` + `src/tui/*.rs`): 12743 satır. Artış, `cargo fmt`'in bu planın taşıdığı dosyalarda uzun satırları sarmasından geliyor — davranış değişmedi (372/372 test, tek clippy uyarısı `plain.rs`'te, `cargo build` temiz).

`src/tui/run.rs` dışında, 600 satır bütçesini aşan başka bir modül kalmadı.

## Sert kısıt: paylaşılan öğeler

`tui/run.rs` **27**, `visual.rs` **1** öğeyi doğrudan `crate::<isim>` ile çağırıyor. Bunlar taşındığında hem yeni modülde en az `pub(crate)` olmalı, hem de çağıran taraftaki `use` yolları güncellenmeli. Tam liste:

```
show_request · last_assistant_text · slugify_topic · parse_watch_command
interpret_topic_input · TopicChoice · start_suggest_system · parse_start_suggestion
slug_system · finalize_slug · new_topic_confirm_msg · lock_path · build_session
seed_mentor_baseline · profile_is_generic · game_streak_line · apply_watch
parse_game_command · GameCmd · game_pref · set_game_pref · is_exam_command
topic_has_goal · game_on_turn · maybe_compact · sleep_until_deadline
handle_file_change · FileFeedback · is_silent_skip
```

Tuzak: `tui/run.rs`'in **kendi** `run_visual_generation` ve `trigger_auto_visual` fonksiyonları var — `main.rs`'tekileri çağırmıyorlar. Aynı isim, farklı fonksiyon. `main.rs`'teki ikisi plain-path'e özel.

## Taşınamayanlar

- **`main()`** — `#[tokio::main]` giriş noktası, tüm fonksiyon boyunca thread edilen yerel mutable state'i (`backend`, `watch_rx`, yol değişkenleri, `(session, recorder, lock)` üçlüsü) sahipleniyor. Crate kökünde kalır.
- **`main.rs:185`'teki `if !ui::is_plain()` çatalı** — TUI ve plain yollarının ayrım noktası, iki tarafı da scope'ta ister.
- **`MAX_FEEDBACK_BATCH`** — hem `main()` (TUI çağrısında) hem `run_plain_loop` kullanıyor. İki yolun ortak sabiti; `main.rs`'te kalır.

## Hedef yapı

Yedi yeni modül. Her biri tek bir sorumluluk, testleri kendisiyle taşınır.

| modül | içerik | paylaşılan? |
|---|---|---|
| `src/cli.rs` | `Command`, `ResetTarget`, `parse_command` | hayır |
| `src/topic.rs` | slug + konu çözümleme: `slugify_topic`, `deasciify`, `finalize_slug`, `slug_system`, `SLUG_SYSTEM`, `interpret_topic_input`, `TopicChoice`, `start_suggest_system`, `parse_start_suggestion`, `new_topic_confirm_msg` | **evet** (8 öğe) |
| `src/slash.rs` | watch/game/exam slash komutları: `WatchCmd`, `parse_watch_command`, `apply_watch`, `GameCmd`, `parse_game_command`, `game_pref`, `set_game_pref`, `read_game_pref`, `restore_game_pref`, `game_streak_line`, `game_on_turn`, `is_exam_command`, `topic_has_goal` | **evet** (10 öğe) |
| `src/lifecycle.rs` | oturum yaşam döngüsü: `build_session`, `lock_path`, `ask_usta`, `flush_target`, `flush_core`, `flush_progress`, `maybe_compact`, `compact_note`, `COMPACT_THRESHOLD`, `COMPACT_KEEP_LAST`, `today`, `now_stamp`, `sleep_until_deadline` | **evet** (4 öğe) |
| `src/file_feedback.rs` | dosya-izleme geribildirimi: `FileFeedback`, `is_exercise_path`, `is_silent_skip`, `feedback_frame`, `seed_mentor_baseline`, `handle_file_change` | **evet** (4 öğe) |
| `src/setup.rs` | scaffold + init + reset + rapor + onay: `ensure_scaffold`, `run_init`, `run_migration`, `print_scaffold_status`, `write_global_defaults`, `write_project_scaffold`, `migrate_profile_to_user_md`, `run_topics`, `render_topics_table`, `run_stats`, `render_stats`, `col_pad`, `run_reset_topic`, `remove_topic_visuals`, `factory_targets`, `FACTORY_RESET_PROMPT`, `run_reset_factory`, `profile_is_generic`, `reset_profile_files`, `run_reset_profile`, `confirm`, `recover_choice`, `confirm_recover` | **evet** (1 öğe: `profile_is_generic`) |
| `src/plain.rs` | plain/pipe yolu: `run_plain_loop`, `resolve_topic`, `derive_slug`, `print_reply`, `run_visual_generation`, `trigger_auto_visual` | hayır |
| `src/visual.rs` (mevcut) | `show_request` ve `last_assistant_text` buraya taşınır — ikisi de yalnız görsel üretimini besliyor ve ikisi de TUI tarafından da kullanılıyor, yani plain-path modülüne giremezler | **evet** (2 öğe) |

Sonrası `main.rs`: `mod` bildirimleri + `use` bloğu + `MAX_FEEDBACK_BATCH` + `main()`. Tahmini **~250 satır**.

`setup.rs` üç alt-kümeyi (scaffold, reset, rapor) tek modülde topluyor. Gerekçe: üçü de yalnız CLI alt-komutlarından çağrılıyor, hiçbiri oturum döngüsüne girmiyor, ve `confirm` üçünde de ortak. Ayırmak üç modül daha üretir, kazanç getirmez. ~600 satır bekleniyor — kabul edilebilir üst sınır.

## İsim çakışmaları

`src/session.rs` **zaten var** (109 satır, `Session` struct'ı). Oturum yaşam döngüsü modülü bu yüzden `lifecycle.rs`. Benzer şekilde `watcher.rs`, `feedback.rs`, `progress.rs`, `history.rs`, `index.rs`, `visual.rs`, `check.rs`, `help.rs`, `input.rs`, `migrate.rs`, `materials.rs`, `transcript.rs`, `defaults.rs`, `config.rs`, `tokens.rs`, `ui.rs`, `brain.rs`, `backend.rs`, `anthropic.rs` kullanımda — yeni isimler bunlarla çakışmıyor.

## Test yerleşimi

Testler `use super::*` ile glob import yapıyor ve **truly private** öğelere dokunuyor (`deasciify`, `confirm`, `col_pad`, `flush_target`, `render_stats`, `render_topics_table`, `write_project_scaffold`, `write_global_defaults`, `reset_profile_files`, `remove_topic_visuals`, `recover_choice`, `confirm_recover`, `print_scaffold_status`, `migrate_profile_to_user_md`, `factory_targets`). Bu yüzden her test, hedefiyle **aynı modüle** taşınır — `pub(crate)` yapmaya gerek kalmaz, görünürlük bugünkü hâliyle korunur.

74 testin modül dağılımı plan dosyasında task bazında verilir.

## Bir daha olmaması için

Boyut kuralı hiçbir yerde yazılı değil; yazılmazsa aynı şey birikir. Son task iki yere kural ekler:

1. **SPEC.md** — modül boyut bütçesi: üretim kodu 600 satırı aşarsa bölünme gerekçesi yazılır.
2. **CLAUDE.md** — "Surgical Changes" bölümüne bir madde: yeni kod eklerken hedef dosyanın boyutuna bak; bütçeyi aşıyorsa önce böl.

Kural rubriğe girmezse uygulanmaz — bu yüzden ikisi de dosyaya yazılır, sözlü niyet olarak kalmaz.

## Doğrulama

Her task'ın kabul kriteri aynı ve sert:

- `cargo test` → **372 passed, 0 failed**. Sayı her task'ta birebir aynı kalır; test eklenmez, silinmez, gevşetilmez.
- `cargo clippy --all-targets` → yeni uyarı yok (mevcut `run_plain_loop` too_many_arguments hariç — o `plain.rs` taşımasıyla birlikte taşınır, çözülmez).
- `git diff --stat` taşınan satır sayısı ile eklenen satır sayısı kabaca eşleşmeli; büyük fark = gövde değişmiş demektir.
- Davranış testi yok çünkü davranış değişmiyor. Değişen tek şey dosya sınırı.

## İlgili
- Plan: `docs/superpowers/plans/2026-08-16-module-split.md`
- Harita kaynağı: bu spec'in yazımı için `main.rs`'in tam yapısal haritası çıkarıldı (80 üst-düzey öğe, çağrı grafiği, test gruplaması, taşınamaz öğeler).
