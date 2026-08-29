# Usta

> A Socratic engineering mentor in your terminal that teaches you **by doing**.
> It doesn't write code, doesn't make things up, doesn't think for you — **it asks, tests, and guides.**

Usta ("master craftsman" in Turkish) is not a code completer. It's the master sitting
next to you: it trains you while you do the real work. The goal isn't your code —
it's **your growth.**

Domain-agnostic — Rust, JavaScript, marketing, whatever you're learning. First user: its author.

---

## Philosophy

- **Learning by doing.** No passive lessons. You learn inside the flow of building a real project.
- **Zero autonomous action.** Usta shows you what's wrong and how to approach it — but **you write the code.** No copy-paste solutions.
- **No fabrication.** If it doesn't know, it researches the web first, then teaches.
- **Thinks like a senior.** Scale-aware architecture (1-person vs 1000-user), technology choices, code quality.
- **"Thin shell, thick brain."** Behavior lives in editable markdown files, not in Rust (`USTA.md`, `learner/`, `approaches/`).

## Highlights

| | |
|---|---|
| 🧠 **Persistent memory** | At session close it writes what you learned to `progress/<topic>.md`. The next session knows it — no re-explaining, it targets your gaps. |
| ⚡ **True proactivity** | Feedback arrives the moment you save a file — no Enter needed (debounce + `tokio::select!`). Full content on first sight, unified diffs after. |
| 📋 **Project context** | Project definition + status live in a visible `mentor/` folder (`PROJECT.md` + `PROGRESS.md`). Usta fills them from the introduction — or you write `PROJECT.md` yourself and Usta starts already knowing your plan. Updated every session; your hand-edits are preserved. |
| 🚀 **Proactive start** | Don't know where to begin? With a filled `PROJECT.md`, just press Enter at the topic prompt — Usta reads your plan and proposes the starting topic, the why, and a concrete first step you can start today. You confirm, the session opens. |
| 🎓 **Pedagogy layer** | Opening drill (recall), explain-mode (Feynman), hint ladder (fading), prediction protocol (it won't tell you the `cargo check` result — it makes you predict first), spaced repetition (recall questions carry due dates on a 1→3→7→16→35→90-day ladder — the opening drill asks only what's due; the welcome box shows "Reviews due today: N"), and in goal mode, `/exam` runs a mock exam built from your map — no hints, real scoring against your target, weak spots become gaps. |
| 🎮 **Gamification (opt-in)** | `/game on` and Usta narrates XP, levels (Çırak → Usta) and badges — points reward the process (showing up, predicting, submitting exercises), never punish. Mock exams become boss fights. ADHD-safe: a broken streak shows your longest, not a guilt trip. |
| 🏋️ **Exercise loop** | Usta assigns a deliverable, you write it under `exercises/` — saving triggers the same Socratic review loop, in any domain (a GTM brief, a German essay, a Rust snippet). Open exercises survive sessions and get reminded at opening. |
| 🔎 **Research** | Searches the web for what it doesn't know (WebSearch) — no making things up. |
| 🌍 **Any topic** | Not Rust-specific. For a new topic (Linux security, GTM, anything) Usta **derives the approach through an introduction** and builds a web-researched **curriculum map** (`not seen → seen → settled → deepened`). Scope guarding: nothing stays hanging in the air. |
| 📚 **Bring your own material** | Drop your book/course notes (md/txt — PDFs auto-convert if `pdftotext` is installed) into `materials/` — Usta anchors the curriculum to its chapters, assigns reading, and quizzes you on it. You read; it never summarizes the book at you. |
| 🎨 **Terminal UI** | Claude Code-style inline TUI (ratatui): two-column welcome box on open (learning status + what's next), live four-sided input box, sticky status line (spinner + context gauge); a calm, colorblind-safe visual language (glyph+color pairs, one accent), designed in a full TUI design system. The flow stays in normal scrollback — scroll up, copy. Auto plain mode on pipe/`NO_COLOR` (scripts don't break). |
| 🗂️ **Management** | `usta topics` shows what you're learning where; `reset` clears a topic or everything. |
| 📈 **Visible progress** | Every session lands in a lightweight history — `usta stats` shows your week (sessions, map %, settled items) and streaks. Broken streak? No guilt: it shows your longest instead. |

## Install

```bash
git clone https://github.com/cursedxp/usta
cd usta
cargo build --release
# optional: cargo install --path .
```

**LLM backend** (either one is enough):

1. **Claude CLI (default, recommended)** — if [Claude Code](https://claude.com/claude-code) is on PATH, Usta uses it. Your existing subscription, **no API key needed**.
2. **Anthropic API** — `export ANTHROPIC_API_KEY=sk-ant-...`

Force one with `USTA_BACKEND=cli|api`.

First run with no backend? Usta walks you through it — install Claude Code or paste an API key, no restart needed.

## Usage

```bash
usta                    # start — asks for the topic (shortest path, use this)
usta start rust         # give the topic upfront — a topic argument requires 'start'
usta topics             # what am I learning where? (catalog)
usta stats              # this week's sessions + map/settled deltas + streaks
usta reset rust         # reset this project's Rust progress (confirmed)
usta reset --profile    # reset only your profile — Usta starts without knowing you (backup: profile.md.bak)
usta reset --factory    # reset everything — as if Usta never met you (word-confirmed)
usta init               # just set up the scaffold (optional — start does it anyway)
```

> **Note:** To pass the topic as an argument you need `start` — `usta rust` gives "unknown command" (the first arg is taken as a command). A bare `usta` opens the session and asks for the topic. Run it in any project: if `.usta/` is missing it's set up silently.

A learning session:

```
usta start gtm            # could be rust, gtm, anything
  → new topic (no progress) → INTRODUCTION: Usta derives the approach + curriculum map
  → (if any) opening drill: 2-3 recall questions from the map + "where we are, what's next"
  → you work (code / plan.md / whatever), you save — quietly noted, never interrupted
  → you write; your saves ride along with your message and Usta answers both, Socratically (without doing the work)
  → on a cargo check error: it won't tell you — "where does this blow up?" — predict first
  → /quit → progress + approach + curriculum updated, catalog refreshed
```

Practice: Usta assigns exercises into `exercises/` — write, save, and say you're done: the review comes with your message. No solutions handed over.

In-session commands: `/watch on|off` (file feedback) · `/watch live` (immediate review per save) · `/show [topic]` (animated visual explainer) · `/exam` (goal mode: mock exam) · `/game on|off` (gamification) · `/help` · `/quit` · `/context` (what fills the context window; exact bytes; token counts are estimates).

### Companion watching

Usta watches your files but never interrupts. By default, saves accumulate quietly and ride along with your next message — the mentor sees your changes and your words together, in one turn, and your words are the last word. The status line counts what's noted (`👁 watching · 2 changes noted`); the counter resets when delivered. Files saved together are merged into a single block, and repeated saves of the same file collapse into one diff at delivery time. Structural changes ride along too — a new directory or a deleted file shows up as a one-line note, never its contents. In a Cargo project, Usta also remembers the last `cargo check` verdict: while it's red, the status line shows a dim `✗ check failing` and the mentor won't call a step done.

Want a review the moment you save? Turn live mode on: `/watch live` (toggle; `on`/`off` also work) for the session, or add a `watch: live` line to the topic's approach file (`.usta/approaches/<topic>.md`, project override first) to make it the topic's default — the session-only command never writes back to the file. Live feedback uses plain review framing. `/watch off` stops watching entirely — anything already noted but not yet delivered is dropped, with a one-line notice saying so.

### Project context: the `mentor/` folder

Every project gets a visible `mentor/` folder next to `.usta/`:

- **`mentor/PROJECT.md`** — what you're building, why, at what scale, with which stack, and what's out of scope. Usta writes it after the first introduction — or **you write it yourself before the first session** and Usta starts already knowing the plan, without re-asking project basics.
- **`mentor/PROGRESS.md`** — the *project's* state (done / in progress / next) plus an append-only decision log ("why we chose X"). Updated at every session close. Separate from *your* learning progress, which lives in `.usta/learner/`.

Both are plain markdown, meant to be read and hand-edited — Usta never overwrites your edits, and `reset` never touches them.

**Proactive start:** on a true first run, Usta opens a short introduction conversation before any topic is chosen (say "let's start" any time to skip forward). In a project with a filled `PROJECT.md` and no topics yet, pressing Enter on the empty topic prompt starts a conversational suggestion that already knows your profile and plan — steer it in conversation instead of a yes/no confirm. Typing a topic still works, of course — the suggestion is for the "I don't know how to begin" moment, and it derives from your plan rather than being biased by whatever you'd have typed.

## Interface

In an interactive terminal Usta opens a **ratatui inline-viewport TUI**: a live input box + status line at the bottom, while the permanent flow (Usta's replies, file feedback) is printed to normal **scrollback** — scroll up to read or copy history. No alternate screen; your terminal history is preserved.

A bare `usta` (Claude Code-style): the **identity welcome** box appears on top (logo + your saved topics), and the input box below asks for the topic — type a word, or describe it in a sentence (the model reduces sentences to a short slug). With a filled `PROJECT.md` you can also just press Enter and Usta will start a conversational suggestion. `usta start <topic>` shows the **full-mode welcome** (learning status: level, map %, what's next) and starts directly.

No TTY or `NO_COLOR=1` → the TUI never opens — it falls back to the plain line mode (pipe/CI/script safe).

## How it works — "thin shell, thick brain"

Rust is only the shell: CLI, LLM client, file watcher (`notify`), `cargo check` runner, markdown loader. **The intelligence and personality live in markdown:**

```
~/.config/usta/          # GLOBAL brain (set up once, shared across all projects)
  USTA.md                #   core behavior + pedagogy rules
  SOUL.md / TEACHING.md  #   persona + pedagogy (exercises + core teaching loop)
  GOAL.md / GAMIFICATION.md / MATERIAL.md / PREDICTION.md  #   conditionally loaded (goal set · game on · materials/ present · Cargo project)
  USER.md                #   who you are (your learning style + preferences) — living document
  learner/index.md       #   ## Records — topic | project | date catalog
  learner/history.md     #   session history — powers `usta stats` + streaks
  approaches/            #   software.md, _default.md — approach per domain

<project>/mentor/        # PROJECT docs (visible, user-facing — yours to edit)
  PROJECT.md             #   what you're building, why, scale, stack, non-goals
  PROGRESS.md            #   project state (done/doing/next) + append-only decision log
<project>/exercises/     # your exercise deliverables — saving triggers review
<project>/materials/     # your book/course notes — curriculum anchors to them

<project>/.usta/         # PROJECT internals (per project)
  learner/progress/<topic>.md      #   level, gaps, recall questions (with due dates), error log
  learner/curriculum/<topic>.md    #   web-researched curriculum map (status-tagged, source refs)
  approaches/<topic>.md            #   Usta's derived topic-specific approach (living document)
  sessions/                        #   live session transcripts (crash-safe)
```

Changing behavior = edit markdown, don't touch Rust. (Global behavior files aren't overwritten by the scaffold — to refresh them: `rm ~/.config/usta/USTA.md ~/.config/usta/approaches/_default.md` + run `usta` once; or `usta reset --factory`.)

**Profile lifecycle:** `USER.md` is a living document — while Usta doesn't know you (generic profile) it briefly introduces itself at session start (name, learning style, 1-2 questions), and at session close it writes what it learned *about you* (not topic knowledge) into your profile. The more you use it, the better it knows you. `usta reset --profile` makes it forget in one command (old version in `USER.md.bak`) → next session it introduces itself again. Hand-edit the profile and Usta won't overwrite what you wrote.

## Status

v0.20 · Rust 2021 · 331 unit tests. Design decisions: [`SPEC.md`](SPEC.md). Core behavior: [`USTA.md`](USTA.md). Roadmap: [`docs/ROADMAP.md`](docs/ROADMAP.md).

Next up: prebuilt binaries + Homebrew tap (deliberately deferred until "everyone can use it" matters). Further ideas: streaming replies, multi-terminal hardening, self-health-check (links/consistency), tech-notes cache.
