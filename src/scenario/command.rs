//! `/scenario ...`: parsing, execution (blocking, run off the UI task via
//! `spawn_blocking`) and the synthesized mentor turns. The session's `Slot`
//! (which pauses this process's runs on exit) lives in `slot.rs`.
//!
//! Conventions copied from `/game`: slash lines never reach the model; notices
//! are shell-only; at most one synthesized `[SCENARIO ...]` turn per command
//! goes through the normal ask flow. Learner-facing text says task or server,
//! never container, image or Docker (install guidance and the no-window
//! fallback are the sanctioned exceptions, in `docker.rs` and `terminal.rs`).

use std::path::PathBuf;

use anyhow::{anyhow, bail, Result};

use crate::progress;
use crate::scenario::container::ContainerEnv;
use crate::scenario::docker::{
    available, ensure_ready, runc_warning, RealDocker, SystemStarter, START_POLL, START_TIMEOUT,
};
use crate::scenario::engine::{self, Tick};
use crate::scenario::env::{CheckOutcome, Internal};
use crate::scenario::ledger::{self, Ledger};
use crate::scenario::library;
use crate::scenario::package::{Package, Tone};
use crate::scenario::select::{self, Tag};
use crate::scenario::store::{project_tag, RunRecord, RunState, Store};
use crate::scenario::sweep;
use crate::scenario::terminal::{window_notice, SystemLauncher, WindowReq};

const USAGE: &str = "scenario commands: /scenario (list) · start [number|id] · resume [id] · check · pause · drop [id] · tone plain|adventure";

#[derive(Debug, PartialEq)]
pub(crate) enum ScenarioCmd {
    List,
    Start(Option<String>),
    Resume(Option<String>),
    Check,
    Pause,
    Drop { id: Option<String>, confirmed: bool },
    Tone(Option<String>),
    Usage,
}

/// `/scenario ...` only; lookalikes (`/scenarios`) and ordinary text return None.
pub(crate) fn parse(line: &str) -> Option<ScenarioCmd> {
    let lower = line.trim().to_ascii_lowercase();
    let rest = if lower == "/scenario" {
        ""
    } else {
        lower.strip_prefix("/scenario ")?
    };
    let mut it = rest.split_whitespace();
    Some(match it.next() {
        None | Some("list") => ScenarioCmd::List,
        Some("start") => ScenarioCmd::Start(it.next().map(String::from)),
        Some("resume") => ScenarioCmd::Resume(it.next().map(String::from)),
        Some("check") => ScenarioCmd::Check,
        Some("pause") => ScenarioCmd::Pause,
        Some("drop") => {
            let (mut id, mut confirmed) = (None, false);
            for a in it {
                if a == "confirm" {
                    confirmed = true;
                } else if id.is_none() {
                    id = Some(a.to_string());
                }
            }
            ScenarioCmd::Drop { id, confirmed }
        }
        Some("tone") => ScenarioCmd::Tone(it.next().map(String::from)),
        Some(_) => ScenarioCmd::Usage,
    })
}

/// Shown before a slow command (the screen does not redraw while it works).
pub(crate) fn busy_note(cmd: &ScenarioCmd) -> Option<&'static str> {
    match cmd {
        ScenarioCmd::Start(_) => Some("preparing your task… the first start can take a few minutes while its server is set up"),
        ScenarioCmd::Resume(_) => Some("resuming your task…"),
        ScenarioCmd::Check => Some("running the check…"),
        _ => None,
    }
}

#[derive(Debug, Default)]
pub(crate) struct Reply {
    pub notices: Vec<String>,
    /// Synthesized user turn for the normal ask flow (start, resume, pass).
    pub turn: Option<String>,
}

impl Reply {
    pub(crate) fn note(s: impl Into<String>) -> Reply {
        Reply {
            notices: vec![s.into()],
            turn: None,
        }
    }
}

#[derive(Clone)]
pub(crate) struct Ctx {
    pub global: PathBuf,
    pub project_root: PathBuf,
    pub topic: String,
    pub pid: u32,
}

pub(crate) fn render_list(
    topic: &str,
    pkgs: &[Package],
    map: &str,
    finished: &[String],
    threads: &[String],
    unfinished: &[RunRecord],
    tone: Tone,
) -> String {
    if pkgs.is_empty() {
        return format!("No tasks for '{topic}' yet.\nTone: {}", tone.as_str());
    }
    let mut out = format!("Tasks for '{topic}':\n");
    for (i, p) in pkgs.iter().enumerate() {
        let t = match select::tag(p, map, finished, threads) {
            Tag::New => "new",
            Tag::Done => "done",
            Tag::Skipped => "skipped, already settled",
            Tag::Later => "later in the story",
        };
        out.push_str(&format!("  {}. {} — {} [{t}]\n", i + 1, p.id, p.title));
    }
    if !unfinished.is_empty() {
        out.push_str("Unfinished:\n");
        for r in unfinished {
            out.push_str(&format!(
                "  {} (last check: {}) — /scenario resume {}\n",
                r.package,
                r.last_check.as_deref().unwrap_or("none yet"),
                r.run_id
            ));
        }
        out.push_str("Give one up with /scenario drop [id].\n");
    }
    out.push_str(&format!(
        "Start one with /scenario start [number|id]. Tone: {} (change: /scenario tone plain|adventure)",
        tone.as_str()
    ));
    out
}

// The three turn builders below are for the mentor (the model), not the
// learner, but they tell it to keep the infrastructure words out of its replies.

pub(crate) fn start_turn(pkg: &Package, instruction: &str, tone: Tone, world: &str) -> String {
    let world = if world.is_empty() {
        String::new()
    } else {
        format!("{world} ")
    };
    format!(
        "[SCENARIO START] The learner just started the task \"{}\" ({}). Skills it exercises: {}. \
         The shell already showed them the opening in {} tone; do not repeat or paraphrase it. \
         {world}Task: {instruction} \
         They work in a terminal on the task's server that you cannot see and will run /scenario check themselves; the shell reports the result. \
         Say task or server to the learner; never container, image or Docker. \
         Mentor Socratically as usual: never give the fix, ask what they see, hint by the ladder. \
         Reply with ONE short sentence that invites the first step.",
        pkg.title,
        pkg.id,
        pkg.skills.join(", "),
        tone.as_str()
    )
}

pub(crate) fn resume_turn(pkg: &Package, instruction: &str, last_check: Option<&str>) -> String {
    format!(
        "[SCENARIO RESUME] The learner came back to the unfinished task \"{}\" ({}). Task: {instruction} \
         Their last check: {}. Their work on the server survived; its services were restarted. \
         Say task or server to the learner; never container, image or Docker. \
         In two sentences, say where they left off if this conversation shows it, otherwise restate the task. \
         Do not give the fix.",
        pkg.title,
        pkg.id,
        last_check.unwrap_or("none yet")
    )
}

pub(crate) fn done_turn(pkg: &Package) -> String {
    format!(
        "[SCENARIO DONE] The check passed for \"{}\" ({}). Skills exercised: {}. The shell already showed the closing. \
         Treat this as evidence for these skills and reflect it in the curriculum map at the closing flush under the usual evidence rules. \
         Reply with one sentence of acknowledgement, no recap.",
        pkg.title,
        pkg.id,
        pkg.skills.join(", ")
    )
}

fn log_error(ctx: &Ctx, text: &str) {
    let p = ctx.project_root.join(".usta/scenarios/last-error.log");
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let _ = std::fs::write(p, format!("{text}\n"));
}

/// Internal errors (tool output) go to the log; the learner reads one plain sentence.
fn failure(ctx: &Ctx, e: anyhow::Error) -> Reply {
    if e.downcast_ref::<Internal>().is_some() {
        log_error(ctx, &format!("{e:#}"));
        Reply::note("scenario: something went wrong with the task's server; details are in .usta/scenarios/last-error.log")
    } else {
        Reply::note(format!("scenario: {e}"))
    }
}

pub(crate) fn execute(ctx: &Ctx, cmd: ScenarioCmd) -> Reply {
    match run(ctx, cmd) {
        Ok(r) => r,
        Err(e) => failure(ctx, e),
    }
}

/// `clean_warning` can carry raw tool text: it goes to the log, never to the learner.
fn clean_notice(ctx: &Ctx, raw: &str) -> String {
    log_error(ctx, raw);
    "the task's server could not be removed yet; Usta will try again at the next start".into()
}

/// A paused task whose server no longer exists cannot be woken. Say so in one
/// plain sentence instead of letting the raw runtime error through.
fn server_gone_error(env: &ContainerEnv, rec: &RunRecord) -> Option<anyhow::Error> {
    let gone = rec.reference.is_empty() || env.is_gone(&rec.reference);
    gone.then(|| {
        anyhow!(
            "the server for this task is gone, so it cannot be resumed; give it up with /scenario drop {} confirm and start it again",
            rec.run_id
        )
    })
}

fn finished_ids(store: &Store) -> Vec<String> {
    store
        .all()
        .into_iter()
        .filter(|r| r.state == RunState::Finished)
        .map(|r| r.package)
        .collect()
}

fn map_text(ctx: &Ctx) -> String {
    std::fs::read_to_string(progress::curriculum_path(&ctx.project_root, &ctx.topic))
        .unwrap_or_default()
}

/// Apply finished runs the ledger has not seen (repairs a crash between the
/// finished record and the ledger write). Never fatal.
fn sync_ledger(ctx: &Ctx, store: &Store, pkgs: &[Package], today: &str) {
    let _ = ledger::reconcile(&ctx.project_root, &ctx.topic, &store.all(), pkgs, today);
}

fn run(ctx: &Ctx, cmd: ScenarioCmd) -> Result<Reply> {
    let store = Store::new(&ctx.project_root);
    let today = crate::lifecycle::today();
    match &cmd {
        ScenarioCmd::Usage => return Ok(Reply::note(USAGE)),
        ScenarioCmd::Tone(arg) => {
            let Some(t) = arg.as_deref().and_then(Tone::parse) else {
                return Ok(Reply::note(format!(
                    "tone is {}; choose plain or adventure: /scenario tone adventure",
                    store.tone().as_str()
                )));
            };
            store.set_tone(t)?;
            return Ok(Reply::note(format!(
                "tone set to {}; it applies from the next task",
                t.as_str()
            )));
        }
        ScenarioCmd::List => {
            library::sync(&ctx.global)?;
            let pkgs = library::load_all(&ctx.global, &ctx.topic)?;
            sync_ledger(ctx, &store, &pkgs, &today);
            let map = map_text(ctx);
            let threads = select::threads_available(
                &pkgs,
                &map,
                &Ledger::load(&ctx.project_root, &ctx.topic).open_ids(),
            );
            let unfinished: Vec<RunRecord> = store
                .all()
                .into_iter()
                .filter(|r| {
                    r.topic == ctx.topic && matches!(r.state, RunState::Paused | RunState::Active)
                })
                .collect();
            return Ok(Reply::note(render_list(
                &ctx.topic,
                &pkgs,
                &map,
                &finished_ids(&store),
                &threads,
                &unfinished,
                store.tone(),
            )));
        }
        _ => {}
    }

    let docker = RealDocker;
    match &cmd {
        ScenarioCmd::Start(_) | ScenarioCmd::Resume(_) => {
            ensure_ready(&docker, &SystemStarter, START_TIMEOUT, START_POLL, &|d| {
                std::thread::sleep(d)
            })?;
            // Never prompts or reads stdin, so it is safe inside the session UI.
            sweep::sweep_docker(&ctx.project_root, &today);
        }
        ScenarioCmd::Check | ScenarioCmd::Pause => available(&docker)?,
        // Drop works without the runtime: a clean that cannot run is retried at the next start.
        _ => {}
    }
    library::sync(&ctx.global)?;
    let mut env = ContainerEnv {
        docker: &docker,
        project_tag: project_tag(&ctx.project_root),
    };
    let stamp = chrono::Local::now().format("%Y%m%d%H%M%S").to_string();
    let alive: &dyn Fn(u32) -> bool = &crate::lock::pid_alive;
    let tick = Tick {
        pid: ctx.pid,
        alive,
        stamp: &stamp,
        today: &today,
    };
    let pkgs = library::load_all(&ctx.global, &ctx.topic)?;
    let find_pkg = |id: &str| -> Result<&Package> {
        pkgs.iter()
            .find(|p| p.id == id)
            .ok_or_else(|| anyhow!("task '{id}' is not in the library"))
    };
    let scratch = ctx.project_root.join(".usta/scenarios");

    match cmd {
        ScenarioCmd::Start(sel) => {
            sync_ledger(ctx, &store, &pkgs, &today);
            let ledger = Ledger::load(&ctx.project_root, &ctx.topic);
            let finished = finished_ids(&store);
            let pkg = select::resolve(
                &pkgs,
                sel.as_deref(),
                &map_text(ctx),
                &finished,
                &ledger.open_ids(),
            )
            .map_err(|m| anyhow!(m))?;
            let tone = store.tone();
            let mut notices = Vec::new();
            if let Some(w) = runc_warning(&docker) {
                notices.push(w);
            }
            let rec = engine::start(&mut env, &store, pkg, tone, &tick)?;
            notices.push(pkg.tone_text(tone, "open")?);
            let req = WindowReq {
                command_line: &rec.access,
                run_id: &rec.run_id,
                scratch: &scratch,
            };
            notices.push(window_notice(&SystemLauncher, &req));
            notices.push("When you think it is solved: /scenario check".into());
            Ok(Reply {
                notices,
                turn: Some(start_turn(
                    pkg,
                    &pkg.instruction()?,
                    tone,
                    &ledger.summary(),
                )),
            })
        }
        ScenarioCmd::Resume(sel) => {
            let mut rec = pick_resumable(&store, sel.as_deref(), &ctx.topic, ctx.pid)?;
            let pkg = find_pkg(&rec.package)?;
            if let Some(e) = server_gone_error(&env, &rec) {
                return Err(e);
            }
            // A closed window leaves an own active run with a healthy server: only reopen it.
            let woke = !already_running(&env, &rec);
            if woke {
                engine::resume(&mut env, &store, pkg, &mut rec, &tick)?;
            }
            let req = WindowReq {
                command_line: &rec.access,
                run_id: &rec.run_id,
                scratch: &scratch,
            };
            let notices = vec![
                resume_notice(&pkg.title, woke),
                window_notice(&SystemLauncher, &req),
            ];
            // The mentor turn says the services restarted: only true when we woke them.
            let turn = if woke {
                Some(resume_turn(
                    pkg,
                    &pkg.instruction()?,
                    rec.last_check.as_deref(),
                ))
            } else {
                None
            };
            Ok(Reply { notices, turn })
        }
        ScenarioCmd::Check => {
            let mut rec = own_active(&store, ctx.pid)?;
            let pkg = find_pkg(&rec.package)?;
            let report = engine::check(&mut env, &store, pkg, &mut rec, &tick)?;
            match report.outcome {
                CheckOutcome::Pass => {
                    let tone = Tone::parse(&rec.tone).unwrap_or(Tone::Plain);
                    let mut notices = vec![pkg.tone_text(tone, "close")?, "check: script (it looked at the system state, not at the commands you typed)".into()];
                    let mut ledger = Ledger::load(&ctx.project_root, &ctx.topic);
                    if ledger.apply_finished(&pkg.story, &rec.run_id, &today)
                        && ledger.save(&ctx.project_root, &ctx.topic).is_err()
                    {
                        notices.push("the story notes could not be saved this time; they are added at the next start".into());
                    }
                    if let Some(w) = report.clean_warning {
                        notices.push(clean_notice(ctx, &w));
                    }
                    Ok(Reply {
                        notices,
                        turn: Some(done_turn(pkg)),
                    })
                }
                CheckOutcome::Fail { output } => {
                    Ok(Reply::note(format!("Not yet. check: script\n{output}")))
                }
            }
        }
        ScenarioCmd::Pause => {
            let mut rec = own_active(&store, ctx.pid)?;
            engine::pause(&mut env, &store, &mut rec, &tick)?;
            Ok(Reply::note(format!(
                "Paused. Continue later with /scenario resume {}",
                rec.run_id
            )))
        }
        ScenarioCmd::Drop { id, confirmed } => {
            let mut rec = pick_unfinished(&store, id.as_deref(), &ctx.topic, ctx.pid)?;
            let title = pkgs
                .iter()
                .find(|p| p.id == rec.package)
                .map(|p| p.title.clone())
                .unwrap_or_else(|| rec.package.clone());
            if !confirmed {
                return Ok(Reply::note(format!(
                    "This removes your work on '{title}' for good, and nothing from it counts toward your progress. To go ahead: /scenario drop {} confirm",
                    rec.run_id
                )));
            }
            let mut notices = vec![format!("Dropped '{title}'. Its server was removed.")];
            if let Some(w) = engine::drop_run(&mut env, &store, &mut rec, &tick)? {
                log_error(ctx, &w);
                notices = vec![format!("Dropped '{title}'. Its server could not be removed right now; Usta will try again at the next start.")];
            }
            Ok(Reply {
                notices,
                turn: None,
            })
        }
        ScenarioCmd::Usage | ScenarioCmd::Tone(_) | ScenarioCmd::List => {
            unreachable!("handled above")
        }
    }
}

fn own_active(store: &Store, pid: u32) -> Result<RunRecord> {
    store
        .all()
        .into_iter()
        .find(|r| r.state == RunState::Active && r.owner_pid == pid)
        .ok_or_else(|| anyhow!("no active task; use /scenario start or /scenario resume"))
}

/// An own active run whose server is still running needs no wake-up.
fn already_running(env: &ContainerEnv, rec: &RunRecord) -> bool {
    rec.state == RunState::Active && env.is_running(&rec.reference)
}

fn resume_notice(title: &str, woke: bool) -> String {
    if woke {
        format!("Resumed: {title}. Your files are as you left them; its programs were restarted.")
    } else {
        format!("Back in: {title}. Your task is still running; opened a new terminal window.")
    }
}

/// Resumable = paused, or active and owned by this process (its server may have
/// stopped mid-session; `engine::resume` handles both).
fn pick_resumable(store: &Store, sel: Option<&str>, topic: &str, pid: u32) -> Result<RunRecord> {
    let paused: Vec<RunRecord> = store
        .all()
        .into_iter()
        .filter(|r| {
            r.topic == topic
                && (r.state == RunState::Paused
                    || (r.state == RunState::Active && r.owner_pid == pid))
        })
        .collect();
    Ok(match sel {
        Some(id) => paused
            .into_iter()
            .find(|r| r.run_id == id)
            .ok_or_else(|| anyhow!("no task '{id}' to resume; see /scenario"))?,
        None => paused
            .iter()
            .rfind(|r| r.state == RunState::Paused)
            .or(paused.last())
            .cloned()
            .ok_or_else(|| anyhow!("nothing to resume; see /scenario"))?,
    })
}

/// Unfinished = paused, or active and owned by this process. Never finished, dropped, another topic's or another Usta's.
pub(crate) fn pick_unfinished(
    store: &Store,
    sel: Option<&str>,
    topic: &str,
    pid: u32,
) -> Result<RunRecord> {
    let open: Vec<RunRecord> = store
        .all()
        .into_iter()
        .filter(|r| {
            r.topic == topic
                && (r.state == RunState::Paused
                    || (r.state == RunState::Active && r.owner_pid == pid))
        })
        .collect();
    match sel {
        Some(id) => open
            .into_iter()
            .find(|r| r.run_id == id)
            .ok_or_else(|| anyhow!("no unfinished task '{id}'; see /scenario")),
        None => match open.len() {
            0 => bail!("no unfinished task; see /scenario"),
            1 => Ok(open.into_iter().next().expect("one element")),
            _ => bail!(
                "several unfinished tasks; name one: {}",
                open.iter()
                    .map(|r| format!("/scenario drop {}", r.run_id))
                    .collect::<Vec<_>>()
                    .join(" · ")
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::env::internal;
    use crate::scenario::package::tests::{tmp, write_pkg};
    use crate::scenario::store::{RunRecord, RunState};

    /// `name` must be unique per test: `tmp` keys by process id and name.
    fn pkg(name: &str) -> Package {
        let root = tmp(name);
        let p = Package::load(&write_pkg(&root)).unwrap();
        let _ = std::fs::remove_dir_all(root);
        p
    }

    fn rec(id: &str, state: RunState, owner: u32) -> RunRecord {
        RunRecord {
            run_id: id.into(),
            package: "linux-b".into(),
            topic: "linux".into(),
            tone: "plain".into(),
            state,
            owner_pid: owner,
            started: "2026-11-04".into(),
            updated: "2026-11-04".into(),
            reference: "r".into(),
            access: "a".into(),
            last_check: Some("fail".into()),
        }
    }

    fn bad_words(s: &str) -> bool {
        let l = s.to_lowercase();
        l.contains("container") || l.contains("docker") || l.contains("image")
    }

    #[test]
    fn parse_variants() {
        assert_eq!(parse("/scenario"), Some(ScenarioCmd::List));
        assert_eq!(parse("  /SCENARIO  "), Some(ScenarioCmd::List));
        assert_eq!(parse("/scenario list"), Some(ScenarioCmd::List));
        assert_eq!(parse("/scenario start"), Some(ScenarioCmd::Start(None)));
        assert_eq!(
            parse("/scenario start 2"),
            Some(ScenarioCmd::Start(Some("2".into())))
        );
        assert_eq!(
            parse("/scenario resume linux-x"),
            Some(ScenarioCmd::Resume(Some("linux-x".into())))
        );
        assert_eq!(parse("/scenario check"), Some(ScenarioCmd::Check));
        assert_eq!(parse("/scenario pause"), Some(ScenarioCmd::Pause));
        assert_eq!(
            parse("/scenario tone adventure"),
            Some(ScenarioCmd::Tone(Some("adventure".into())))
        );
        assert_eq!(parse("/scenario tone"), Some(ScenarioCmd::Tone(None)));
        assert_eq!(parse("/scenario wat"), Some(ScenarioCmd::Usage));
    }

    #[test]
    fn parse_drop_with_and_without_id_and_confirmation() {
        assert_eq!(
            parse("/scenario drop"),
            Some(ScenarioCmd::Drop {
                id: None,
                confirmed: false
            })
        );
        assert_eq!(
            parse("/scenario drop linux-x"),
            Some(ScenarioCmd::Drop {
                id: Some("linux-x".into()),
                confirmed: false
            })
        );
        assert_eq!(
            parse("/scenario drop linux-x confirm"),
            Some(ScenarioCmd::Drop {
                id: Some("linux-x".into()),
                confirmed: true
            })
        );
        assert_eq!(
            parse("/scenario drop confirm"),
            Some(ScenarioCmd::Drop {
                id: None,
                confirmed: true
            })
        );
    }

    #[test]
    fn parse_ignores_lookalikes_and_ordinary_text() {
        assert_eq!(parse("/scenarios"), None);
        assert_eq!(parse("scenario start"), None);
        assert_eq!(parse("tell me about /scenario"), None);
        assert_eq!(parse("/game on"), None);
    }

    #[test]
    fn only_slow_commands_get_a_busy_note() {
        assert!(busy_note(&ScenarioCmd::Start(None))
            .unwrap()
            .contains("first start"));
        assert!(busy_note(&ScenarioCmd::Resume(None)).is_some());
        assert!(busy_note(&ScenarioCmd::Check).is_some());
        assert!(busy_note(&ScenarioCmd::List).is_none());
        assert!(busy_note(&ScenarioCmd::Tone(None)).is_none());
        assert!(busy_note(&ScenarioCmd::Drop {
            id: None,
            confirmed: true
        })
        .is_none());
    }

    #[test]
    fn list_shows_tags_unfinished_runs_drop_hint_and_tone() {
        let mut a = pkg("cmd_pkg_list_a");
        a.id = "linux-a".into();
        a.title = "A title".into();
        let mut b = pkg("cmd_pkg_list_b");
        b.id = "linux-b".into();
        b.title = "B title".into();
        let mut c = pkg("cmd_pkg_list_c");
        c.id = "linux-c".into();
        c.title = "C title".into();
        c.story.needs = vec!["not-open".into()];
        let paused = rec("linux-b-2026", RunState::Paused, 0);
        let out = render_list(
            "linux",
            &[a, b, c],
            "",
            &["linux-b".to_string()],
            &[],
            &[paused],
            Tone::Plain,
        );
        assert!(out.contains("1. linux-a"), "{out}");
        assert!(
            out.contains("[new]") && out.contains("[done]") && out.contains("[later in the story]"),
            "{out}"
        );
        assert!(out.contains("/scenario resume linux-b-2026"), "{out}");
        assert!(out.contains("last check: fail"), "{out}");
        assert!(out.contains("/scenario drop"), "{out}");
        assert!(out.contains("Tone: plain"), "{out}");
    }

    #[test]
    fn list_without_packages_says_so() {
        let out = render_list("rust", &[], "", &[], &[], &[], Tone::Plain);
        assert!(out.contains("No tasks for 'rust' yet"), "{out}");
    }

    #[test]
    fn turns_carry_task_facts_and_the_world_but_never_the_solution() {
        let root = tmp("cmd_turn");
        let p = Package::load(&write_pkg(&root)).unwrap();
        std::fs::write(p.dir.join("solution/solve.sh"), "SECRET-FIX-LINE\n").unwrap();
        let s = start_turn(
            &p,
            "Fix it.",
            Tone::Adventure,
            "World so far (Shopwell): people: Deniz.",
        );
        assert!(s.starts_with("[SCENARIO START]"));
        assert!(s.contains("demo-pkg") && s.contains("Fix it.") && s.contains("adventure"));
        assert!(s.contains("World so far (Shopwell)"));
        assert!(s.contains("task or server") && !s.to_lowercase().contains("container terminal"));
        assert!(!s.contains("SECRET-FIX-LINE"));
        assert!(!start_turn(&p, "Fix it.", Tone::Plain, "").contains("World so far"));
        let r = resume_turn(&p, "Fix it.", Some("fail"));
        assert!(r.starts_with("[SCENARIO RESUME]") && r.contains("Their last check: fail"));
        let d = done_turn(&p);
        assert!(d.starts_with("[SCENARIO DONE]") && d.contains("permission"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn learner_text_avoids_infrastructure_words() {
        for c in [
            ScenarioCmd::Start(None),
            ScenarioCmd::Resume(None),
            ScenarioCmd::Check,
        ] {
            assert!(!bad_words(busy_note(&c).unwrap()));
        }
        assert!(!bad_words(USAGE));
        let mut a = pkg("cmd_pkg_words");
        a.id = "linux-a".into();
        let out = render_list(
            "linux",
            &[a],
            "",
            &[],
            &[],
            &[rec("r1", RunState::Paused, 0)],
            Tone::Plain,
        );
        assert!(!bad_words(&out), "{out}");
    }

    #[test]
    fn internal_errors_are_logged_and_shown_as_one_plain_sentence() {
        let g = tmp("cmd_fail_g");
        let p = tmp("cmd_fail_p");
        let ctx = Ctx {
            global: g.clone(),
            project_root: p.clone(),
            topic: "linux".into(),
            pid: 1,
        };
        let r = failure(
            &ctx,
            internal("docker rm failed: Error response from daemon: boom"),
        );
        let shown = r.notices.concat();
        assert!(
            shown.contains("last-error.log") && !bad_words(&shown) && !shown.contains("boom"),
            "{shown}"
        );
        let log = std::fs::read_to_string(p.join(".usta/scenarios/last-error.log")).unwrap();
        assert!(log.contains("boom"));
        let plain = failure(&ctx, anyhow::anyhow!("no such task"));
        assert_eq!(plain.notices, vec!["scenario: no such task".to_string()]);
        let _ = std::fs::remove_dir_all(g);
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn pick_unfinished_takes_the_only_one_by_default_and_names_choices_otherwise() {
        let p = tmp("cmd_pick");
        let store = Store::new(&p);
        store.save(&rec("a-1", RunState::Paused, 0)).unwrap();
        assert_eq!(
            pick_unfinished(&store, None, "linux", 7).unwrap().run_id,
            "a-1"
        );
        store.save(&rec("a-2", RunState::Active, 7)).unwrap();
        let e = pick_unfinished(&store, None, "linux", 7)
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("/scenario drop a-1") && e.contains("/scenario drop a-2"),
            "{e}"
        );
        assert_eq!(
            pick_unfinished(&store, Some("a-2"), "linux", 7)
                .unwrap()
                .run_id,
            "a-2"
        );
        assert!(pick_unfinished(&store, Some("nope"), "linux", 7).is_err());
        // finished, dropped, other-topic and other-owner runs are never offered
        store.save(&rec("a-3", RunState::Finished, 0)).unwrap();
        store.save(&rec("a-4", RunState::Dropped, 0)).unwrap();
        store.save(&rec("a-5", RunState::Active, 99)).unwrap();
        assert!(pick_unfinished(&store, Some("a-3"), "linux", 7).is_err());
        assert!(pick_unfinished(&store, Some("a-5"), "linux", 7).is_err());
        assert!(pick_unfinished(&store, Some("a-1"), "rust", 7).is_err());
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn execute_handles_tone_usage_and_dropping_nothing_without_the_runtime() {
        let g = tmp("cmd_exec_g");
        let p = tmp("cmd_exec_p");
        let ctx = Ctx {
            global: g.clone(),
            project_root: p.clone(),
            topic: "rust".into(),
            pid: 1,
        };
        let t = execute(&ctx, ScenarioCmd::Tone(Some("adventure".into())));
        assert!(t.notices.concat().contains("adventure"));
        let bad = execute(&ctx, ScenarioCmd::Tone(Some("loud".into())));
        assert!(bad.notices.concat().contains("plain or adventure"));
        assert!(execute(&ctx, ScenarioCmd::Usage)
            .notices
            .concat()
            .contains("drop"));
        let d = execute(
            &ctx,
            ScenarioCmd::Drop {
                id: None,
                confirmed: false,
            },
        );
        assert!(
            d.notices.concat().contains("no unfinished task"),
            "{:?}",
            d.notices
        );
        assert!(d.turn.is_none());
        let _ = std::fs::remove_dir_all(g);
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn a_paused_task_whose_server_vanished_gets_one_plain_sentence() {
        use crate::scenario::docker::fake::{fail, ok, FakeDocker};
        let mut r = rec("lin-1", RunState::Paused, 0);
        let gone = FakeDocker::new(|_| fail(1, "Error: No such object: usta-x-lin-1"));
        let e = server_gone_error(
            &ContainerEnv {
                docker: &gone,
                project_tag: "x".into(),
            },
            &r,
        )
        .expect("gone")
        .to_string();
        assert!(
            e.contains("/scenario drop lin-1") && !bad_words(&e) && !e.contains("No such"),
            "{e}"
        );
        assert!(e.starts_with("the server for this task is gone"), "{e}");
        let there = FakeDocker::new(|_| ok("true"));
        assert!(server_gone_error(
            &ContainerEnv {
                docker: &there,
                project_tag: "x".into()
            },
            &r
        )
        .is_none());
        r.reference.clear();
        assert!(server_gone_error(
            &ContainerEnv {
                docker: &there,
                project_tag: "x".into()
            },
            &r
        )
        .is_some());
    }

    #[test]
    fn an_unreachable_runtime_is_not_mistaken_for_a_vanished_server() {
        use crate::scenario::docker::fake::{fail, FakeDocker};
        let mut r = rec("lin-1", RunState::Paused, 0);
        r.reference = "usta-x-lin-1".into();
        let d = FakeDocker::new(|_| fail(1, "Cannot connect to the Docker daemon"));
        let env = ContainerEnv {
            docker: &d,
            project_tag: "x".into(),
        };
        assert!(server_gone_error(&env, &r).is_none());
    }

    #[test]
    fn resume_also_picks_an_active_run_this_process_owns() {
        let p = tmp("cmd_pick_resume");
        let store = Store::new(&p);
        store.save(&rec("a-1", RunState::Active, 7)).unwrap();
        assert_eq!(
            pick_resumable(&store, None, "linux", 7).unwrap().run_id,
            "a-1"
        );
        assert!(pick_resumable(&store, Some("a-1"), "linux", 8).is_err());
        store.save(&rec("a-2", RunState::Active, 99)).unwrap();
        assert!(pick_resumable(&store, Some("a-2"), "linux", 7).is_err());
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn check_failure_text_stays_out_of_the_learner_reply() {
        let g = tmp("cmd_warn_g");
        let p = tmp("cmd_warn_p");
        let ctx = Ctx {
            global: g.clone(),
            project_root: p.clone(),
            topic: "linux".into(),
            pid: 1,
        };
        let n = clean_notice(&ctx, "docker rm: Error response from daemon: RAW-TEXT");
        assert!(!n.contains("RAW-TEXT") && !bad_words(&n), "{n}");
        let log = std::fs::read_to_string(p.join(".usta/scenarios/last-error.log")).unwrap();
        assert!(log.contains("RAW-TEXT"));
        let _ = std::fs::remove_dir_all(g);
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn bare_resume_prefers_the_paused_run_over_an_own_active_one() {
        let p = tmp("cmd_pick_pref");
        let store = Store::new(&p);
        store.save(&rec("p-1", RunState::Paused, 0)).unwrap();
        store.save(&rec("z-2", RunState::Active, 7)).unwrap();
        assert_eq!(
            pick_resumable(&store, None, "linux", 7).unwrap().run_id,
            "p-1"
        );
        assert_eq!(
            pick_resumable(&store, Some("z-2"), "linux", 7)
                .unwrap()
                .run_id,
            "z-2"
        );
        let _ = std::fs::remove_dir_all(p);
    }

    #[test]
    fn an_active_run_with_a_running_server_is_not_woken_again() {
        use crate::scenario::docker::fake::{ok, FakeDocker};
        let mut r = rec("a-1", RunState::Active, 7);
        r.reference = "usta-x-a-1".into();
        let on = FakeDocker::new(|_| ok("true"));
        let off = FakeDocker::new(|_| ok("false"));
        let env = |d| ContainerEnv {
            docker: d,
            project_tag: "x".into(),
        };
        assert!(already_running(&env(&on), &r));
        assert!(!already_running(&env(&off), &r));
        r.state = RunState::Paused;
        assert!(!already_running(&env(&on), &r));
        assert!(!resume_notice("T", false).contains("restarted"));
        assert!(resume_notice("T", true).contains("restarted"));
    }
}
