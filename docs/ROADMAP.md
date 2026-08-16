# Usta Roadmap

> Decided: 2026-08-12. The order is deliberate — every item goes through its own spec → plan → implementation cycle.
> Versioning: every completed roadmap item is a minor bump (aligned with its SPEC §); tag vX.Y.Z.

| # | Feature | Status | Summary |
|---|---|---|---|
| 1 | **Visual explainer** (`/show`) | ✅ done (2026-08-13) | Embedded HTML skeleton + model-generated scene script → animated explanation in the browser. Proactive: on a "not landing" signal Usta suggests `/show`. |
| 2 | **Exercise/artifact loop** | ✅ done (2026-08-15) | Generalizes the watcher beyond code: Usta assigns a deliverable, the user writes it under `exercises/`, the same feedback loop works in every domain. |
| 3 | **Spaced repetition** | ✅ done (2026-08-15) | Due dates on recall questions (simplified SM-2); "N reviews due today" at opening. |
| 4 | **Distribution/onboarding** | partially ✅ (2026-08-15: onboarding-lite + v0.13.0; distribution/brew deferred) | Prebuilt binaries (brew/release), no-API-key path, first-run wizard. Precondition for "anyone can use it". |
| 5 | **Material ingestion** | ✅ done (2026-08-15) | The user provides a PDF/book/course; the curriculum is built around it. |
| 6 | **Progress summary/motivation** | ✅ done (2026-08-15) | Weekly summary: map % delta, settled items, streaks. ADHD: visible progress = fuel. |
| 7 | **Mock exam generator** | ✅ done (2026-08-15) | `/exam` in goal mode: timed mock from the map, score recorded into `## Hedef Durumu`. |
| 8 | **Gamification mode** | ✅ done (2026-08-15) | Opt-in `/game on\|off` (persisted in the USER.md profile). Map states = XP, process points (showing up/predicting — not correctness), gap closures = badges. ADHD-safe: no guilt on a broken streak ("your longest was X" frame), points reward process, overjustification guarded. Merges with #7: exam = boss fight. Mostly markdown/prompt work. |
| 9 | **Prompt diet** | ✅ done (2026-08-16) | Principle: anything the shell can solve deterministically never goes into a prompt. Conditional brain loading (GAMIFICATION/MATERIAL/PREDICTION follow the GOAL.md pattern), due-question selection moves to the shell, dead instructions removed. Target v0.19.0. |
| 10 | **Spaced-rep arithmetic → shell** | pending | Audit finding: the interval ladder + date math (`due = today + ivl`, reset on miss, retirement at 90) is deterministic but currently done by the model at closing. Move it to the shell: the model only labels a per-question verdict (recalled / struggled / not drilled); the shell computes and writes the schedule tail. Needs a structured verdict channel in the closing format — medium refactor; the win is reliability (zero arithmetic drift), not tokens. |
| 11 | **Distribution (deferred half of #4)** | pending | CI release workflow (macOS arm64/x86_64 + Linux), GitHub Releases, Homebrew tap (`brew install cursedxp/usta/usta`). Deliberately parked until "anyone can use it" matters. |

## Tech debt / small items

- `run_plain_loop` has 10 args (pre-existing clippy `too_many_arguments`) — needs a real signature refactor (params struct); deliberately left, open as its own small task.
- Conditional loading for `approaches/_default.md` (only needed during new-topic onboarding) and `learner/index.md` (catalog rarely needed inside a single-topic session) — low-priority follow-up to #9.
- Further ideas (unscheduled): streaming replies, multi-terminal hardening, self-health-check (links/consistency), tech-notes cache.

## Completed

- 2026-08-16: Prompt diet — principle locked: anything the shell solves deterministically never rides in a prompt. Conditional brain loading (GAMIFICATION.md only when the profile enables the game — /game on embeds the rules into the turn; MATERIAL.md only when materials/ has files; PREDICTION.md only in Cargo projects — GOAL.md pattern); due-question selection moved to the shell (deterministic drill list, due_count single-sourced); dead Tercihler KEEP instruction removed (shell restore already guarantees it). Common-scenario system prompt ~4.1KB (~19%) smaller; TEACHING.md 11KB → 6.9KB; v0.19.0.

- 2026-08-16: Hardening round (v0.18.1–v0.18.5) — `/game` preference gains a shell restore guarantee (captured before the profile write, re-applied if the model drops/flips it); spinner aligned to the design mockup (`⠋⠙⢸⢴`); factory-reset confirm wording aligned across prompt/SPEC/code (`yes` or `evet`); hard language lock in SOUL.md (reply language: user's current language → USER.md preference → English base; no mid-session drift); half-finished session records: batch `[Y/n]` question — default recover (salvage: transcript replayed through the closing flush, files written retroactively, record marked `.done`), `n` deletes; catalog upsert now also at session OPEN so factory reset covers cancelled sessions' projects (plus the cwd project even when uncataloged); Turkish-uppercase reject fix (`HAYIR`); unchanged mentor-file saves no longer reach the LLM (first-sight baseline seed — edits now go as diffs); leading speaker-glyph `●` from the model is stripped deterministically by the shell. Black-box behavior test round: 13/13 pass, isolated HOME, zero functional bugs.

- 2026-08-16: TUI design system applied — approved Claude Design project brought into code; `src/tui/theme.rs` as the single source (semantic `Color::Indexed` + glyph pairs `●❯·✓⚠✗▸`; all modules + `ui.rs` plain-ANSI + termimad skin feed from it); amber warning (179 — raw `Color::Yellow` is gone), violet game accent (141), orange discipline (≤2 at rest, test-locked); notice tiers `· / ⚠ / ✗` (texts unchanged, only prefix+style); context gauge flips amber at ≥70%; stats/topics column alignment + header rule; exam card drawn by the model (GOAL.md `## Mock Exams` format rule — the shell parses nothing) + game glyph note in TEACHING.md; zero behavior/text regression (presentation only); v0.18.0.

- 2026-08-15: Mock exam generator — `/exam` in goal mode; goal gate (no `## Hedef` in the topic's approach → gate notice, never reaches the LLM); on a goal topic `exam_prompt` is injected as a normal user turn (one question at a time, hint ladder + teaching SUSPENDED during the exam, scoring against the goal's threshold + per-map-item breakdown, weak items become gap candidates); result recorded at closing into the `## Hedef Durumu` measurement log (`date | mock exam | score`); rules live in embedded GOAL.md `## Mock Exams` (loaded only for goal topics); v0.16.0.

- 2026-08-15: Gamification mode — opt-in `/game on|off` (persisted in USER.md `## Tercihler`, shell-managed `set_game_pref`, idempotent, never disturbs the rest of the file); when on, TEACHING.md `## Gamification` rules drive XP (map states + process points, independent of correctness) / levels (0/100/250/500/1000/2000, Çırak→Usta) / badges — dosed (one short line at milestones only), ADHD-safe (no guilt on broken streaks; `streak: 0` is structurally unprintable); `/exam` = boss fight; `[GAME]` streak line at opening (`game_streak_line`); bare `/game` = status (never reaches the LLM), On/Off inject a mode turn via the `/exam` pattern; v0.17.0.

- 2026-08-15: Progress summary/motivation — global append-only `learner/history.md` (one line per closing flush: topic | map% | settled); `usta stats` weekly summary (per-topic sessions + map/settled deltas, current + longest streak); ADHD-safe tone — "current streak: 0" is never printed anywhere, a broken streak shows only the longest with a positive frame; welcome box "This week: N session(s) · streak M day(s)"; no LLM (the shell counts); v0.15.0.

- 2026-08-15: Material ingestion — the user drops md/txt (PDF→txt when pdftotext exists) into the visible `materials/` folder; at new-topic introduction the shell injects a deterministic digest (heading skeleton + excerpts, UTF-8-safe caps); Usta anchors the curriculum to the material's sections with `— kaynak: <file> §<section>` refs (web scope-guarding kept; missing critical items enter as `— kaynak: web`); scaffold creates `materials/`; v0.14.0.

- 2026-08-15: Onboarding-lite + versioning — when no backend is found on a TTY, a guided wizard (install Claude Code and press Enter to re-check / paste an API key, process-env only, never persisted / `q` to quit); pipe/CI keeps the plain error; Cargo version aligned with the SPEC (0.1.0 → 0.13.0) + minor-bump policy + first tag; v0.13.0.

- 2026-08-15: Spaced repetition — recall questions carry a `| due: YYYY-MM-DD | ivl: <days>` tail; simplified SM-2 ladder (`1→3→7→16→35→90` days, no ease factor); the opening drill asks only what's due (max 3, oldest first) and skips when nothing is due; the welcome box shows "Reviews due today: N" / "No reviews due today"; a question passing comfortably at `ivl: 90` retires into `Kapatılanlar`.

- 2026-08-15: Exercise/artifact loop — Usta assigns a deliverable, the user writes it under `exercises/` → the same Socratic feedback loop (reviewed against the assignment, no solutions handed over), `cargo check` skipped; open exercises live in progress + get reminded at opening; scaffold creates `exercises/`; pedagogy in TEACHING.md.

- 2026-08-14: Project context layer + proactive start — visible `mentor/` folder (`PROJECT.md` project definition + `PROGRESS.md` project state with an append-only decision log), written by Usta from the introduction or hand-written by the user and read at every opening; reset never touches it; with a filled `PROJECT.md` and no topics yet, an empty Enter at the topic prompt makes Usta propose the starting topic + rationale + a first step from the plan (one mini LLM call, session-reset parity), accepted suggestion carried into onboarding.

- 2026-08-13: Visual explainer full package — /show + anime.js player + Excalidraw design language (rough.js/Excalifont) + glass notch + detail panel + [[show:]] natural-language trigger + retention.

- 2026-08-12: UX package — multi-line input (Ctrl+J), single-Esc cancel, `/watch` toggle, English base language + language mirror, `/help`.
