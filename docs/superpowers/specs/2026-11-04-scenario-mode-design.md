# Scenario mode. Design

Date: 2026-11-04 · Status: draft, awaiting user review · Base: `main` @ v0.39.0 + `e5cf82a`

## Problem

Usta today is a mentor beside your own project. You bring the project, it reacts to file saves and assigns exercises. If you have no project idea, nothing starts.

Scenario mode reverses that: Usta brings the work. It generates scenarios that build a person toward professional level in a subject, tracks what they did, and continues from there.

## Goals

- Works for any subject: Linux, a programming language, SEO, marketing, an app like Etsy, trading. Nothing in the core is specific to one subject or one environment.
- Usta decides which environment a subject needs and builds it. A container is one option, not the model.
- Scenarios connect. What you did carries into what comes next.
- Finished skills are skipped; due skills come back as a different scenario.
- An unfinished task can be resumed. Idle work costs almost no resources.

## Non-goals

- A non-terminal UI. The core stays UI-independent so one can be added later, but the first user works in the terminal.
- Copying any existing game. Hacking games are a reference for feeling only.
- Investment, medical or legal advice (see Safety).

## Core model

A scenario has four parts: **role, situation, task, check**. The environment is separate and pluggable.

### Scenario package

A scenario is stored as a package in a fixed format, so it can be validated, reused and resumed.

| Field | Meaning |
|---|---|
| `id`, `topic` | Identity |
| `skills` | Curriculum map items it exercises |
| `environment` | Which environment runs it, plus that environment's setup |
| `task` | What the learner must achieve |
| `check` | How completion is decided, script or rubric |
| `reference_solution` | Used for validation only, never shown to the learner |
| `story` | Tone-independent facts the scenario adds to or needs from the world ledger |

On disk the package follows the Terminal-Bench / Harbor task shape (Apache-2.0), reduced for Usta: `instruction.md`, `environment/`, `solution/`, `tests/`, `task.toml`. The shape is copied; the Harbor runner is not used, since it is built to run agents, not human sessions. `skills` and `story` are Usta's additions.

### Generate, then validate

Usta generates packages; the system validates them before a learner sees them. A model-made scenario that cannot be solved makes the learner blame themselves, so this step is not optional.

For script checks, validation has two conditions:

1. On the fresh environment, the check fails.
2. After the reference solution is applied, the check passes.

A fresh environment is also the "do nothing" attempt, so condition 1 covers it. When a condition fails, the error output goes back to the model, for at most 3 rounds. A package that still fails is discarded and never shown to a learner.

A package that passes enters the local library and is reused, so generation cost is paid once. Rubric-checked scenarios cannot be validated this way; they are validated by confirming the rubric references only material present in the scenario.

## Environments

### The decision

At the start of a subject, Usta picks the environment from three questions:

1. In which environment can this skill be practised at all?
2. Does the person already have the tool or a real account?
3. How expensive is a mistake here?

The last question can override the second. Editing an Etsy listing is safe on the real shop. An ad budget decision burns real money, so it is practised in a simulated world first.

### The contract

The scenario engine never knows which environment it runs in. Every environment implements five operations:

| Operation | Meaning |
|---|---|
| `prepare` | Build the starting state |
| `pause` | Release resources, keep the work |
| `resume` | Return to the paused state |
| `check` | Decide whether the task is done |
| `clean` | Remove what is left after a finished task |

A new environment (browser, virtual machine, another app) is a new implementation. The engine does not change.

### The first environments

| Environment | Used for | `prepare` | `pause` / `resume` | `check` |
|---|---|---|---|---|
| Container | Linux, software, technical SEO | Start image, apply the fault | `docker stop` / `docker start` | Script |
| Folder (simulated world) | SEO strategy, marketing, trading | Write brief, data exports, competitor pages | Files stay; Usta's record | Script or rubric |
| Real account | Etsy, Excel, Figma | Learn the person's current state | Usta's record | Screenshot, exported file |
| Conversation only | Subjects needing no setup | Nothing | Usta's record | Usta judges |

**Own editor:** a container task that has the learner write code declares `environment.workspace`, a path inside the container. Usta creates a task folder on the host, puts the starting files there and mounts it at that path. The learner edits in their own editor; builds and the check run in the container. Generated scripts still never run on the host; files an editor itself runs once the folder is trusted (a `build.rs`, a `package.json` script) are covered by the editor's trust prompt, not by Usta. This replaces the earlier "own machine" environment, which would have run generated scripts on the host.

**Real account:** Usta cannot break this environment on purpose, so tasks are built from the person's actual state. Usta must learn that state first, and prebuilt packages are less useful here.

**Trading:** replay of real historical market data with date and symbol hidden, revealed one step at a time. Real money is never used. The data source and its licence are unverified.

### Tool setup

Tools live in the container, so the learner installs nothing but Docker. Without Docker, Usta asks the learner to install it and offers tasks that need no running system.

## No work for the learner

The learner never needs to know Docker. They never see or type a Docker command.

- Usta starts the container runtime if it is installed but not running.
- Usta opens a new terminal window already inside the task. If it cannot open one on that system, it falls back to showing the command.
- Learner-facing text speaks of the task or the server, never containers or images.
- An embedded terminal pane inside Usta is a later step. It would give one window and let Usta see the session.
- Installing the runtime is the one remaining manual step. Removing it is a later step; the environment contract allows other runtimes.

## Check scripts

Prior art shows two opposite failures: checks that reject valid solutions, and checks that can be bypassed.

- Check the system state, never the commands typed.
- Accept every valid solution, not only the reference one.
- Check scripts stay outside anything the learner can write to, and enter the environment only at check time.

## Container lifecycle

| State | When | Resources |
|---|---|---|
| Running | The person is on the task | Limited memory and CPU |
| Stopped | Left unfinished, Usta closed, or idle | No memory or CPU, a small amount of disk |
| Removed | Task finished and record written | None |

- Containers are not started with `--rm`, because that would delete unfinished work.
- One task, one container. A new task opens a new container. Continuity lives in Usta's records and the story, not in the container.
- If a later task needs an earlier state, its `prepare` rebuilds that state in a new container.
- Each container has `--memory`, `--cpus` and `--pids-limit` set.
- At most one container runs at a time. Other unfinished tasks wait stopped.
- Network is off by default. A scenario declares it when needed, for example to fetch crates.
- Disk: a generated task's image `usta-scn-gen-...:v<n>` is not removed when a run ends (a build is create + exec + commit with no layer cache, so reopening a kept task would redo slow installs). Each run start or build records or refreshes `<image> <date>` in `scenarios/bases.txt` beside the library, the same file and idle rule as pulled base images. The startup sweep removes a listed image unused for 14 days with a plain `docker rmi` (never `-f`; a refused removal keeps its line, the package stays and `prepare` rebuilds a missing image), only if the runtime is already running and only for accepted names: pulled bases, the folder check image, and `usta-scn-gen-` images with a `:v<number>` tag. A proof that fails still removes its own image. A base that was on the machine first is never listed, so never removed.
- Usta warns if the container runtime is older than the fixes for the November 2025 runc escapes (CVE-2025-31133, CVE-2025-52565, CVE-2025-52881).
- Containers carry a `usta-` name prefix and label. Usta touches nothing else.
- On startup Usta removes only leftovers of finished tasks and `usta-` containers with no record. It never removes an unfinished task without asking.
- A long-untouched unfinished task triggers a question to the person, never a silent delete.
- On a Linux host, files created inside the container stay owned by root, so the learner's editor cannot save those and moving them may need more rights. Tested on Docker Desktop for Mac only.
- A task with `environment.workspace` keeps its files in a host task folder mounted into the container. The person edits in their own editor and builds run in the container. A finished task's files move to the done folder, as in a folder task.
- The mounted folder is writable from inside, so a generated script can write there. Once, after `prepare` ran setup and start, Usta removes every hidden entry and every symlink from it, inside the container as root (a `find` through `docker exec`). The scrub never runs on the host, so a link swapped mid-walk cannot reach host files. The scrub is a one-time pass: anything generated that runs later (a process detached by setup or start, a `start` script at resume, a check) can still write there, and the learner's own hidden files must survive anyway. The editor's own trust prompt is the guard for all of that. This matches the rule that a container protects against mistakes, not malicious code.

**Resuming:** files survive a stop; running processes do not. On start, the scenario's services come back up and Usta summarises where the person left off.

**Safety limit:** a container protects against mistakes such as a wrong `rm -rf` or a broken config. It is not presented as a defence against malicious code.

## Continuity

The core is a following, continuing course of teaching, and it cannot be turned off. Usta carries three things between tasks:

1. **Skill state:** the existing curriculum map (`seen`, `settled`, `deepened`, `due:`).
2. **World ledger:** the company, characters, past events, open threads.
3. **Decisions:** what the person did. Skipping a backup in one task can have a cost three tasks later.

### Skip and repeat

Each scenario is tied to map items. If its items are already settled, it is skipped. When an item is due, the person gets a different scenario that needs the same skill, not the same one again, so it cannot be solved from memory.

## Tone

Tone is a layer over the core. Task, check and difficulty do not change with it.

Plain:

> nginx returns 502. Find the cause and fix it.

Adventure:

> 03:12. Deniz writes: "The site is down, the client is calling." The nginx you set up last week returns 502.

- Tone can be changed later, on the `/game on` model. Skill records are unaffected.
- Story speaks only at task open and close, never during the task. This follows the dose rule in `GAMIFICATION.md`.
- In the real account environment no fiction is needed: the story is the person's own shop growing.

## Measuring

- **Process is measured, not outcome.** A bad decision can win by luck. Trading checks rule-following: stop placed, position size within the rule, reason written before the trade. SEO and marketing scenarios measure decision quality in the same way. This matches `GAMIFICATION.md`: points live in the process.
- **No invented results.** A simulated world must not report made-up effects such as "traffic rose 20%". That teaches wrong things. Real historical data, as in trading replay, is allowed.
- Rubric checks rest on Usta's judgment and are less reliable than scripts. Published results on LLM grading are mixed, and weakest on ambiguous cases. So each task gets its own rubric, a single verdict is never treated as final, and the learner is told which kind of check was used.

## Safety

Usta gives no advice on real positions, never uses real money, and generates scenarios for finance, health and law from past or fictional data only. Any scenario with real-money risk runs in a simulated world.

## Generation on demand

Hand-written packages are not the product (the three examples were removed on 2026-11-05). A person can ask for any subject at any moment, and Usta produces the scenario then.

- `/scenario start` in any topic generates a scenario when no suitable package is left. No topic ever answers "no tasks".
- The model receives the topic, the curriculum map, the learner's level, the world ledger and the finished scenarios. It returns a package in the fixed format and picks the environment using the three questions above.
- Script-checked packages pass the validation above before the learner sees them. Rubric-checked packages pass the rubric check. A failure goes back to the model for at most 3 rounds, then Usta tells the learner plainly and starts a conversation-only scenario on the same skill.
- Passing packages enter a local library and are reused, so generation is paid for once.
- Without Docker, Usta picks an environment that needs none and says so once. A subject that truly needs a running system gets install guidance.

### Safety of generated content

Model-written content is untrusted.

- A generated script runs only inside a container, never on the host.
- Any subject may use a container when the task needs one. Usta decides per scenario, not per subject: a marketing task with a script check or a tool to run gets a container like a Linux task does.
- The folder environment writes data files only, inside the project's task folder. If its check needs a script, the script runs in a throwaway container with the task folder mounted read-only. Otherwise the check is a rubric.
- Package paths cannot leave the package. `lint` runs at load, for every package, and a failed package is reported, not silently skipped.
- Generated containers keep every limit in Container lifecycle. Network is off at run time unless declared.
- Building a generated image needs network, and a build step can reach the local network and host services. Usta limits what it can: an allowlist of official base images, no host mounts, no raw sockets, a time budget, and a disk check between steps. A single step is limited only by the time budget. This is accepted, since a container here protects against mistakes, not malicious code.

## Build order

This is the order of construction, not a limit on the product.

1. **Done (v0.40.0):** scenario engine, contract, container environment, three Linux scenarios in a plain and an adventure tone. The three built-in scenarios were removed on 2026-11-05: Usta generates every task.
2. **Done (v0.41.0), any subject, on demand:** generate and validate, the local library, and the folder and conversation-only environments.
3. **Own editor:** the mounted task folder for container tasks, tried on a programming language.

Parked by decision on 2026-11-05: the real account environment (Etsy runs in a simulated world) and trading replay.

Each step gets its own plan.

## Prior art

Scanned 2026-11-04; details in the headspace tool registry (`70_tools/`). No product found that combines subject independence, the person's own project or account, a skill map, spaced repetition and a continuing story. The scan does not claim to be exhaustive. Each piece exists separately.

| Piece | Who | Use |
|---|---|---|
| Broken Linux servers with checks | SadServers | Design reference; code is closed and unlicensed, and its issues are the source of the check rules above |
| Container task package with reference solution | Terminal-Bench / Harbor | Copy the shape |
| LLM-generated tasks with execution feedback | TerminalWorld, Endless Terminals (papers) | Validation loop idea; numbers read at summary level only |
| Trading replay | FX Replay, TradingView Bar Replay, Forex Tester | Commercial; hidden-date behaviour unverified |

## Open measurements

Unverified; each is measured in the step that needs it.

- Token cost of tone and story per scenario.
- Disk size of base images and of stopped containers.
- Resource cost of Docker Desktop itself on macOS.
- Whether a dev container gives `rust-analyzer` full function when Rust is not on the host.
- The idle time before a running container stops, and the age at which Usta asks about an unfinished task.

## Testing

- The contract is tested with a fake environment, so the engine needs no Docker in CI.
- Each environment has its own tests for all five operations. Container tests are separated from the default `cargo test` run.
- Validation is tested with a passing package, one whose check already passes on a fresh environment, and one whose reference solution does not pass.
- Lifecycle tests cover resume after stop, a crash leaving a container behind, and proof that unfinished tasks survive startup cleanup.
