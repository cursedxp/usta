# Usta — Design Spec

> A terminal-native, Claude (Opus)-powered, Rust-written, domain-agnostic **Socratic learning mentor**. It runs learning-by-doing. It never does the user's work for them. It doesn't make things up — if it doesn't know, it researches.

- **Status:** draft (v0.1) — 2026-08-06
- **Owner:** Anil (cursedxp)
- **First user:** Anil (learning Rust). Later: any language/domain, anyone.
- **Repo:** `cursedxp/usta` (public), a separate project inside headspace — its own git, headspace keeps it in `.gitignore`.

---

## 1. Purpose and Philosophy

**In one sentence:** The master who sits beside you and trains you while you do the real work.

**Core philosophy:** Learning by doing. No passive lessons. You learn inside the flow while building the real project. The project = the learning vehicle.

**The gap it closes:** The user knows "how to do it"; not "how an engineer approaches the work" — good spec, good architecture, reading scale, technology choice. Usta teaches this meta-skill.

**Initial purpose:** Anil's own use + giving feedback as he uses it, to develop Usta together. A double loop: Usta teaches Anil, Anil improves Usta.

## 2. Hard Rules (non-negotiable)

1. **Zero autonomous action.** Usta doesn't write code, doesn't fix files, doesn't do the user's work.
   - **It may show:** what's wrong · how it should be done (approach, logic, direction) · a tiny illustration/pseudocode that shows the concept.
   - **It may not write:** a working/copy-paste solution into the user's project. The one who fixes is always the user.
2. **No making things up.** On a topic it doesn't know it doesn't speculate → it **researches** (web/sources), then teaches.
3. **Prevents paralysis.** Perfecting the spec/plan mustn't kill getting started. Usta is the "enough, get in the water" guardian (ADHD-aware).
4. **Project-based.** Feedback isn't theory in the air — it's anchored to the user's real project, with rationale.
5. **Audits its own health.** Broken wiki-links, stale/inconsistent files → catches them autonomously, repairs/warns.

## 3. Persona

- **Senior / domain expert.** Behaves like someone who has mastered the subject.
- **Responsible for code quality** (in code domains) — holds the "is it good" standard, not "does it run".
- **Kind but pushing.** Cuts perfectionism, ADHD-aware (headspace `mentorship-mode` spirit): no judgment, lower the threshold, break it into pieces.
- Knows the user (ADHD, the "get in the water" mantra, personality, communication style) → gives the right support accordingly.

## 4. Capabilities

1. **Teaching (in-flow):** teaches at the moment, at that step, while the project is being built — the next step + a language/topic tip.
2. **Meta-skill teaching:** how to write a good spec, how an engineer thinks, work planning.
3. **Scale-aware architecture:** teaches reading the project's scale (1-person vs 1000-person). Prevents over/under-engineering. Not a memorized pattern — "what's enough in this context, and why".
4. **Technology choice:** recommends/explains technologies fit for the task, surfaces what the user doesn't know, teaches the why-this-technology rationale. Currency → comes from live research (there is NO separate "self-updating system" — YAGNI).
5. **Proactive feedback:** watches the code being written, speaks up if it sees a problem ("this shouldn't be like this, because...") — without you asking.
6. **Gap diagnosis:** catches weak points by watching the user's work (evidence-based: which code it was seen in).
7. **Curriculum planning:** designs targeted mini-lessons/exercises for the detected gaps. Plans and suggests — doesn't do it for you.
8. **Approach selection by domain:** not every project is spec-worthy. Software → spec+architecture; marketing → brief/hypothesis/measurement; learning git → "spec unnecessary, just do it". Usta picks the right structuring step.

## 4.5 Launch / Usage

- **`usta`** (no args) → if missing, the global + project `.usta/` is **set up automatically** (bootstrap), then it asks for the topic (in a TTY) or falls back to `general` (piped; when the model returns `general` or `genel`, the slug is re-derived from the raw input instead).
- **`usta start <topic>`** → the topic is given explicitly (slugified: `"JavaScript Basics"` → `javascript`). A shortcut.
- **`usta init`** → optional; only sets up (without starting), writes per-file state. No longer required.
- Topic = the learning title / filing key (`progress/<topic>.md`). **What you build** lives in `mentor/PROJECT.md` (Usta writes it before the introduction, the user can hand-edit it); Usta looks there first, and asks if it isn't there. The project's status is in `mentor/PROGRESS.md` (Done/In Progress/Next + append-only Decisions).
- **Project-aware start:** in the first session (`local` empty), if `mentor/PROJECT.md` is filled, at the topic entry **an empty Enter = a starting suggestion** — Usta suggests a topic + rationale + a concrete first step from PROJECT.md (a single mini-call, followed by an unconditional session reset — slug mini-session parity); if the user confirms, the session opens on that topic; the suggestion text is carried into onboarding as `intro` (Usta doesn't re-explain its own suggestion, it continues from the first step). In this case the welcome/prompt line shows the hint "PROJECT.md found — press Enter, Usta suggests where to start." If `local` is filled, an empty Enter = **resume** (takes priority, unchanged). No suggestion on the plain/pipe path.

## 4.6 Pedagogy Layer (v0.3)

It's not the teaching direction but the **recall direction** that's optimized — durable learning happens in the production that comes out of the user (testing/generation effect):

1. **Opening drill (due-aware, v0.13):** at session start, from the progress file's "Recall questions" only the **due ones** are asked (`due:` today or earlier; an old item with no queue counts as due), at most 3, oldest-due first (if progress exists the shell triggers it, Usta gets the first word). If no question is due, the drill is skipped with a single sentence "no reviews due today" and work begins directly. A 2-min warm-up — a low-threshold "get in the water" ramp for ADHD. Detail: §4.13.
2. **Explain-mode (Feynman):** at a slice's close the roles flip — the user explains what they wrote; a gap in the explanation is a gap signal (better than the code).
3. **Hint ladder:** question → concept name → pseudocode; never code (Hard Rule 1). As the level rises the ladder shortens (fading); after ~2 rounds stuck on one rung, drop one rung (ADHD balance).
4. **Prediction protocol:** on save it runs `cargo check` (60s timeout, 4KB truncation, silently skipped in non-Rust projects); if there's an error Usta doesn't state the result, it makes you predict first (hypercorrection).
5. **Error log:** in progress `error type | counter | last example`; 3+ repeats = `GAP CANDIDATE` → a mini-exercise suggestion into the curriculum.

All the rules live in USTA.md; Rust only triggers (opening turn, check runner, progress format).

## 4.7 Management Commands (v0.4)

- **`usta topics`** — the global catalog is listed: `topic | project | last session`. No LLM needed.
- **`usta reset <topic>`** — the current project's progress for that topic is deleted (`[y/N]`-confirmed; `e/evet` silently accepted), removed from the catalog. The "let me relearn this topic" scenario.
- **`usta reset --factory`** — the `.usta/` of ALL projects in the catalog + **the current project's `.usta` (even if it isn't in the catalog)** + the global brain are deleted; the list is shown first, and "yes" is typed to confirm (display is English-only; "evet" stays silently accepted). Since reset is already word-confirmed, there's no extra question for leftovers — the next launch starts clean. Other non-catalog old projects are out of scope (a warning + a `find` hint is printed).
- **The catalog auto-updates:** both session **open** and the closing flush upsert `- topic | project-path | YYYY-MM-DD` into the `## Records` section at the end of `learner/index.md` (same line, deduplicated by the `(topic, project)` key — open+close a single line). Thanks to the open-upsert, every project that has been opened even once — even if the flush never runs, even if the session is cancelled — ends up in the catalog; so `reset --factory` can also clean the leftovers of cancelled sessions. Side effect (accepted): a progress-less topic (opened and cancelled) may show up in the catalog — `usta topics` already tolerates this. Also, since the index is in the system prompt, Usta is aware of all titles — isolation isn't broken (progress is loaded only from the active topic).

## 4.8 Every-Topic Learning (v0.6)

The domain list isn't expanded by hand — the system expands itself:

- **Approach generation:** on a topic with no approach, the first session opens with `[YENİ KONU — TANIŞMA]` (NEW TOPIC — INTRODUCTION) — an open conversation, not a form; the user holds the direction. Usta derives the domain's nature from `_default.md`'s three questions (practice / output / feedback), and at close `.usta/approaches/<topic>.md` is written. **Living document:** revised during the session, hand-editable.
- **Curriculum map:** in the first session a FULL web-researched map is extracted to `.usta/learner/curriculum/<topic>.md`; each item is `not seen/seen/settled/deepened`. Updated at close. Scope guarding ("X is still open on the map"), drill feeding (the settled-but-stale zone) and depth tuning (no going shallow) all work from here.
- **Brain loading is general:** ALL files under `approaches/` (global ∪ project, override in the project's favor, alphabetical) + the active topic's curriculum + progress enter the system prompt. The approach choice is made by a USTA.md rule, not by code.
- **Multi-file close:** a single call produces `progress`(always) / `approach` / `curriculum`(when changed) with `===FILE: <name>===` dividers; a divider-less response is treated as backward-compatible progress; an unknown name is skipped with a warning.

## 4.9 Context Management (v0.7)

- **Indicator:** under each response `▓▓░░░░░░ context 41k/1000k` (the last call's input+cache total / **window per model**); ≥70% yellow. Not drawn in plain mode or when token info is unavailable.
- **Automatic interim-save + compaction:** at the 70% threshold a flush runs (progress/approach/curriculum land on disk), the system prompt is reloaded with the fresh files, history is trimmed to a `[CHECKPOINT]` note + the last 4 turns, and the CLI `session_id` is reset. The user's flow isn't interrupted. If the flush fails, compaction is cancelled — history is never dropped before data is written. The loss is minimal: what matters is already in the files (progress = the distilled session).
- **Visual:** the banner carries the model label (`opus · cli`), the user prompt is `❯`, the Usta block has 2-space padding + width-wrapping.

## 4.10 Goal-Directed Learning (v0.8)

Two modes of learning in one system: **exploration** (open-ended curiosity) and **goal** (certificate/level/deliverable — date + threshold). The introduction clarifies which one it is.

- **Generic goal record:** approach `## Goal` (what / date / threshold / format), progress `## Goal Status` (time remaining / map % / pace / measurement log). AWS SAA and Goethe B1 both use the same pattern.
- **Map from the official frame:** exam syllabus / exam guide / CEFR — web-researched, not guessed.
- **Pace guarding:** thanks to the system prompt's `===== TODAY =====` section the time remaining is computed; a one-line status at each open, a plan revision if it's at risk.
- **Format-matched drill:** the scenario is multiple-choice / a writing task / a rehearsal — the goal's real exam format.
- **Medium limit:** modules that don't work in a terminal are marked `external resource required` on the map — no fake completeness.

## 4.11 Hardening (v0.9)

- **Raw session recording + confirmed recovery:** every turn lands immediately in `.usta/sessions/<topic>-<time>.jsonl`; on a successful close, `.done.jsonl`. If the flush dies or the terminal crashes the session stays on disk — at open (TTY only) unmarked records are listed dim + a single batch question `recover N unflushed session(s)? [Y/n]` (default YES, the lossless side). **Recover** (Enter/`y`/`e`/`yes`/`evet` or an unrecognized input): for each record the transcript is parsed, the closing flush runs with that history (progress/approach/curriculum written retroactively), the record is moved to `.done.jsonl`; an empty transcript (no user turn) is silently `.done`d without an LLM; the backend session is reset between records; a parse/LLM/write error → warn + record left in place (asked again at the next open). **Delete** (`n`/`no`/`h`/`hayır`/`hayir` — uppercase included; since Rust gives lowercase 'I'→'i', the ASCII form is also accepted): all unmarked records are deleted + a single info line (`cleaned N stale session record(s)`). An error never blocks the open in any case. In pipe/script mode there's no recovery/deletion — only a warning is printed (exactly the old behavior).
- **Event-flood ceiling:** a 5+-file debounce batch (git checkout, format-all) passes without an LLM; `FileMemory` syncs silently.
- **Topic lock:** `.usta/.lock-<topic>` — a concurrent second session is opened with confirmation, progress isn't silently overwritten. In pipe mode: warn + continue.
- **Backup:** `write_atomic` copies the previous version to `.bak` — a bad model output can be rolled back.
- **Pruning + secret filter:** progress is pruned at a 20-item threshold; `.pem`/`.key`/`secret`/`credential` files never go from the watcher to the LLM.
- **Tool-transient filter (v0.20.2):** `is_ignored` also drops tool-artifact basenames that aren't real content — libgit2's `_git2_` temp files, editor swap/backup files (`~`, `.swp`, `.tmp`, `#...#`) — and a file that vanishes between the watcher event and the feedback read (`io::ErrorKind::NotFound`) is skipped silently instead of warning; binary (non-UTF-8) content is skipped just as silently (v0.20.3, `InvalidData`) — content-based, so any text file of any extension stays watched.

## 4.12 Exercise Loop (v0.12)

- **`exercises/` convention:** a visible folder, set up by the scaffold; Usta drops the deliverable in the chat, the user writes the file.
- **Path recognition:** `is_exercise_path` — any record whose root-relative or absolute path contains an `exercises/` component counts as an exercise.
- **Exercise feedback frame:** an "AS AN EXERCISE" mark is added to the watcher turn — evaluate against the assignment (not against perfection), the hint ladder applies as usual, a solution or a completable skeleton is never written.
- **Check-skip:** `cargo check` isn't run on paths under `exercises/` — an exercise works in every domain, code-specific verification isn't mandatory.
- **Persistence:** the `## Open exercise` section in progress holds the open assignment; it's reminded at session open, and once completed it's moved to `Retired`.

## 4.13 Spaced Repetition (v0.13)

Roadmap #3: recall questions are given a due-date, one that isn't due isn't asked, and one that is becomes visible at open — bringing the spacing effect to the drill.

- **Format (machine-readable queue):** `## Recall questions` items are `- <question> — <one-line answer> | due: YYYY-MM-DD | ivl: <days>`. An old item with no queue counts as due today (migration: the model adds the queue on the first close).
- **Simplified SM-2 (NO ease factor):** the interval ladder in days is `1 → 3 → 7 → 16 → 35 → 90`. Recalled comfortably → one rung up (`due = today + new ivl`); struggled/wrong or a new question → `ivl: 1` (due tomorrow); a question not in the drill → the queue stays unchanged.
- **Retirement:** a question that comfortably passes the `ivl: 90` rung is moved to `Retired` as a one-line summary and drops off the question list — progress doesn't bloat.
- **Opening drill:** only the due ones (`due` ≤ today), at most 3, oldest first; if none is due it's skipped with the single sentence "no reviews due today".
- **Welcome indicator:** the pure function `due_count(progress, today)` — `Reviews due today: N` (N>0) / `No reviews due today` (questions exist, none due) / no line (no questions at all).
- **The owner of the arithmetic = the model** (interval choice, queue writing — at the closing flush the model already writes the file); **the shell only counts** (`due_count` — the welcome indicator). The "thin shell" is preserved.

All the rules live in USTA.md (the closing/opening prompts); Rust carries only the `due_count` counter and the welcome render. Design detail: `docs/superpowers/specs/2026-08-15-spaced-repetition-design.md`.

## 4.14 Onboarding-Lite Wizard (v0.13)

The first half of Roadmap #4: instead of dying with a bare error when `backend::select()` finds nothing, a lightweight first-run wizard that guides you in a suitable environment kicks in — once setup is complete it continues **in the same process**.

- **Trigger condition:** `select()` returns `Err` AND stdin+stdout are a TTY (`std::io::IsTerminal`) AND `USTA_BACKEND` is not set. If any of these isn't met (no TTY — pipe/CI, or `USTA_BACKEND` is set), the wizard does NOT kick in, the existing `bail!` is preserved. If `USTA_BACKEND` has an invalid value that's a configuration error, not a missing-backend one — the wizard doesn't kick in here either.
- **Flow:** the wizard shows two options — a Claude Code CLI install (link + "then just press Enter here") or pasting an Anthropic API key (`sk-ant-...`). Input interpretation:
  - empty line → **Recheck**: `select()` is retried; if it succeeds, continue to the normal flow, otherwise the same prompt again.
  - a line starting with `sk-ant-` → **Key**: trimmed, written only to the process env (`std::env::set_var`) — **NEVER WRITTEN TO DISK**, not echoed back; then `select()` is retried (the API path is now found) + a one-line persistence hint ("add to your shell profile to skip this next time").
  - `q`/`quit` (case-insensitive) → **Quit**: a clean exit with the wizard's message.
  - any other input → a short warning + the same prompt again.
- **In-process scope:** the entered API key lives only in the running process's environment variable — it's not written to a file, keychain, or profile in any way; it's lost when the process closes (persistence is the user's own choice, the wizard only reminds).
- **Out of scope (deliberately deferred):** prebuilt binary, GitHub Releases, Homebrew tap, CI release workflow, persisting the key to disk/keychain, the full wizard (language/name/introduction flow), the model-selection wizard.

Design detail: `docs/superpowers/specs/2026-08-15-onboarding-lite-design.md`.

**Versioning policy:** each completed roadmap item is marked with a minor bump, tag `vX.Y.Z`.

## 4.15 Material Ingest (v0.14)

Roadmap #5: the user brings their own book/course notes, the curriculum is anchored to its chapters — web research is now complementary, not the spine.

- **`materials/` convention:** a visible folder, like the others (`exercises/`, `progress/`), set up by the scaffold. The user puts an md/txt file here; the shell discovers it automatically, the model creates nothing.
- **Digest injection ONLY at a new-topic introduction:** if `materials/` is non-empty and the topic is being opened for the first time (NOT in the resume/opening flow), the shell produces a deterministic digest and injects it into the model — a heading skeleton + short excerpts. In continuing sessions the digest is not re-injected; persistence comes from the source references in the curriculum map.
- **pdftotext optional:** if `pdftotext` is on PATH, PDF files are auto-converted to txt. Otherwise the PDF is skipped, a single info line is printed (with a `brew install poppler` suggestion) — no hard error, the flow continues.
- **Source-ref anchoring:** curriculum map items are bound to the material with `— source: <file> §<section>` references. What's persistent is NOT the digest but these references — the digest is a one-time injection, not carried across sessions.
- **Caps:** 8_000 characters per file, 16_000 characters total; the cut is made at a UTF-8-safe boundary and marked with `[truncated]`.
- **Web-research scope guarding is preserved:** if there's a critical topic the material doesn't cover, the map is filled with web research and the relevant item is marked `— source: web`.

Design detail: docs/superpowers/specs/2026-08-15-material-ingest-design.md

## 4.16 Progress Summary / Motivation (v0.15)

Roadmap #6: visible progress = fuel for ADHD, zero blame. Entirely shell work — no LLM call ("the shell counts").

- **Session history:** global `~/.config/usta/learner/history.md`, append-only, heading `# Session History`. The closing flush drops a line right next to the catalog update (`index::record`): `- YYYY-MM-DD | <topic> | map <P>% | settled <N>` (P = `curriculum_percent`, N = the count of `settled`+`deepened` items; the curriculum file is read from disk AFTER the flush — if there's no curriculum, `map -` / `settled -`). Multiple sessions on the same topic on the same day = multiple lines. A write error = warn, doesn't drop the session (same tolerance as the catalog).
- **`usta stats` command:** a last-7-days window — per topic, session count + map% first→last delta + settled first→last delta; overall: total sessions, current streak (consecutive days, any topic — counting back from today or yesterday), longest streak. No LLM needed, a pure parser + arithmetic. Listed in the `usta help`/`/help` text.
- **ADHD-safe rules:** `current streak: 0` is **never written on any surface.** If the streak is broken only `longest streak: N day(s)` is printed with a positive frame. An empty week (7 days with no session): `quiet week — your longest streak is still N day(s)`. If there's no record at all: `no sessions recorded yet — streaks start with the first one.` No comparison/shaming language in any output.
- **Welcome line:** in both the identity and full-mode boxes, when `week_sessions > 0`, `This week: N session(s) · streak M day(s)` (if M=0 the streak part drops, no line is lost). The data is read from `history.md` with a pure parser — no separate counter is kept.
- **Resume continuation panel (v0.21.0):** the resume path (`TopicChoice::Resume`, or a slug that matches a local topic) no longer prints the full-mode box on top of the identity welcome `ask_topic` already showed — it prints `welcome::render_resume` instead, a single-column panel carrying no identity (no logo, greeting, model, dir, or week/streak line) with what's being picked up, when it was last touched, and how far along the map it is. `usta start <topic>` is unchanged — that path never shows the identity welcome, so its full-mode `render_welcome` box stays the only frame. The panel's own orange budget is 2 elements (title, reviews-due count), separate from the identity box's budget above it.

Design detail: `docs/superpowers/specs/2026-08-15-progress-stats-design.md`.

## 4.17 Mock Exam (v0.16)

Roadmap #7: a real rehearsal mechanism in goal-directed (GOAL-mode) learning. `/exam` is a **prompt-injection command** — not a static intercept; the shell holds the goal gate, the exam itself (question generation, evaluation) is the LLM's work.

- **Gate: goal required.** If the topic's approach file (project override takes priority, otherwise global — same priority order as in `brain.rs`) has no `## Goal`, the shell prints a gate notice ("no goal set for this topic — /exam needs a goal (exam/certificate); set one in the introduction") and it **never goes to the LLM**. On a goal-directed topic, `exam_prompt(topic)` is injected into the session like a normal user turn (`session.push_user` + recorder + ask flow — the in-session counterpart of the opening drill).
- **Exam flow:** the model builds a mock exam from the curriculum map, follows the `## Goal` format from the approach (question style, time budget, passing threshold), weights toward weak/not-`settled` items, and states the question count and time budget up front. Then it asks **one question at a time** and waits for the answer; during the exam the **hint ladder and teaching are SUSPENDED** — a real rehearsal feel, no intermediate feedback. If you say "stop the exam" it ends early, and the answers up to that point are scored.
- **Result:** after the last answer the model scores against the goal's threshold, presents a short map-item breakdown (strong/weak), names the weak items as gap candidates, and reminds you that the result will be saved at close.
- **Timing soft (v1):** the shell keeps no time — a hard timer is out of scope; the time budget is only the model's verbal commitment.
- **Saved at close:** a single rule sentence was added to `closing_prompt` — if a mock exam (`/exam`) ran in this session, the result is written into the `## Goal Status` measurement log (`date | mock exam | score`), and the items that came out weak are written to `## Gaps`.
- **Rule home: GOAL.md** — embedded, loaded only on goal-directed topics. The `## Mock Exams` section carries the exam execution rules (one question, suspended hint ladder, score against threshold, breakdown, early finish, save reminder) and the pedagogical note (a mock = the strongest retrieval practice; after the exam the weak items return to normal teaching mode).
- **Out of scope:** a hard timer, a separate exam-history file (the measurement log is enough), a question bank/recurring patterns, a general quiz mode on goal-less topics (the drill already exists).

Design detail: `docs/superpowers/specs/2026-08-15-mock-exam-design.md`.

## 4.18 Gamification Mode (v0.17)

Roadmap #8: opt-in gamification — a visible dopamine loop for the ADHD brain. The narrative is entirely in the prompt/TEACHING layer; the shell only does toggle persistence + opening streak feeding ("thin shell").

- **Toggle + persistence:** `/game on|off` writes a `- gamification: on|off` line into USER.md's `## Preferences` section (shell-managed `set_game_pref` — idempotent, doesn't break the rest of the file). `/game` (no argument) = a status notice, **doesn't go to the LLM**. On/Off uses the same injection pattern as `/exam`: the line is turned into a `[GAME MODE ON/OFF]` info turn and left to the normal ask flow → the model applies the TEACHING.md rules from that point on.
- **The model does the narrative, the shell doesn't count:** XP is derived from the curriculum states (seen 10 · settled 25 · deepened 50) + process points (session +5, prediction +2, exercise submission +10 — independent of correctness); level thresholds 0/100/250/500/1000/2000 (Çırak → Usta); badges for a gap close / first exercise / 7-day streak / first boss; `/exam` = a boss fight.
- **Opening [GAME] feed:** when the game is on, the shell adds a single line to the opening turn from `history.md` (`game_streak_line`): streak>0 → `streak: N day(s) (longest M)`; broken streak → only `longest streak: M day(s)`. **ADHD-safe code guarantee:** `streak: 0` is structurally impossible to produce (test-locked) — not a prompt rule but a shell guarantee.
- **Closing protection:** the KEEP sentence in the prompt (the instruction to preserve the `## Preferences` section) was removed in v0.19 — the only assurance now is the shell restore guarantee (see §4.20). **Shell restore guarantee:** at the closing flush the shell captures the disk state of the `- gamification:` line BEFORE the write, and if after the profile is written the model dropped the line or flipped its value (`restore_game_pref`) it writes the preference back; if the user never toggled (no line) it doesn't touch it.
- **Rule home: GAMIFICATION.md** (embedded, conditionally loaded file — since v0.19 it's not inside TEACHING.md; loaded by the shell only when `- gamification: on`, and when off not a single game word is in the prompt — see §4.20). DOSE: one line at a milestone, NO score in every message. Overjustification guard: points are in the process, no penalty mechanic.
- **Out of scope:** the shell computing/persisting XP, a leaderboard, a visual badge, a separate game-data file (the level is derived from the curriculum — idempotent).

Design detail: `docs/superpowers/specs/2026-08-15-gamification-design.md`.

## 4.19 TUI Design System (v0.18)

The approved design system (Claude Design project) was applied to the code — behavior doesn't change, only presentation. Goal: a calm screen (ADHD), color semantics, color-blind safety, monochrome resilience.

- **Single source `src/tui/theme.rs`:** all TUI modules (+ `ui.rs` plain-ANSI, termimad skin) take colors/glyphs from here — scattered `Color::` literals were cleaned up. Colors are `Color::Indexed` (correct on truecolor terminals too).
- **Semantic palette + glyph pairs** (color-blindness/monochrome — color only reinforces the glyph, it doesn't carry meaning on its own):

  | Role | Color | Glyph |
  |---|---|---|
  | Brand / identity | orange 208 | `●` bullet · `❯` prompt |
  | Info / ambient | dim 244 | `·` |
  | Success | green 149 | `✓` |
  | Warning | amber 179 (the old `Color::Yellow` retired) | `⚠` |
  | Error | red 210 | `✗` |
  | Game / XP | purple 141 | `▸` |
  | Code (inline) | green 114 | — |

- **Orange discipline:** ≤2 orange elements on a static screen (the logo block = 1) — test-locked (`welcome_orange_discipline`). Orange = identity, never status.
- **Box/indicator language:** live frames are rounded `╭╮╰╯`; the table header underline is a thin `─` line; the gauge is `▓░`, amber at ≥70%; the spinner is `⠋⠙⠸⠴` ~120ms; exam progress is `●○`.
- **Notice layers:** `page_notice` `·` dim · `page_warn` `⚠` amber · `page_error` `✗` red — the existing texts unchanged, only prefix + style. The `ui::warn` buffer is routed to the amber layer at flush.
- **The exam card is NOT in the shell:** a format rule was added to GOAL.md `## Mock Exams` (`── Question N/M ──` header, `●○` progress, breakdown table) — the model draws it, the shell doesn't parse ("thin shell"). The game-line glyph note goes to TEACHING.md's DOSE rule (`▸`).

Design detail: `docs/superpowers/specs/2026-08-16-tui-design-apply-design.md`.

## 4.20 Prompt Diet (v0.19)

Binding principle: nothing that can be resolved deterministically by the shell is written into the prompt; a section whose condition the shell knows isn't loaded unconditionally.

- **Conditional brain table:**

  | File | Condition |
  |---|---|
  | GOAL.md | the topic's approach has `## Goal` |
  | GAMIFICATION.md | `- gamification: on` in USER.md |
  | MATERIAL.md | ≥1 `.md`/`.txt`/`.pdf` under `materials/` |
  | PREDICTION.md | `Cargo.toml` at the project root |

  GAMIFICATION.md, MATERIAL.md, PREDICTION.md were moved **verbatim** out of TEACHING.md (content unchanged) — the only difference is that the loading condition now lives in the shell (`brain::load_system_prompt`). When the condition is false the file's content isn't in the prompt at all (previously it went out every session as part of TEACHING.md).
- **Due-selection in the shell:** the selection of due recall questions is not the model's work — `welcome::due_questions(progress, today)` selects them (due ≤ today or untagged-legacy; oldest first, legacy first; at most 3). `due_count` is the uncapped length of the same scan — a single source. `opening_prompt` no longer carries a filtering instruction, it embeds the items the shell selected (or, when questions exist but none is due, the "no reviews today" skip; the "generate 2 small recall questions" rule survives only when there are no questions at all in progress). The due:/ivl: generation rules at close are unchanged — the arithmetic deliberately stays in the model (a separate job; not to be conflated with roadmap #10, spaced-rep arithmetic).
- **Mid-session gap:** the `/game on` info turn now embeds the GAMIFICATION.md rule text the shell reads from the global directory (`game_on_turn`; if the file can't be read, a short fallback text) — so the model learns the rules even after opening. `/game off` is unchanged.
- **Dead-sentence cleanup:** the "KEEP the '## Preferences' section... shell-managed" sentence in `closing_prompt` was removed — the shell's `restore_game_pref` guarantee is already enough, no need to repeat it in the prompt.
- **Global brain sync:** the three new files are Code-owned → the existing `write_global_defaults` synchronization distributes them automatically.

Design detail: `docs/superpowers/specs/2026-08-16-prompt-diet-design.md`.

## 5. Flow (one learning session)

```
usta start rust-takvim
  → detect domain → pick approach (spec needed? or not?)
  → per SLICE: ASKS ("what's your spec for this slice?")
      → you write a mini-spec
      → you interpret it together
      → you write the code
  → watches file saves → proactive, project-based feedback
  → if it doesn't know → researches → then teaches
  → slice ends → update progress + gaps + curriculum
  → next slice
```

**Per-slice spec:** a spec is never a huge up-front document. At the start of each slice a small one, specific to that slice. Do it and move on, another mini-spec at the next slice. Paralysis is solved by this cadence.

## 6. Architecture — "thin shell, thick brain"

- **Rust = thin shell:** CLI, LLM backend, file watching (`notify` crate), web research, health auditing.
- **Intelligence + personality = in markdown files** (the headspace pattern). Changing behavior = edit markdown, don't touch Rust.
- **Pluggable LLM backend (both supported — some have an API, some don't):**
  - **CLI (default):** the local `claude` CLI (Claude Code) → existing auth/subscription, **no API key, no token bill**. `--allowedTools WebSearch` opens research + enforces "no touching" at the tool level.
  - **API (optional):** the Anthropic Messages API via `ANTHROPIC_API_KEY` (reqwest), model `claude-opus-4-8`, server-side web_search, adaptive thinking.
  - Selection: `USTA_BACKEND=cli|api` takes priority; otherwise `claude` on PATH → CLI, otherwise if a key exists → API.
- **Call:** non-streaming (no client timeout in raw reqwest → robust). Streaming in a later version. The CLI backend continues a session with `--resume <session_id>` — the first call captures the id from `--output-format json`, subsequent turns send only the new message (on a stale session it falls back to the full transcript).

## 7. File Structure (wiki-linked)

```
usta/
  SPEC.md                # this file
  USTA.md                # core behavior: Socratic, no touching, no making things up, senior
  learner/
    index.md             # catalog of ALL learning titles — the "## Records" section is auto-upserted at close (v0.4)
    profile.md           # user: ADHD, "get in the water", personality, communication style
    progress/
      rust.md            # per-title progress + level (so it doesn't re-explain)
      javascript.md
    gaps/
      rust.md            # detected gaps + evidence
    curriculum/          # lessons planned per gap + curriculum map — from v0.6 on, project-local .usta/learner/curriculum/<topic>.md
      rust.md            # topic tree with not seen/seen/settled/deepened status
    tech-notes.md        # (optional, later) researched technology notes — so it doesn't research twice
  approaches/
    software.md          # spec + architecture (scale reading) + technology choice + code quality
    marketing.md         # brief/hypothesis/measurement
    _default.md          # the "spec unnecessary, just do it" logic
  projects/
    rust-takvim/         # active work context, per-slice mini-specs
  mentor/                # USER-FACING, visible — at the project root (OUTSIDE .usta)
    PROJECT.md           # project definition (What/Why/Scale/Stack/Out of Scope) — Usta writes it before the introduction, the user edits it
    PROGRESS.md          # project status (Done/In Progress/Next) + append-only Decisions — the closing flush writes it, reset doesn't touch it
  src/                   # Rust: cli, claude client, watcher, research, health
  Cargo.toml
```

**Isolation principle (from headspace):** every learning title is fully isolated — in a Rust session JS gaps don't bleed in. `index.md` links them all from above. `profile.md` is shared (the user is always the same).

## 8. Concurrent Multiple Titles

- Parallel active titles (Rust today, JS tomorrow — both ACTIVE).
- Usta loads the context of whichever title you're on in the session.
- Multiple terminals: Rust in one terminal, JS in another. Separate context, shared `profile.md`, separate progress.

## 9. Memory & State

- **Persistent (realized in v0.2).** At session close (`/quit`, Ctrl-C, Ctrl-D) Usta summarizes the session and rewrites `.usta/learner/progress/<topic>.md` in full as content (atomic: tmp+rename). The next session loads this file into the system prompt → doesn't re-explain. An empty session doesn't touch the file.
- **Multiple terminals (post-MVP):** a shared brain, concurrent writes are rare → hardening comes later.

## 10. MVP Boundary

**In:** the chat loop (pick project → ask → you write → Socratic feedback) + **file watching** (proactive code feedback) + **research** (no making things up).

**Out (later versions):** multi-terminal hardening · model routing · multi-domain polish beyond marketing · `tech-notes` cache · a self-updating tech system.

## 11. Decisions Made

**(v0.2)**

- **File-watch granularity:** 1s debounce (from the last save). Full content on first sight, unified diff on subsequent saves, files over 64KB out of watching (a one-time local warning).
- **Proactivity:** input on a separate thread (rustyline + ready handshake), the main loop `tokio::select!` — no waiting for Enter to give feedback.
- **Pedagogy triggers (v0.3):** the opening drill is triggered from the shell (if progress is non-empty); the `cargo check` result goes to the LLM with a `[... FOR YOUR EYES ONLY ...]` block — the hide/predict decision is in a USTA.md rule, not in code.
- **Global USTA.md update (v0.3):** the scaffold doesn't overwrite an existing file — after a behavior update you need `rm ~/.config/usta/USTA.md` + running `usta` once. A deliberate acceptance; file versioning is a v0.4 candidate.
- **Catalog format (v0.4):** `## Records` at the end of `learner/index.md`; the line `- topic | project-path | YYYY-MM-DD`; free text above the section is preserved; the date via `chrono` in local time.
- **Reset confirmations (v0.4, display anglicized v0.20.1):** topic reset `[y/N]`, factory reset a word confirmation (displays "yes"; "evet" silently accepted); closed/empty stdin = no (safe default). Reset commands work without a backend. The TUI start-suggestion confirm was the last surface still showing a Turkish token (`[E/h]`) — anglicized to `[y/N]` in v0.20.4, which also fixes the polarity it implied: `tui_confirm` returns true only on `y/Y/e/E` and false on any other key, i.e. the default is NO. The accepted key set is unchanged.
- **Presentation layer (v0.5):** roles are separated by icon — `●` (orange 208) the Usta block, `■` the user prompt, a faint `·`/`!` a system notice. Usta responses are markdown-rendered with termimad; a spinner while waiting for the LLM. Plain output if not a TTY or if `NO_COLOR` is set (pipe/test compatibility). The behavior layer was not touched.
- **Topic entry (v0.5, refined later):** **no rejection** at the TTY prompt — write it short or describe it in a sentence. Short input (≤2 words) is slugified locally (Turkish simplification + hyphen; `"temel Linux güvenliği"` → `temel-linux-guvenligi`). **If a sentence is written, a short call to the model** extracts what you want to learn and picks the most sensible slug (`"ben rust ile bir todo yapmak istiyorum"` → `rust-todo`); the format is guaranteed by `slugify_topic`, on a call error it falls back to the local slug. The chosen slug is announced. The detail is again in the chat. `usta start <topic>` and pipe behavior are unchanged.
- **Every-topic (v0.6):** approach files are produced not by hand but by the first-session introduction; the curriculum lives project-local (`.usta/learner/curriculum/`) — instead of the global `learner/curriculum/` in §7 (isolation: the map too belongs to the topic+project context). The closing divider format is `===FILE: <name>===`.
- **Context (v0.7):** the window is **derived per model** (`backend.context_window()`: opus/sonnet/fable 1M, haiku 200k) — not fixed; the compaction threshold is 70%, the preserved queue is 4 messages; measurement = the last call's `usage` total (input + cache_read + cache_creation) — no separate counter is kept, the source is the API/CLI report.
- **Goal-directed learning (v0.8):** the goal is not a separate mode but an approach field; the date reference is from the system prompt's `TODAY` section (`load_system_prompt` took a `today` parameter — the model's clock is unreliable). Pace/measurement lives in progress, there's NO goal logic on the code side (the thin shell is preserved).
- **Hardening (v0.9):** transcript/lock errors are warn-and-continue (never break the main flow); the batch ceiling is 5; the backup is single-generation (`.bak`); a half session isn't processed automatically, only announced (recovery is the user's decision — YAGNI).
- **Topic entry in the TUI (v0.11):** a topic-less interactive `usta` first prints an identity-welcome (logo + saved topics), then asks the topic from the input box — a Claude-style "welcome on top, question below". `usta start <topic>` a full-mode welcome (learning status) + the drill directly. Slug resolution is inside the TUI (≤2 words local `slugify_topic`, a sentence → `SLUG_SYSTEM` LLM + spinner, `finalize_slug`). Topic-dependent setup is in the `build_session` helper (system+session+lock+recorder+has_progress) — shared by both the TUI and plain; `run` returns artifacts, the close is shared in `main`. The lock-conflict confirmation is single-key in the TUI. The plain path (`NO_COLOR`/pipe) was preserved verbatim (rustyline `resolve_topic`). **The embedded default profile is nameless** — a new user is greeted generically (personal identity was removed from the seed). Detail: `docs/superpowers/specs/2026-08-07-tui-topic-entry-design.md`.
- **Interface — inline TUI (v0.10):** the interactive path was moved to a Claude Code-style ratatui `Viewport::Inline` — the bottom region (live input box + status line: spinner + context indicator) is drawn continuously, while permanent content (the opening box, Usta responses, file feedback) lands in normal **scrollback** via `insert_before`. **NO alternate screen** — terminal history is preserved (scroll up/copy). Input via crossterm `EventStream` + `tui-input` instead of rustyline; while waiting for the LLM an inner `select!` spins the spinner, Enter locked (single turn). The `●`/`■`/markdown visual language of v0.5 is preserved but now lives in the TUI flow. **The plain path (`ui::is_plain()`: no TTY / `NO_COLOR`) is the old behavior verbatim** — the rustyline loop as-is; the TUI never opens (pipe/CI/test safe). Compaction/flush output is isolated in the TUI with a `TUI_ACTIVE` gate (doesn't dirty stdout in raw-mode; spinner no-op, notice buffer→viewport). Detailed design: `docs/superpowers/specs/2026-08-07-tui-interface-design.md`.
- **Progress summary (v0.15):** `history.md` is global and append-only — not project-local, because a streak is consecutive days on "any topic" (an exception to the isolation principle, because the motivation signal is cross-topic). The `current streak: 0` ban is enforced at the code level (the pure function `render_stats` is test-locked) — the ADHD-safe tone is not a prompt rule but a shell guarantee. Version: `0.15.0`.
- **Mock exam (v0.16):** `/exam` is not a static intercept but a prompt-injection command — the shell only holds the goal gate (if there's no `## Goal` it never goes to the LLM), the exam itself (question generation, single-question flow, suspended hint ladder, score + breakdown) flows entirely in the LLM via the `exam_prompt` injection and the GOAL.md `## Mock Exams` rules — the "thin shell" principle preserved. Version: `0.16.0`.
- **Gamification (v0.17):** `/game` is not a static intercept but a prompt-injection command like `/exam` — the shell only holds the toggle persistence (USER.md `## Preferences`, `set_game_pref` idempotent) and the opening streak line (`game_streak_line`; `streak: 0` is structurally impossible to produce — an ADHD-safe shell guarantee); the XP/level/badge narrative flows entirely in the LLM via the TEACHING.md `## Gamification` rules ("thin shell"). Version: `0.17.0`.
- **TUI design system (v0.18):** the visual language was pulled to a single source (`src/tui/theme.rs`) — semantic color `Color::Indexed` + glyph pairs, all TUI modules + `ui.rs` plain-ANSI + termimad skin fed from here (no scattered `Color::` literals). Color reinforces the glyph, it isn't status (color-blind/monochrome safety); orange = identity, ≤2 on a static screen (test-locked). The exam card isn't drawn in the shell — the model draws it via the GOAL.md `## Mock Exams` format rule, the shell doesn't parse question-state ("thin shell" preserved). Behavior/text unchanged, only presentation. Version: `0.18.0`.
- **Module size budget (v0.22.0):** a module whose production code (test module excluded) exceeds 600 lines needs a documented reason to stay unsplit. A test module that bloats a file moves to its own file instead (`#[cfg(test)] #[path = "..."] mod tests;`), it doesn't count against the budget. Grew out of `main.rs` reaching 3045 lines with no rubric anywhere ever asking "has this file grown too large" — see `docs/superpowers/specs/2026-08-16-module-split-design.md`. Version: `0.22.0`.
- **Module size budget, status (v0.23.0):** the two modules over budget when the rule shipped — `tui/run.rs` (1185) and `tui/welcome.rs` (797) — were split (cleanup round); no module in the crate exceeds the budget now. `run()` itself stays 558 lines inside the 591-line `tui/run.rs` by documented exception (zero test coverage on the function, five mutable values live across `.await` in its `select!`) — see `docs/superpowers/specs/2026-08-17-cleanup-round-design.md`. Version: `0.23.0`.

## 12. Open Decision Points (clarified in the implementation plan)

- The full format of the `approaches/*` templates (representation of the structuring-step per domain).
- The example/pseudocode limit: "a tiny illustration showing the concept is OK, writing a solution into your project is forbidden" — how it's enforced in practice (a prompt rule).
- The research tool: which web search/fetch mechanism.
