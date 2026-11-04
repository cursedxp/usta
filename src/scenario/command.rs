//! `/scenario ...`: parsing and execution (blocking, run off the UI task via
//! `spawn_blocking`). The synthesized mentor turns live in `turns.rs`, the
//! session's `Slot` (which pauses this process's runs on exit) in `slot.rs`.
//!
//! Conventions copied from `/game`: slash lines never reach the model; notices
//! are shell-only; at most one synthesized `[SCENARIO ...]` turn per command
//! goes through the normal ask flow. Learner-facing text says task or server,
//! never container, image or Docker (install guidance and the no-window
//! fallback are the sanctioned exceptions, in `docker.rs` and `terminal.rs`).

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{anyhow, Result};

use crate::scenario::docker::{Docker, RealDocker, RuntimeStarter, SystemStarter};
use crate::scenario::engine::{self, Tick};
use crate::scenario::env::{CheckOutcome, Internal, CONTAINER, FOLDER};
use crate::scenario::folder::{DONE_DIR, TASKS_DIR};
use crate::scenario::ledger::Ledger;
use crate::scenario::library;
use crate::scenario::model::Live;
use crate::scenario::model::ModelFailed;
use crate::scenario::package::{Package, Tone};
use crate::scenario::pick::{own_active, pick_resumable, pick_unfinished};
use crate::scenario::route::Router;
use crate::scenario::select::{self, Tag};
use crate::scenario::start;
use crate::scenario::steps::{
    finished_ids, kept_notice, log_error, map_text, ready, resume_run, sync_ledger, title_of, Step,
};
use crate::scenario::store::{project_tag, RunRecord, RunState, Store};
use crate::scenario::sweep;
use crate::scenario::terminal::{Launcher, SystemLauncher};
use crate::scenario::turns::done_turn;

const USAGE: &str = "scenario commands: /scenario (list) · start [number|id|what you want] · talk [skill] · resume [id] · check · pause · drop [id] · tone plain|adventure · help";

#[derive(Debug, PartialEq)]
pub(crate) enum ScenarioCmd {
    List,
    Start(Option<String>),
    Talk(Option<String>),
    Resume(Option<String>),
    Check,
    Pause,
    Drop { id: Option<String>, confirmed: bool },
    Tone(Option<String>),
    Usage,
}

fn rest_of<'a>(it: impl Iterator<Item = &'a str>) -> Option<String> {
    let words: Vec<&str> = it.collect();
    (!words.is_empty()).then(|| words.join(" "))
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
        Some("start") => ScenarioCmd::Start(rest_of(it)),
        Some("talk") => ScenarioCmd::Talk(rest_of(it)),
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
        Some("help") => ScenarioCmd::Usage,
        // Not a verb, so it says what they want: the learner never has to learn `start`.
        Some(_) => ScenarioCmd::Start(rest_of(rest.split_whitespace())),
    })
}

/// Shown before a slow command, so the wait has a reason.
pub(crate) fn busy_note(cmd: &ScenarioCmd) -> Option<&'static str> {
    match cmd {
        ScenarioCmd::Start(_) => Some("preparing your task… this can take a few minutes"),
        ScenarioCmd::Talk(_) => Some("preparing your task…"),
        ScenarioCmd::Resume(_) => Some("resuming your task…"),
        ScenarioCmd::Check => Some("running the check…"),
        ScenarioCmd::Pause => Some("pausing your task…"),
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
    let mut out = if pkgs.is_empty() {
        format!("Nothing prepared for '{topic}' yet.\n")
    } else {
        format!("Tasks for '{topic}':\n")
    };
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
    out.push_str(&if pkgs.is_empty() {
        format!(
            "/scenario start makes a task for you, or say what you want: /scenario start <what you want>.\nTone: {}",
            tone.as_str()
        )
    } else {
        format!(
            "Start one with /scenario start [number|id], or ask for something new: /scenario start <what you want>. Tone: {} (change: /scenario tone plain|adventure)",
            tone.as_str()
        )
    });
    out
}

/// Internal errors (tool output) go to the log; the learner reads one plain sentence.
pub(crate) fn failure(ctx: &Ctx, e: anyhow::Error) -> Reply {
    if e.downcast_ref::<Internal>().is_some() {
        log_error(ctx, &format!("{e:#}"));
        Reply::note("scenario: something went wrong with the task; details are in .usta/scenarios/last-error.log")
    } else if e.downcast_ref::<ModelFailed>().is_some() {
        log_error(ctx, &format!("{e:#}"));
        Reply::note("scenario: Usta could not reach the model just now, so this was not judged; try /scenario check again in a moment")
    } else {
        Reply::note(format!("scenario: {e}"))
    }
}

pub(crate) fn execute(ctx: &Ctx, live: &Live, cmd: ScenarioCmd) -> Reply {
    match run(ctx, live, cmd) {
        Ok(r) => r,
        Err(e) => failure(ctx, e),
    }
}

/// `clean_warning` can carry raw tool text: it goes to the log, never to the learner.
fn clean_notice(ctx: &Ctx, raw: &str) -> String {
    log_error(ctx, raw);
    "the task's server could not be removed yet; Usta will try again at the next start".into()
}

/// Everything a command reaches outside this process, so the whole flow can run
/// on fakes: no runtime, no real terminal window, no real waiting.
pub(crate) struct Seams<'a> {
    pub docker: &'a dyn Docker,
    pub starter: &'a dyn RuntimeStarter,
    pub launcher: &'a dyn Launcher,
    pub alive: &'a dyn Fn(u32) -> bool,
    pub sleep: &'a dyn Fn(Duration),
    /// A shutdown signal arrived: no new work on the learner's behalf.
    pub stopping: &'a dyn Fn() -> bool,
    pub live: &'a Live,
}

fn run(ctx: &Ctx, live: &Live, cmd: ScenarioCmd) -> Result<Reply> {
    run_with(
        ctx,
        cmd,
        &Seams {
            docker: &RealDocker,
            starter: &SystemStarter,
            launcher: &SystemLauncher,
            alive: &crate::lock::pid_alive,
            sleep: &std::thread::sleep,
            stopping: &crate::shutdown::is_requested,
            live,
        },
    )
}

/// One plain sentence for the learner; the details go to the log.
fn bad_notice(ctx: &Ctx, bad: &[library::Bad]) -> Option<String> {
    if bad.is_empty() {
        return None;
    }
    let lines: Vec<String> = bad
        .iter()
        .map(|b| format!("{}: {}", b.dir, b.problems.join("; ")))
        .collect();
    log_error(ctx, &lines.join("\n"));
    Some(format!(
        "{} saved task(s) could not be used and were left out; details are in .usta/scenarios/last-error.log",
        bad.len()
    ))
}

fn check_label(pkg: &Package) -> &'static str {
    if pkg.is_rubric() {
        "check: rubric (Usta judged your work, which is less certain than a script, so one pass does not settle the skill)"
    } else {
        "check: script (it looked at the system state, not at the commands you typed)"
    }
}

/// A folder task's files outlive it and a conversation leaves none; only a server is removed.
fn drop_warning(rec: &RunRecord, title: &str) -> String {
    let id = &rec.run_id;
    match rec.env.as_str() {
        CONTAINER => format!("This removes your work on '{title}' for good, and nothing from it counts toward your progress. To go ahead: /scenario drop {id} confirm"),
        FOLDER => format!("This ends '{title}', and nothing from it counts toward your progress. Your files move to {TASKS_DIR}/{DONE_DIR}/. To go ahead: /scenario drop {id} confirm"),
        _ => format!("This ends '{title}', and nothing from it counts toward your progress. To go ahead: /scenario drop {id} confirm"),
    }
}

pub(crate) fn run_with(ctx: &Ctx, cmd: ScenarioCmd, seams: &Seams) -> Result<Reply> {
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
        _ => {}
    }

    library::sync(&ctx.global)?;
    let loaded = library::load_all(&ctx.global, &ctx.topic)?;
    let pkgs = loaded.pkgs;
    let bad = || bad_notice(ctx, &loaded.bad);
    if matches!(cmd, ScenarioCmd::List) {
        sync_ledger(ctx, &store, &pkgs, &today);
        let map = map_text(ctx);
        let ledger = Ledger::load(&ctx.project_root, &ctx.topic);
        // The same list `start` picks from, so `/scenario start 2` means the second line.
        let offered: Vec<Package> = pkgs
            .iter()
            .filter(|p| library::fits_world(p, &ledger))
            .cloned()
            .collect();
        let threads = select::threads_available(&offered, &map, &ledger.open_ids());
        let unfinished: Vec<RunRecord> = store
            .all()
            .into_iter()
            .filter(|r| {
                library::topic_matches(&r.topic, &ctx.topic)
                    && matches!(r.state, RunState::Paused | RunState::Active)
            })
            .collect();
        let mut text = render_list(
            &ctx.topic,
            &offered,
            &map,
            &finished_ids(&store),
            &threads,
            &unfinished,
            store.tone(),
        );
        if let Some(n) = bad() {
            text.push_str(&format!("\n\n{n}"));
        }
        return Ok(Reply::note(text));
    }

    let docker = seams.docker;
    sweep::park_serverless(&store, &today, docker, seams.alive);
    let mut env = Router::new(
        docker,
        &project_tag(&ctx.project_root),
        store.tasks_dir(),
        seams.live.model.as_ref(),
        &seams.live.said,
    );
    let stamp = chrono::Local::now().format("%Y%m%d%H%M%S").to_string();
    let tick = Tick {
        pid: ctx.pid,
        alive: seams.alive,
        stamp: &stamp,
        today: &today,
    };
    let find_pkg = |id: &str| -> Result<&Package> {
        pkgs.iter().find(|p| p.id == id).ok_or_else(|| {
            if loaded.bad.iter().any(|b| b.dir == id) {
                anyhow!("task '{id}' is damaged; see /scenario")
            } else {
                anyhow!("task '{id}' is not in the library")
            }
        })
    };
    let step = Step {
        ctx,
        seams,
        store: &store,
        tick: &tick,
        pkgs: &pkgs,
    };

    match cmd {
        ScenarioCmd::Start(sel) => start::start(&step, &mut env, sel, bad().into_iter().collect()),
        ScenarioCmd::Talk(skill) => {
            start::talk(&step, &mut env, skill, bad().into_iter().collect())
        }
        ScenarioCmd::Resume(sel) => {
            let pick = || pick_resumable(&store, sel.as_deref(), &ctx.topic, ctx.pid);
            let rec = match pick() {
                Ok(r) if r.env != CONTAINER => r,
                other => {
                    let waiting = store.all().iter().any(|r| {
                        r.env == CONTAINER
                            && matches!(
                                r.state,
                                RunState::Preparing | RunState::Active | RunState::Paused
                            )
                    });
                    if !waiting {
                        other?
                    } else {
                        ready(&step)?;
                        // Never prompts or reads stdin, so it is safe inside the session UI.
                        // It is what parks an active run whose owner died, so it comes before the pick.
                        sweep::sweep_docker(&ctx.project_root, &today, docker, seams.alive);
                        pick()?
                    }
                }
            };
            resume_run(&step, &mut env, find_pkg(&rec.package)?, rec)
        }
        ScenarioCmd::Check => {
            let mut rec = own_active(&store, ctx.pid)?;
            let pkg = find_pkg(&rec.package)?;
            if rec.env == CONTAINER || pkg.needs_runtime() {
                ready(&step)?;
            }
            // A model outage here is an `Err` for `failure`, which says it was not judged.
            let report = engine::check(&mut env, &store, pkg, &mut rec, &tick)?;
            match report.outcome {
                CheckOutcome::Pass => {
                    let tone = Tone::parse(&rec.tone).unwrap_or(Tone::Plain);
                    let mut notices = vec![pkg.tone_text(tone, "close")?, check_label(pkg).into()];
                    let mut ledger = Ledger::load(&ctx.project_root, &ctx.topic);
                    if ledger.apply_finished(&pkg.story, &rec.run_id, &today)
                        && ledger.save(&ctx.project_root, &ctx.topic).is_err()
                    {
                        notices.push("the story notes could not be saved this time; they are added at the next start".into());
                    }
                    if rec.env == FOLDER {
                        notices.push(kept_notice(&step, &rec, report.clean_warning.as_deref()));
                    } else if let Some(w) = report.clean_warning {
                        notices.push(clean_notice(ctx, &w));
                    }
                    Ok(Reply {
                        notices,
                        turn: Some(done_turn(pkg)),
                    })
                }
                CheckOutcome::Fail { output } => {
                    let label = if pkg.is_rubric() {
                        "rubric, Usta's judgment"
                    } else {
                        "script"
                    };
                    Ok(Reply::note(format!("Not yet. check: {label}\n{output}")))
                }
            }
        }
        ScenarioCmd::Pause => {
            let mut rec = own_active(&store, ctx.pid)?;
            // The record may only turn paused once the server really stopped.
            if rec.env == CONTAINER {
                ready(&step)?;
            }
            engine::pause(&mut env, &store, &mut rec, &tick)?;
            Ok(Reply::note(format!(
                "Paused. Continue later with /scenario resume {}",
                rec.run_id
            )))
        }
        ScenarioCmd::Drop { id, confirmed } => {
            let mut rec = pick_unfinished(&store, id.as_deref(), &ctx.topic, ctx.pid)?;
            let title = title_of(&pkgs, &rec.package);
            if !confirmed {
                return Ok(Reply::note(drop_warning(&rec, &title)));
            }
            // Drop works without the runtime: a clean that cannot run is retried at the next start.
            let warning = engine::drop_run(&mut env, &store, &mut rec, &tick)?;
            let dropped = format!("Dropped '{title}'.");
            let notices = match (rec.env.as_str(), warning) {
                (FOLDER, w) => vec![dropped, kept_notice(&step, &rec, w.as_deref())],
                (CONTAINER, None) => vec![format!("{dropped} Its server was removed.")],
                (CONTAINER, Some(w)) => {
                    log_error(ctx, &w);
                    vec![format!("{dropped} Its server could not be removed right now; Usta will try again at the next start.")]
                }
                (_, _) => vec![dropped],
            };
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scenario::container::ContainerEnv;
    use crate::scenario::env::internal;
    use crate::scenario::package::tests::{tmp, write_pkg};
    use crate::scenario::steps::{already_running, resume_notice, server_gone_error};
    use crate::scenario::store::{RunRecord, RunState};
    use crate::scenario::turns::{resume_turn, start_turn};

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
            env: "container".into(),
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
        assert_eq!(parse("/scenario help"), Some(ScenarioCmd::Usage));
        assert_eq!(
            parse("/scenario Price Anchoring"),
            Some(ScenarioCmd::Start(Some("price anchoring".into())))
        );
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
        let start = busy_note(&ScenarioCmd::Start(None)).unwrap();
        assert!(
            start.contains("few minutes") && !start.contains("server"),
            "{start}"
        );
        assert!(busy_note(&ScenarioCmd::Resume(None)).is_some());
        assert!(busy_note(&ScenarioCmd::Check).is_some());
        assert!(busy_note(&ScenarioCmd::Pause).is_some());
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
    fn start_keeps_the_whole_request_and_talk_parses() {
        assert_eq!(
            parse("/scenario start Price Anchoring"),
            Some(ScenarioCmd::Start(Some("price anchoring".into())))
        );
        assert_eq!(parse("/scenario talk"), Some(ScenarioCmd::Talk(None)));
        assert_eq!(
            parse("/scenario talk cold email"),
            Some(ScenarioCmd::Talk(Some("cold email".into())))
        );
    }

    #[test]
    fn an_empty_list_offers_to_make_a_task() {
        let out = render_list("rust", &[], "", &[], &[], &[], Tone::Plain);
        assert!(!out.to_lowercase().contains("no tasks"), "{out}");
        assert!(out.contains("/scenario start"), "{out}");
        assert!(out.contains("Tone: plain"), "{out}");
    }

    #[test]
    fn an_unfinished_task_stays_listed_when_nothing_else_is_offered() {
        let paused = rec("linux-b-2026", RunState::Paused, 0);
        let out = render_list("rust", &[], "", &[], &[], &[paused], Tone::Plain);
        assert!(out.contains("/scenario resume linux-b-2026"), "{out}");
    }

    #[test]
    fn a_full_list_also_says_new_tasks_can_be_asked_for() {
        let out = render_list(
            "linux",
            &[pkg("cmd_pkg_hint")],
            "",
            &[],
            &[],
            &[],
            Tone::Plain,
        );
        assert!(out.contains("/scenario start <what you want>"), "{out}");
    }

    #[test]
    fn turns_keep_the_rule_for_every_kind() {
        use crate::scenario::lint::tests::folder_pkg;
        let root = tmp("cmd_turn_kinds");
        let container = Package::load(&write_pkg(&root.join("c"))).unwrap();
        let folder = Package::load(&folder_pkg(&root.join("f"))).unwrap();
        let mut talk = folder.clone();
        talk.environment.kind = "conversation".into();
        for p in [&container, &folder, &talk] {
            for t in [
                start_turn(p, "Fix it.", Tone::Plain, ""),
                resume_turn(p, "Fix it.", None, true),
                done_turn(p),
            ] {
                assert!(t.contains("never container, image or Docker"), "{t}");
            }
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn each_kind_gets_its_own_workplace_and_a_rubric_pass_is_seen_at_most() {
        use crate::scenario::lint::tests::folder_pkg;
        let root = tmp("cmd_turn_work");
        let container = Package::load(&write_pkg(&root.join("c"))).unwrap();
        let folder = Package::load(&folder_pkg(&root.join("f"))).unwrap();
        let mut talk = folder.clone();
        talk.environment.kind = "conversation".into();
        let start = |p: &Package| start_turn(p, "x", Tone::Plain, "");
        assert!(start(&container).contains("in a terminal"));
        assert!(start(&folder).contains("task folder") && !start(&folder).contains("terminal"));
        assert!(start(&talk).contains("in this conversation"));
        assert!(!resume_turn(&talk, "x", None, true).contains("server survived"));
        assert!(!resume_turn(&talk, "x", None, true).contains("files"));
        assert!(done_turn(&folder).contains("seen at most"));
        assert!(done_turn(&talk).contains("never to settled"));
        assert!(!done_turn(&container).contains("seen at most"));
        let _ = std::fs::remove_dir_all(root);
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
        let r = resume_turn(&p, "Fix it.", Some("fail"), true);
        assert!(r.starts_with("[SCENARIO RESUME]") && r.contains("Their last check: fail"));
        let d = done_turn(&p);
        assert!(d.starts_with("[SCENARIO DONE]") && d.contains("permission"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn learner_text_avoids_infrastructure_words() {
        for c in [
            ScenarioCmd::Start(None),
            ScenarioCmd::Talk(None),
            ScenarioCmd::Resume(None),
            ScenarioCmd::Check,
        ] {
            assert!(!bad_words(busy_note(&c).unwrap()));
        }
        assert!(!bad_words(USAGE));
        use crate::scenario::generate::{SAY_PROVING, SAY_RUBRIC, SAY_WRITING};
        for s in [
            SAY_WRITING,
            SAY_RUBRIC,
            SAY_PROVING,
            crate::scenario::steps::NO_RUNTIME,
        ] {
            assert!(!bad_words(s), "{s}");
        }
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
        let t = execute(
            &ctx,
            &Live::idle(),
            ScenarioCmd::Tone(Some("adventure".into())),
        );
        assert!(t.notices.concat().contains("adventure"));
        let bad = execute(&ctx, &Live::idle(), ScenarioCmd::Tone(Some("loud".into())));
        assert!(bad.notices.concat().contains("plain or adventure"));
        assert!(execute(&ctx, &Live::idle(), ScenarioCmd::Usage)
            .notices
            .concat()
            .contains("drop"));
        let d = execute(
            &ctx,
            &Live::idle(),
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
        assert!(!resume_notice("T", false, true).contains("restarted"));
        assert!(resume_notice("T", true, true).contains("restarted"));
        let broken = resume_notice("T", true, false);
        assert!(!broken.contains("restarted") && broken.contains("did not come back up"));
        let turn = resume_turn(&pkg("cmd_turn_broken"), "Fix it.", None, false);
        assert!(!turn.contains("were restarted") && turn.contains("did not come back up"));
    }

    #[test]
    fn a_model_outage_is_logged_and_shown_as_one_plain_sentence() {
        let g = tmp("cmd_down_g");
        let p = tmp("cmd_down_p");
        let ctx = Ctx {
            global: g.clone(),
            project_root: p.clone(),
            topic: "linux".into(),
            pid: 1,
        };
        let down = failure(
            &ctx,
            crate::scenario::model::model_failed(anyhow::anyhow!("HTTP 529 overloaded_error")),
        );
        let shown = down.notices.concat();
        assert!(
            !shown.contains("529") && !shown.contains("overloaded") && !bad_words(&shown),
            "{shown}"
        );
        assert!(shown.contains("model"), "{shown}");
        let log = std::fs::read_to_string(p.join(".usta/scenarios/last-error.log")).unwrap();
        assert!(log.contains("529"));
        // An unreadable judge reply on a conversation task: no server in the sentence.
        let bad = failure(
            &ctx,
            internal("the judge's reply could not be read: no verdict on a"),
        );
        let shown = bad.notices.concat();
        assert!(
            !shown.contains("server") && !shown.contains("no verdict"),
            "{shown}"
        );
        let _ = std::fs::remove_dir_all(g);
        let _ = std::fs::remove_dir_all(p);
    }
}
