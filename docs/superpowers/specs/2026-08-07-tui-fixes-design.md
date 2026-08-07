# Spec — TUI Review Düzeltmeleri (slug oturumu, Ctrl-C iptal, genişlik, stream guard)

**Tarih:** 2026-08-07
**Durum:** Onaylandı (Anil — review bulguları üzerine "sen oluştur" talimatı)
**Kaynak:** TUI implementasyonu code review'u (commit aralığı `c0b9d19..aaf99d8`). 4 bulgu.

## 1. Amaç

TUI v1 review'unda bulunan 1 gerçek bug + 1 UX riski + 2 minör sorunu kapat. Davranış ekleme yok — mevcut davranışın doğrulanması/sağlamlaştırılması.

## 2. Bulgular ve Çözümler

### B1 — Slug oturumu kirliliği (bug, öncelik 1)

**Sorun:** Cümleden slug çıkarma LLM çağrısı (`run.rs` slug dalı + `main.rs derive_slug`) CLI backend'te `claude -p` oturum id'si yakalar. Sonraki açılış drilli/tanışma çağrısı o oturumu `--resume` eder → öğrenme oturumunun sunucu-taraflı geçmişinde SLUG_SYSTEM + ham cümle + çıplak slug cevabı kalır. İlk Usta yanıtını kirletebilir.

**Çözüm:** Slug LLM çağrısından hemen sonra `backend.reset_session()` — HER İKİ yolda (TUI `run.rs` slug dalı, plain `derive_slug`). Başarı VE hata dalında (hata dalı da id yakalamış olabilir — `parse_cli_output` kısmi başarıda id döndürebilir; koşulsuz reset en güvenlisi). API backend'te no-op, zararsız.

**Doğrulama:** `Backend::Cli` variant alanları crate-içi erişilebilir — unit test: `session_id: Some(..)` ile kur, `reset_session()` sonrası `None`. Çağrı-yeri doğruluğu: `derive_slug` testlenebilir hale gelir mi bak; gelmiyorsa kod incelemesi + elle doğrulama (aşağıda §5).

### B2 — Kilitli modda Ctrl-C yutulması (UX riski, öncelik 2)

**Sorun:** `ask_live` sırasında (`editor_key_locked`) Ctrl-C/D tamamen çöpe gidiyor; raw mode'da SIGINT de yok. Uzun/askıda `claude -p` çağrısında kullanıcının kaçış yolu yok.

**Çözüm — çift Ctrl-C iptal:**
- İlk Ctrl-C: durum satırında ipucu — `Status::Thinking`'e `cancel_hint: bool` alanı; render'da "… (iptal: tekrar Ctrl-C)" eki.
- İkinci Ctrl-C (ilkinden sonra herhangi bir zamanda, sayaç sıfırlanmaz): LLM future'ı düşürülür, `ask_live` iptal döner.
- `ask_live` dönüş tipi değişir: `Result<Reply>` → `Result<AskOutcome>` where `enum AskOutcome { Reply(backend::Reply), Cancelled }`.
- Çağrı yerleri:
  - slug dalı: `Cancelled` → yerel `slugify_topic(raw)` fallback (kullanıcı beklemek istemedi, konu yine de kurulur).
  - açılış drilli/tanışma: `Cancelled` → `page_notice("açılış turu iptal edildi")`, akış devam.
  - ana döngü Submit: `Cancelled` → `page_notice("yanıt iptal edildi")`. Kullanıcı turn'ü session history'de KALIR (recorder'a da yazıldı — transcript dayanıklılığı bilinçli; sonraki yanıt iki user turn'ü birden görür, API bunu kabul eder). Backend `reset_session()` çağrılır — yarım kalan CLI oturumu resume edilmez, sonraki çağrı tam transcript'le taze başlar.
- **Alt süreç sızıntısı (zorunlu parça):** `backend.rs run_claude_cli` future'ı düşürüldüğünde `claude` alt süreci yaşamaya devam eder (kill_on_drop yok). `cmd.kill_on_drop(true)` eklenir — iptal çocuğu da öldürür. API yolunda reqwest future drop zaten isteği keser.

### B3 — Genişlik bir kez ölçülüyor (minör)

**Sorun:** `run.rs:197` genişliği başta ölçer; resize sonrası `page_reply`/welcome eski genişliğe sarar.

**Çözüm:** `fn current_width(tui: &Tui) -> u16` yardımcısı (`tui.terminal.size()` → width; hata → 80 fallback). Sarma yapan her `page_reply`/`render_welcome`/`render_welcome_identity` çağrısı anlık genişlikle. Başta ölçülen `width` değişkeni kalkar.

### B4 — Event stream sonu sıcak döngüsü (minör)

**Sorun:** `ask_topic`/`tui_confirm`/ana döngüde `events.next()` `None` dönerse (stream sonu) döngü draw+poll ile boşta döner.

**Çözüm:** `None` = çıkış sinyali: `ask_topic` → `Ok(None)`, `tui_confirm` → `Ok(false)`, ana döngü → `break` (Eof gibi), `ask_live` → ilgili select kolu zaten `Some(Ok(..))` desenli — None gelirse kol devre dışı kalıp future bekler, sorun yok (dokunma).

## 3. Kapsam Dışı

- Ctrl-C'nin plain yoldaki davranışı (rustyline/SIGINT — mevcut, değişmiyor).
- İptal edilen user turn'ün history'den geri alınması (bilinçli tutuluyor, §B2).
- Resize'da açılış kutusunun yeniden çizimi (scrollback'te sabit — v1 kararı geçerli).

## 4. Test Stratejisi

- B1: `backend.rs` unit test (variant kurulum + reset). 
- B2: `AskOutcome` ve iptal sayacı mantığı saf yardımcıya çıkarılabilirse unit test (`LockedKey::{Edit, CancelRequest}` çevirisi); `ask_live` döngüsünün kendisi terminal gerektirir → elle doğrulama.
- B3: `current_width` fallback'i trivial; elle doğrulama (resize).
- B4: kod incelemesi (EventStream mock'lanmaz).
- Regresyon: mevcut 130 test yeşil kalmalı; plain yol davranışı değişmez.

## 5. Elle Doğrulama Listesi (plan sonunda)

1. Cümleyle konu gir → slug üret → ilk Usta yanıtında slug bağlamı sızıntısı yok; `claude` süreç listesinde artık öksüz süreç yok.
2. Yanıt beklerken Ctrl-C → durum satırında ipucu; ikinci Ctrl-C → "yanıt iptal edildi", girdi kutusu tekrar aktif, sonraki mesaj normal yanıt alıyor.
3. Slug beklerken çift Ctrl-C → yerel slug ile devam.
4. Terminal genişliğini değiştir → sonraki yanıt yeni genişliğe sarıyor.
5. `NO_COLOR=1` + pipe → plain yol birebir eski davranış.

## 6. Başarı Ölçütü

Tüm elle doğrulama maddeleri geçer; `cargo test` yeşil; `usta` binary'si `cargo install --path .` ile güncellenir.
