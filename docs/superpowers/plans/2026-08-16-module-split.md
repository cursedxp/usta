# Plan — `main.rs` Modül Bölünmesi (v0.22.0)

**Spec:** `docs/superpowers/specs/2026-08-16-module-split-design.md` (hedef yapı, sert kısıtlar, ölçüm)
**Branch:** `main-split`, taban `979bcab` (= main)

## Global Constraints

Her task bunlara uymak zorunda. İhlal = review reddi.

1. **SAF TAŞIMA.** Hiçbir fonksiyon gövdesi değişmez. Hiçbir imza yeniden düşünülmez. Hiçbir şey yeniden adlandırılmaz. Hiçbir `use` "temizlenmez", hiçbir yorum "iyileştirilmez". Kod satırı taşınır, o kadar.
2. **Test sayısı sabit: 372.** Her task sonunda `cargo test` → `372 passed; 0 failed`. Test eklenmez, silinmez, gevşetilmez, birleştirilmez. Sayı değişirse task başarısızdır.
3. **Testler hedefiyle taşınır.** Testler `use super::*` ile private öğelere erişiyor; hedef fonksiyonla aynı modüle gitmezlerse derlenmezler. Görünürlük yükseltmek (private → `pub(crate)`) bu sorunun yanlış çözümüdür — testi taşı.
4. **Paylaşılan öğeler `pub(crate)` olur ve çağıran güncellenir.** `tui/run.rs` (27 öğe) ve `visual.rs` (1 öğe) `crate::<isim>` ile doğrudan çağırıyor. Taşınan öğe için hem yeni modülde görünürlük, hem çağıran taraftaki yol güncellenir. Tam liste spec'te.
5. **`main.rs`'te kalanlar:** `mod` bildirimleri, `use` bloğu, `MAX_FEEDBACK_BATCH`, `main()`. Başka hiçbir şey.
6. **`clippy` yeni uyarı üretmez.** Mevcut `run_plain_loop` too_many_arguments uyarısı Task 8'de `plain.rs`'e taşınır — **çözülmez**, taşınır. Argüman sayısını azaltmak bu planın kapsamı dışında.
7. **Her task tek commit, tek modül.** Task'lar sırayla koşar; sonraki task öncekinin bıraktığı yapının üstüne kurar.
8. **`cargo build` uyarısız.** Kullanılmayan `use` bırakma — taşıma sonucu ölü kalan import'ları temizle (bu kendi çöpün, CLAUDE.md §3'e uygun).

## Doğrulama (her task için aynı)

```
cargo test              → 372 passed; 0 failed
cargo clippy --all-targets → yalnız mevcut run_plain_loop uyarısı
```

`git diff --stat`: silinen ve eklenen satır sayısı kabaca eşleşmeli. Büyük fark = gövde değişmiş demektir, taşıma değil.

---

## Task 1 — Test modüllerini ayrı dosyaya al (`welcome.rs`, `progress.rs`)

`main.rs`'e hiç dokunmaz. Deseni küçük dosyada kanıtlar.

Rust'ta test modülü aynı dosyada yaşamak zorunda değil — `#[path]` ile ayrı dosyaya alınır, alt modül olmaya devam eder (private erişim korunur), `#[cfg(test)]` sayesinde release'de hâlâ derlenmez.

### Yapılacaklar

`src/tui/welcome.rs` sonundaki `#[cfg(test)] mod tests { ... }` bloğunun **gövdesini** `src/tui/welcome_tests.rs`'e taşı. Yerine:

```rust
#[cfg(test)]
#[path = "welcome_tests.rs"]
mod tests;
```

Aynısını `src/progress.rs` → `src/progress_tests.rs` için yap.

Taşınan dosyanın başına, neden ayrı dosyada olduğunu tek cümleyle açıklayan bir `//!` yorumu koy (dosya boyutu; `#[path]` ile hâlâ alt modül, private erişim korunuyor).

`use super::*;` satırı taşınan dosyada kalır — kapsam değişmiyor.

### Beklenen sonuç

`welcome.rs` ~1508 → ~720. `progress.rs` ~767 → ~373.

---

## Task 2 — `src/topic.rs`

En çok bağımlısı olan küme; önce o taşınır ki sonraki task'lar sabit bir hedefe `use` yazsın.

### Taşınacak öğeler (`main.rs` → `topic.rs`)

`SLUG_SYSTEM` (const) · `slug_system` · `finalize_slug` · `start_suggest_system` · `parse_start_suggestion` · `new_topic_confirm_msg` · `TopicChoice` (enum) · `interpret_topic_input` · `deasciify` · `slugify_topic`

`derive_slug` **taşınmaz** — `ask_usta`'ya bağlı, plain-path'e özel, Task 8'de `plain.rs`'e gider.

### Taşınacak testler

`slugify_*` (6 test) · `finalize_slug_*` (2) · `slug_system_*` (2) · `start_suggest_system_defines_konu_contract` (1) · `parse_start_suggestion_*` (3) · `interpret_*` + `empty_enter_*` (8) · `new_topic_confirm_msg_names_slug_and_keys` (1)

### Çağıran güncellemeleri

`tui/run.rs`: `slugify_topic`, `interpret_topic_input`, `TopicChoice` (+ `Suggest`/`Resume`/`New` varyantları), `start_suggest_system`, `parse_start_suggestion`, `slug_system`, `finalize_slug`, `new_topic_confirm_msg`
`visual.rs:102`: `crate::slugify_topic`
`main.rs`: `parse_command` ve `resolve_topic` `slugify_topic`'i çağırıyor

Hepsi `crate::topic::<isim>` olur. Görünürlük: `pub(crate)` (yalnız `slugify_topic` bugün `pub`, aynen `pub` kalsın).

---

## Task 3 — `src/cli.rs`

### Taşınacak öğeler

`ResetTarget` (enum) · `Command` (enum) · `parse_command`

`parse_command`, `slugify_topic`'i çağırıyor → `use crate::topic::slugify_topic` (Task 2 tamamlandığı için hedef sabit).

### Taşınacak testler

`parse_bare_is_start_without_topic` · `parse_start_keeps_topic_arg` · `parse_start_without_arg_is_start_none` · `parse_init_and_topics` · `parse_stats` · `parse_unknown_command_errors` · `parse_reset_topic_is_slugified` · `parse_reset_without_arg_errors` · `parse_reset_factory_flag` · `parse_reset_profile_flag_both_spellings`

### Çağıran güncellemeleri

`main()` içinde `parse_command`, `Command::*`, `ResetTarget::*` → `crate::cli::`. Bu üç öğe bugün `pub`; aynen kalsın.

---

## Task 4 — `src/slash.rs`

Slash komut ayrıştırma + game/watch/exam durum yönetimi. Hepsi saf yardımcı, oturum döngüsünden bağımsız.

### Taşınacak öğeler

`WatchCmd` (enum) · `parse_watch_command` · `apply_watch` · `GameCmd` (enum) · `parse_game_command` · `game_pref` · `set_game_pref` · `read_game_pref` · `restore_game_pref` · `game_streak_line` · `game_on_turn` · `is_exam_command` · `topic_has_goal`

### Taşınacak testler

`parse_watch_command_variants` · `apply_watch_transitions` · `parse_game_command_variants` · `game_on_turn_embeds_rules_or_falls_back_when_empty` · `game_pref_roundtrip_idempotent_preserves_user_md` · `restore_game_pref_readds_dropped_line_and_keeps_other_content` · `restore_game_pref_reverts_flipped_value` · `restore_game_pref_none_before_is_untouched` · `game_streak_line_never_shows_streak_zero` · `is_exam_command_exact_only` · `topic_has_goal_override_priority`

### Çağıran güncellemeleri

`tui/run.rs`: `parse_watch_command`, `apply_watch`, `parse_game_command`, `GameCmd` (+varyantlar), `game_pref`, `set_game_pref`, `game_streak_line`, `game_on_turn`, `is_exam_command`, `topic_has_goal`
`main.rs`: `run_plain_loop` aynı kümeyi kullanıyor
`restore_game_pref` `flush_core`'dan çağrılıyor — Task 5'te `lifecycle.rs`'e taşınacak, o zaman `crate::slash::restore_game_pref` olur

---

## Task 5 — `src/lifecycle.rs`

Oturum yaşam döngüsü: kurulum, kapanış flush'ı, kompaksiyon, zaman/kilit yardımcıları.

**İsim notu:** `src/session.rs` zaten var (`Session` struct'ı). Bu modül `lifecycle.rs`, `session.rs` değil.

### Taşınacak öğeler

`COMPACT_THRESHOLD` · `COMPACT_KEEP_LAST` · `compact_note` · `ask_usta` · `build_session` · `flush_target` · `flush_core` · `flush_progress` · `maybe_compact` · `today` · `now_stamp` · `lock_path` · `sleep_until_deadline`

### Taşınacak testler

`flush_target_maps_profile_to_global_other_three_to_project` · `flush_target_routes_mentor_files_to_project_root` · `flush_target_rejects_unknown_name`

### Çağıran güncellemeleri

`tui/run.rs`: `build_session`, `lock_path`, `maybe_compact`, `sleep_until_deadline`
`main.rs`: `main()` → `build_session`, `lock_path`, `flush_core`, `flush_progress`, `today`; `run_plain_loop` → `ask_usta`, `maybe_compact`, `sleep_until_deadline`, `today`
`flush_core` içindeki `restore_game_pref` çağrısı → `crate::slash::restore_game_pref`
Task 6 sonrası `handle_file_change` ve `run_visual_generation` `ask_usta`'yı `crate::lifecycle::ask_usta` olarak çağıracak

`today` ve `now_stamp` geniş kullanımda — `pub(crate)` olmalı.

---

## Task 6 — `src/file_feedback.rs`

Dosya-izleyici geribildirim hattı.

**İsim notu:** `src/feedback.rs` ve `src/watcher.rs` zaten var. Bu modül `file_feedback.rs`.

### Taşınacak öğeler

`FileFeedback` (enum) · `is_exercise_path` · `is_silent_skip` · `feedback_frame` · `seed_mentor_baseline` · `handle_file_change`

`handle_file_change`, `ask_usta`'yı çağırıyor → `use crate::lifecycle::ask_usta`.

### Taşınacak testler

`is_exercise_path_detects_exercises_dir` · `is_silent_skip_true_for_wrapped_not_found` · `is_silent_skip_true_for_wrapped_invalid_data` · `is_silent_skip_false_for_wrapped_permission_denied` · `binary_file_read_error_classifies_as_silent_skip` · `feedback_frame_regular_paths_keep_existing_wording` · `feedback_frame_exercise_paths_review_as_exercise`

### Çağıran güncellemeleri

`tui/run.rs`: `handle_file_change`, `FileFeedback` (+varyantlar), `is_silent_skip`, `seed_mentor_baseline`
`main.rs`: `run_plain_loop` aynı kümeyi kullanıyor

---

## Task 7 — `src/setup.rs`

Scaffold + init + reset + rapor + onay yardımcıları. Üçü de yalnız CLI alt-komutlarından çağrılıyor, hiçbiri oturum döngüsüne girmiyor, `confirm` üçünde ortak — bu yüzden tek modül. En büyük task; ~600 satır bekleniyor.

### Taşınacak öğeler

**Scaffold/init:** `run_migration` · `ensure_scaffold` · `run_init` · `print_scaffold_status` · `migrate_profile_to_user_md` · `write_global_defaults` · `write_project_scaffold`
**Rapor:** `run_topics` · `render_topics_table` · `col_pad` · `run_stats` · `render_stats`
**Reset:** `run_reset_topic` · `remove_topic_visuals` · `FACTORY_RESET_PROMPT` · `factory_targets` · `run_reset_factory` · `profile_is_generic` · `reset_profile_files` · `run_reset_profile`
**Onay:** `confirm` · `recover_choice` · `confirm_recover`

`run_stats` `today`'i çağırıyor → `crate::lifecycle::today`.

### Taşınacak testler

`factory_targets_includes_uncatalogued_cwd_project` · `recover_choice_defaults_yes_only_explicit_no_deletes` · `write_project_scaffold_*` (5) · `reset_topic_leaves_mentor_dir_untouched` · `render_stats_*` (3) · `render_topics_table_aligns_columns_with_header_rule` · `profile_is_generic_matches_embedded_template_only` · `reset_profile_files_*` (2) · `migrate_moves_old_profile_once` · `migrate_never_overwrites_existing_user_md` · `write_global_defaults_syncs_code_owned_preserves_user_owned` · `migration_before_scaffold_preserves_legacy_approaches_bak` · `remove_topic_visuals_*` (2) · `factory_reset_prompt_advertises_only_english_word`

### Çağıran güncellemeleri

`tui/run.rs`: `profile_is_generic`
`main()`: `parse_command` sonrası tüm alt-komut dispatch'i (`run_init`, `run_topics`, `run_stats`, `run_reset_*`), `ensure_scaffold`, `run_migration`, `confirm`, `confirm_recover`

---

## Task 8 — `src/plain.rs` + `show_request`/`last_assistant_text` → `visual.rs`

### 8a — `visual.rs`'e iki taşıma

`show_request` ve `last_assistant_text` **`plain.rs`'e giremez**: ikisi de `tui/run.rs` tarafından da çağrılıyor. İkisi de yalnız görsel üretimini besliyor → mevcut `src/visual.rs`'e taşınır.

Testler: `show_request_composition` → `visual.rs`'in test modülüne.

Çağıranlar: `tui/run.rs` (`show_request`, `last_assistant_text`), `main.rs`'teki `trigger_auto_visual` → `crate::visual::`.

**Tuzak:** `tui/run.rs`'in kendi `run_visual_generation` ve `trigger_auto_visual` fonksiyonları var; `main.rs`'tekileri çağırmıyorlar. Aynı isim, farklı fonksiyon — karıştırma, `tui/run.rs`'teki ikisine dokunma.

### 8b — `src/plain.rs`

Geriye kalan plain/pipe yolu:

`resolve_topic` · `derive_slug` · `run_plain_loop` · `print_reply` · `run_visual_generation` · `trigger_auto_visual`

Test yok (bu altı fonksiyonun hiçbirinin doğrudan testi yok).

`run_plain_loop`'un mevcut `too_many_arguments` clippy uyarısı **taşınır, çözülmez** — argüman sayısını azaltmak bu planın kapsamı dışında. Uyarı `plain.rs`'te görünür hâle gelir.

### Beklenen sonuç

`main.rs`: `mod` bildirimleri + `use` + `MAX_FEEDBACK_BATCH` + `main()` ≈ **250 satır**.

---

## Task 9 — Kural + sürüm + son doğrulama

Bölme tek başına yetmez; kural yazılmazsa aynı birikim tekrarlar.

### Yapılacaklar

1. **`SPEC.md`** — modül boyut bütçesi maddesi ekle: üretim kodu (test modülü hariç) 600 satırı aşan modül için bölünme gerekçesi yazılır; test modülü şişirdiğinde `#[cfg(test)] #[path = "..."]` ile ayrı dosyaya alınır. Çevresindeki madde stiline uy: kısa, kuralı söyle, sürümü an.

2. **`CLAUDE.md`** — "3. Surgical Changes" bölümüne bir madde: yeni kod eklerken hedef dosyanın mevcut boyutuna bak; bütçeyi aşıyorsa önce böl, sonra ekle. Dosyanın mevcut ton ve biçimine uy (kısa emir cümleleri, madde işaretleri).

3. **`docs/ROADMAP.md`** — en üste tarihli tek satır (2026-08-16), mevcut format.

4. **Sürüm:** `Cargo.toml` `0.21.0` → `0.22.0`, `src/tui/welcome.rs`'teki `version_aligned_with_spec` testinin dizgesini de güncelle.

5. **`setup.rs` test ayrımı:** Task 7 sonunda `setup.rs` 998 satır — ~527 üretim + ~470 test. Üretim kısmı bütçe içinde, dosyayı testler şişiriyor. Task 1'in desenini uygula: `#[cfg(test)] mod tests` gövdesini `src/setup_tests.rs`'e taşı, yerine `#[cfg(test)] #[path = "setup_tests.rs"] mod tests;` koy. Aynı kontrolü diğer yeni modüllere de yap — üretim kodu 600'ü aşan başka modül varsa aynı işlem, aşmayanlara dokunma.

6. **Biçim normalizasyonu:** Task 1'de taşınan test dosyaları (`welcome_tests.rs`, `progress_tests.rs`) eski `mod tests { }` yuvalamasından kalma 4-boşluk taban girintisini koruyor — saf-taşıma kısıtı gereği bilinçliydi, ama artık üst düzey dosyalar ve rustfmt-clean değiller. Tüm yeni/taşınan dosyalarda `cargo fmt` çalıştır. Bu task'ta bilinçli olarak yapılır: bir önceki task'ta yapılsaydı taşımanın saflığı kanıtlanamazdı. `cargo fmt` sonrası test sayısı yine 372 olmalı.

7. **Son ölçüm ve raporlama:** `wc -l src/*.rs src/tui/*.rs` çıktısını al, spec'teki "Ölçüm" tablosunun yanına ulaşılan hâli yaz. 600 satırı aşan modül kaldıysa gerekçesini yaz.

### Doğrulama

`cargo test` → 372, `cargo clippy --all-targets` → yalnız `plain.rs`'teki taşınmış uyarı. Elle doğrulama ATLA — Anil koşturacak.

---

## Notlar

- Bu plan davranış değiştirmiyor; bu yüzden yeni test yok ve olmamalı. Test sayısının 372'de sabit kalması, taşımanın saf olduğunun kanıtı.
- Task sırası bağımlılık sırası: `topic` → `cli`, `lifecycle` → `file_feedback`/`setup`, hepsi → `plain`. Sırayı değiştirme.
- Bir task'ta beklenmedik bir bağımlılık çıkarsa (haritada görünmeyen bir çağrı), o task'ı bölme — bağımlılığı raporla, controller karar versin.
