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
| 🎓 **Pedagogy layer** | Opening drill (recall), explain-mode (Feynman), hint ladder (fading), prediction protocol (it won't tell you the `cargo check` result — it makes you predict first). |
| 🔎 **Research** | Searches the web for what it doesn't know (WebSearch) — no making things up. |
| 🌍 **Any topic** | Not Rust-specific. For a new topic (Linux security, GTM, anything) Usta **derives the approach through an introduction** and builds a web-researched **curriculum map** (`not seen → seen → settled → deepened`). Scope guarding: nothing stays hanging in the air. |
| 🎨 **Terminal UI** | Claude Code-style inline TUI (ratatui): two-column welcome box on open (learning status + what's next), live four-sided input box, sticky status line (spinner + context gauge). The flow stays in normal scrollback — scroll up, copy. Auto plain mode on pipe/`NO_COLOR` (scripts don't break). |
| 🗂️ **Management** | `usta topics` shows what you're learning where; `reset` clears a topic or everything. |

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

## Usage

```bash
usta                    # start — asks for the topic (shortest path, use this)
usta start rust         # give the topic upfront — a topic argument requires 'start'
usta topics             # what am I learning where? (catalog)
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
  → you work (code / plan.md / whatever), you save
  → Usta gives proactive Socratic feedback (without doing the work)
  → on a cargo check error: it won't tell you — "where does this blow up?" — predict first
  → /quit → progress + approach + curriculum updated, catalog refreshed
```

### Project context: the `mentor/` folder

Every project gets a visible `mentor/` folder next to `.usta/`:

- **`mentor/PROJECT.md`** — what you're building, why, at what scale, with which stack, and what's out of scope. Usta writes it after the first introduction — or **you write it yourself before the first session** and Usta starts already knowing the plan, without re-asking project basics.
- **`mentor/PROGRESS.md`** — the *project's* state (done / in progress / next) plus an append-only decision log ("why we chose X"). Updated at every session close. Separate from *your* learning progress, which lives in `.usta/learner/`.

Both are plain markdown, meant to be read and hand-edited — Usta never overwrites your edits, and `reset` never touches them.

**Proactive start:** in a project with a filled `PROJECT.md` and no topics yet, press **Enter on the empty topic prompt** — Usta reads the plan and proposes where to start (topic + rationale + a first step small enough to start today). Confirm and go. Typing a topic still works, of course — the suggestion is for the "I don't know how to begin" moment, and it derives from your plan rather than being biased by whatever you'd have typed.

## Interface

In an interactive terminal Usta opens a **ratatui inline-viewport TUI**: a live input box + status line at the bottom, while the permanent flow (Usta's replies, file feedback) is printed to normal **scrollback** — scroll up to read or copy history. No alternate screen; your terminal history is preserved.

A bare `usta` (Claude Code-style): the **identity welcome** box appears on top (logo + your saved topics), and the input box below asks for the topic — type a word, or describe it in a sentence (the model reduces sentences to a short slug). With a filled `PROJECT.md` you can also just press Enter and let Usta suggest the start. `usta start <topic>` shows the **full-mode welcome** (learning status: level, map %, what's next) and starts directly.

No TTY or `NO_COLOR=1` → the TUI never opens — it falls back to the plain line mode (pipe/CI/script safe).

## How it works — "thin shell, thick brain"

Rust is only the shell: CLI, LLM client, file watcher (`notify`), `cargo check` runner, markdown loader. **The intelligence and personality live in markdown:**

```
~/.config/usta/          # GLOBAL brain (set up once, shared across all projects)
  USTA.md                #   core behavior + pedagogy rules
  USER.md                #   who you are (your learning style) — living document, see below
  learner/index.md       #   ## Records — topic | project | date catalog
  approaches/            #   software.md, _default.md — approach per domain

<project>/mentor/        # PROJECT docs (visible, user-facing — yours to edit)
  PROJECT.md             #   what you're building, why, scale, stack, non-goals
  PROGRESS.md            #   project state (done/doing/next) + append-only decision log

<project>/.usta/         # PROJECT internals (per project)
  learner/progress/<topic>.md      #   level, gaps, recall questions, error log
  learner/curriculum/<topic>.md    #   web-researched curriculum map (status-tagged)
  approaches/<topic>.md            #   Usta's derived topic-specific approach (living document)
```

Changing behavior = edit markdown, don't touch Rust. (Global behavior files aren't overwritten by the scaffold — to refresh them: `rm ~/.config/usta/USTA.md ~/.config/usta/approaches/_default.md` + run `usta` once; or `usta reset --factory`.)

**Profile lifecycle:** `USER.md` is a living document — while Usta doesn't know you (generic profile) it briefly introduces itself at session start (name, learning style, 1-2 questions), and at session close it writes what it learned *about you* (not topic knowledge) into your profile. The more you use it, the better it knows you. `usta reset --profile` makes it forget in one command (old version in `USER.md.bak`) → next session it introduces itself again. Hand-edit the profile and Usta won't overwrite what you wrote.

## Status

Rust 2021 · 225 unit tests. Design decisions: [`SPEC.md`](SPEC.md). Core behavior: [`USTA.md`](USTA.md).

Roadmap ideas: streaming, multi-terminal hardening, self-health-check (links/consistency), tech-notes cache.
