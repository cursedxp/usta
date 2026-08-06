# Usta — MVP Implementation Plan

> **For agentic workers:** implement task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Terminal'de çalışan, Opus destekli, Rust ile yazılmış Socratic öğrenim mentoru MVP'si — sohbet döngüsü + dosya izleme + web araştırma.

**Architecture:** "İnce kabuk, kalın beyin." Rust = CLI + Anthropic client + file watcher + markdown brain loader. Zekâ + kişilik = markdown dosyalarında (`USTA.md`, `learner/`, `approaches/`). Davranış değişimi = markdown düzenle, koda dokunma.

**Tech Stack:** Rust 2021 · tokio (async) · reqwest (HTTP + SSE) · serde/serde_json · notify (file watch) · rustyline (REPL) · anyhow/thiserror · dirs.

## Global Constraints

- Model: `claude-opus-4-8` (config'te sabit ama değiştirilebilir).
- Anthropic Messages API: `POST https://api.anthropic.com/v1/messages`; headers `x-api-key`, `anthropic-version: 2023-06-01`, `content-type: application/json`.
- Thinking: `{"type":"adaptive"}`; `output_config.effort: "high"`.
- Araştırma: server-side tool `{"type":"web_search_20260209","name":"web_search"}` — client döngüsü yok, sonuç aynı response'ta.
- Streaming (SSE, `"stream": true`) — timeout'tan kaçış; server-tool döngüsü `pause_turn` dönerse mesajı re-send et.
- Sert kural (prompt seviyesinde, USTA.md): kod yazma/düzeltme YOK; uydurma YOK → bilmezse web_search; parça-başı mini-spek; ADHD-aware "suya gir".
- API key: env `ANTHROPIC_API_KEY`; yoksa net hata.

---

## Dosya Yapısı

```
usta/
  Cargo.toml
  src/
    main.rs          # CLI giriş, komut ayrıştırma, REPL döngüsü
    config.rs        # model, api key, brain kökü çözümleme
    brain.rs         # markdown dosyalarını oku → system prompt birleştir
    anthropic.rs     # Messages API istek/yanıt tipleri + streaming client
    sse.rs           # SSE olay ayrıştırıcı (saf, test edilebilir)
    session.rs       # konuşma geçmişi + oturum durumu
    watcher.rs       # notify tabanlı dosya izleyici + debounce
  USTA.md            # çekirdek davranış (Socratic, dokunmaz, uydurmaz, senior)
  learner/
    index.md
    profile.md
    progress/rust.md
  approaches/
    software.md
    _default.md
```

---

## Görev Sırası

### Task 1: Cargo scaffold + bağımlılıklar
- `cargo init --name usta`, `Cargo.toml`'a bağımlılıklar.
- `cargo build` geçer (boş main).
- **Test:** `cargo build` başarılı.

### Task 2: `sse.rs` — SSE olay ayrıştırıcı (saf, TDD)
- Fn: `parse_sse_line(line: &str) -> Option<SseEvent>` ve delta biriktirici.
- Girdi: `data: {...}` satırları → `content_block_delta`/`text_delta` metnini çıkar; `message_stop`; `content_block_delta`/`thinking_delta` yoksay; `message_delta` içinden `stop_reason`.
- **Test:** örnek SSE bloğu → beklenen metin parçaları + stop_reason. Saf fonksiyon, ağ yok.

### Task 3: `anthropic.rs` — istek gövdesi tipleri (TDD serde)
- `MessageRequest { model, max_tokens, system, messages, thinking, output_config, tools, stream }`.
- `web_search` tool + adaptive thinking serialize.
- **Test:** `serde_json::to_value(req)` → beklenen anahtarlar (`"model":"claude-opus-4-8"`, `"thinking":{"type":"adaptive"}`, tool type doğru).

### Task 4: `config.rs` — key + brain kökü
- `ANTHROPIC_API_KEY` çöz (yoksa `anyhow::bail!` net mesaj). Brain kökü = binary'nin yanındaki repo kökü veya CWD.
- **Test:** env set/unset senaryosu (env guard ile).

### Task 5: `brain.rs` — markdown yükle + system prompt birleştir
- `load_system_prompt(root, topic) -> String`: `USTA.md` + `learner/profile.md` + `approaches/<domain|_default>.md` + `learner/progress/<topic>.md` (varsa) birleştir.
- **Test:** geçici dizinde sahte md dosyaları → birleşik string beklenen başlıkları içerir; eksik dosya sessiz atlanır.

### Task 6: `anthropic.rs` streaming client (entegrasyon, key-gated)
- `stream_message(req) -> impl Stream<Item=String>`: reqwest POST, SSE gövdesini `sse.rs` ile ayrıştır, metin parçalarını yay. `pause_turn` → re-send.
- **Test:** `#[ignore]` entegrasyon testi (key gerektirir); birim testler sse.rs'te.

### Task 7: `session.rs` — konuşma durumu
- `Session { history: Vec<Message>, topic, root }`; `push_user`, `push_assistant`, `build_request()`.
- **Test:** push sırası + build_request geçmişi doğru sıralar.

### Task 8: `main.rs` — REPL sohbet döngüsü
- `usta start <topic>`: brain yükle → rustyline döngüsü → her satır: session'a ekle, stream, ekrana yaz, assistant'ı kaydet.
- Slash: `/quit`.
- **Test:** elle duman testi (key ile) — plan doğrulama adımı.

### Task 9: `watcher.rs` — dosya izleme (proaktif feedback)
- `notify` ile CWD'yi izle, debounce (500ms), değişen dosyayı oku → session'a "kullanıcı şu dosyayı kaydetti: <içerik>" olarak enjekte → proaktif Socratic feedback.
- REPL ile eşzamanlı (tokio task + kanal).
- **Test:** debounce birim testi (sahte olaylar → tek tetik).

### Task 10: Brain markdown dosyaları
- `USTA.md`, `learner/*`, `approaches/*` ilk içerikleri yaz (sert kurallar + persona + Anil profili).
- **Test:** brain.rs testleri gerçek dosyalarla da geçer.

---

## Test Stratejisi

- **Birim (ağsız):** sse ayrıştırma, istek serde, brain birleştirme, debounce, session sıralama.
- **Entegrasyon (`#[ignore]`, key-gated):** gerçek Opus çağrısı — CI'da atlanır, elle çalıştırılır.
- **Duman testi:** `cargo run -- start rust` + `ANTHROPIC_API_KEY` → gerçek sohbet.

## MVP Dışı (sonraki sürümler)
Çoklu terminal sağlamlaştırma · model routing · gaps/curriculum otomasyonu · kendi-sağlık-denetimi (link check) · marketing dışı domain cilası.
