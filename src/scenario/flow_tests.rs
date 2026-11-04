//! Whole-flow tests: `/scenario` commands through `run_with` on a fake runtime
//! that behaves like a tiny daemon. No Docker, no real window, no real waiting.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;

use crate::scenario::command::{run_with, Ctx, Reply, ScenarioCmd, Seams};
use crate::scenario::docker::fake::{fail, ok, FakeDocker};
use crate::scenario::docker::{Out, RuntimeStarter};
use crate::scenario::ledger::{Ledger, Thread};
use crate::scenario::package::tests::tmp;
use crate::scenario::store::{project_tag, RunRecord, RunState, Store};
use crate::scenario::sweep;
use crate::scenario::terminal::{Launcher, WindowReq};

const TODAY: &str = "2026-11-04";
const BACKUP: &str = "linux-backup-permissions";
const CONFIG: &str = "linux-config-typo";

#[derive(Default)]
struct Daemon {
    up: bool,
    /// name -> (running, project tag)
    boxes: BTreeMap<String, (bool, String)>,
    check_code: i32,
    start_script_fails: bool,
}

fn word_after<'a>(line: &'a str, key: &str) -> &'a str {
    line.split(key)
        .nth(1)
        .unwrap_or("")
        .split(' ')
        .next()
        .unwrap_or("")
}

impl Daemon {
    fn answer(&mut self, l: &str) -> Out {
        if !self.up {
            return fail(1, "Cannot connect to the Docker daemon");
        }
        let last = l.rsplit(' ').next().unwrap_or("").to_string();
        let exists = self.boxes.contains_key(&last);
        if l.starts_with("ps ") {
            let project = word_after(l, "label=usta.project=");
            let all = l.starts_with("ps -a");
            let rows: Vec<String> = self
                .boxes
                .iter()
                .filter(|(_, (run, tag))| (project.is_empty() || tag == project) && (all || *run))
                .map(|(n, (run, tag))| {
                    let state = if *run { "running" } else { "exited" };
                    let id = n.trim_start_matches(&format!("usta-{tag}-"));
                    if all {
                        format!("{n}|{state}|{id}\n")
                    } else {
                        format!("{n}\n")
                    }
                })
                .collect();
            ok(&rows.concat())
        } else if l.starts_with("create ") {
            let (name, tag) = (word_after(l, "--name "), word_after(l, "usta.project="));
            self.boxes.insert(name.into(), (false, tag.into()));
            ok("")
        } else if l.starts_with("start ") || l.starts_with("stop ") {
            match self.boxes.get_mut(&last) {
                Some(b) => {
                    b.0 = l.starts_with("start ");
                    ok("")
                }
                None => fail(1, "No such container"),
            }
        } else if l.starts_with("rm ") {
            self.boxes.remove(&last);
            ok("")
        } else if l.starts_with("inspect -f {{.State.Running}}") {
            match self.boxes.get(&last) {
                Some(b) => ok(if b.0 { "true\n" } else { "false\n" }),
                None => fail(1, "No such object"),
            }
        } else if l.starts_with("inspect ") {
            if exists {
                ok("abc\n")
            } else {
                fail(1, "No such object")
            }
        } else if l.contains(" timeout ") {
            let c = self.check_code;
            Out {
                code: c,
                stdout: if c == 1 {
                    "not yet\n".into()
                } else {
                    String::new()
                },
                stderr: String::new(),
            }
        } else if l.contains("start.sh") && self.start_script_fails {
            fail(1, "shopd: bad config")
        } else {
            ok("")
        }
    }
}

struct Starter {
    daemon: Rc<RefCell<Daemon>>,
    calls: Cell<u32>,
}
impl RuntimeStarter for Starter {
    fn start(&self) -> bool {
        self.calls.set(self.calls.get() + 1);
        self.daemon.borrow_mut().up = true;
        true
    }
}

#[derive(Default)]
struct Windows(Cell<u32>);
impl Launcher for Windows {
    fn open(&self, _: &WindowReq) -> bool {
        self.0.set(self.0.get() + 1);
        true
    }
}

/// One learner's machine: a project folder, a runtime and the live Usta pid.
struct World {
    global: PathBuf,
    base: PathBuf,
    project: PathBuf,
    topic: String,
    daemon: Rc<RefCell<Daemon>>,
    docker: FakeDocker,
    starter: Starter,
    windows: Windows,
    pid: Rc<Cell<u32>>,
}

impl World {
    fn new(name: &str) -> World {
        let base = tmp(name);
        let project = base.join("project");
        std::fs::create_dir_all(&project).unwrap();
        let daemon = Rc::new(RefCell::new(Daemon {
            up: true,
            ..Daemon::default()
        }));
        let d2 = daemon.clone();
        World {
            global: base.join("global"),
            base,
            project,
            topic: "linux".into(),
            docker: FakeDocker::new(move |l| d2.borrow_mut().answer(l)),
            starter: Starter {
                daemon: daemon.clone(),
                calls: Cell::new(0),
            },
            windows: Windows::default(),
            pid: Rc::new(Cell::new(101)),
            daemon,
        }
    }

    /// Every story prerequisite met, so any package can be started.
    fn open_the_story(&self) {
        let threads = ["backups-ok", "logs-unrotated"].map(|id| Thread {
            id: id.into(),
            text: id.into(),
            open: true,
        });
        let ledger = Ledger {
            threads: threads.to_vec(),
            ..Ledger::default()
        };
        ledger.save(&self.project, "linux").unwrap();
    }

    fn try_cmd(&self, cmd: ScenarioCmd) -> anyhow::Result<Reply> {
        let ctx = Ctx {
            global: self.global.clone(),
            project_root: self.project.clone(),
            topic: self.topic.clone(),
            pid: self.pid.get(),
        };
        let pid = self.pid.clone();
        let alive = move |p: u32| p == pid.get();
        let seams = Seams {
            docker: &self.docker,
            starter: &self.starter,
            launcher: &self.windows,
            alive: &alive,
            sleep: &|_| {},
        };
        run_with(&ctx, cmd, &seams)
    }

    fn cmd(&self, cmd: ScenarioCmd) -> Reply {
        self.try_cmd(cmd).unwrap()
    }

    fn start(&self, id: Option<&str>) -> Reply {
        self.cmd(ScenarioCmd::Start(id.map(String::from)))
    }

    /// Usta ends without pausing and starts again: the launch sequence of `sweep::startup`.
    fn relaunch(&self) {
        self.pid.set(self.pid.get() + 1);
        let pid = self.pid.clone();
        sweep::drop_foreign(&self.project, TODAY);
        sweep::sweep_docker(&self.project, TODAY, &self.docker, &move |p| p == pid.get());
    }

    fn runs(&self) -> Vec<RunRecord> {
        Store::new(&self.project).all()
    }

    fn only_run(&self) -> RunRecord {
        let runs = self.runs();
        assert_eq!(runs.len(), 1, "{runs:?}");
        runs.into_iter().next().unwrap()
    }

    fn running(&self) -> Vec<String> {
        let d = self.daemon.borrow();
        d.boxes
            .iter()
            .filter(|(_, b)| b.0)
            .map(|(n, _)| n.clone())
            .collect()
    }

    fn box_count(&self) -> usize {
        self.daemon.borrow().boxes.len()
    }

    /// The runtime goes down, and with it everything that ran.
    fn docker_off(&self) {
        let mut d = self.daemon.borrow_mut();
        d.up = false;
        d.boxes.values_mut().for_each(|b| b.0 = false);
    }

    fn removed_anything(&self) -> bool {
        self.docker.calls().iter().any(|c| c.starts_with("rm "))
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.base);
    }
}

fn says(r: &Reply, needle: &str) -> bool {
    r.notices.join("\n").contains(needle)
}

fn turn(r: &Reply) -> &str {
    r.turn.as_deref().unwrap_or("")
}

#[test]
fn start_quit_sweep_resume_check_next_task_keeps_the_ledger() {
    let w = World::new("flow_full");
    let first = w.start(None);
    assert!(turn(&first).starts_with("[SCENARIO START]"));
    assert!(
        !turn(&first).contains("World so far"),
        "nothing happened yet"
    );
    let run = w.only_run();
    assert_eq!(run.package, BACKUP);
    assert_eq!(w.windows.0.get(), 1);
    assert_eq!(w.running().len(), 1);

    w.relaunch();
    assert_eq!(w.only_run().state, RunState::Paused);
    assert!(w.running().is_empty(), "an idle task costs nothing");
    assert!(!w.removed_anything());

    let back = w.cmd(ScenarioCmd::Resume(None));
    assert!(turn(&back).starts_with("[SCENARIO RESUME]"));
    assert_eq!(w.windows.0.get(), 2);
    assert_eq!(w.only_run().owner_pid, w.pid.get());

    w.daemon.borrow_mut().check_code = 1;
    assert!(says(&w.cmd(ScenarioCmd::Check), "Not yet"));
    assert_eq!(w.only_run().last_check.as_deref(), Some("fail"));

    w.daemon.borrow_mut().check_code = 0;
    let pass = w.cmd(ScenarioCmd::Check);
    assert!(turn(&pass).starts_with("[SCENARIO DONE]"));
    assert_eq!(w.only_run().state, RunState::Finished);
    assert_eq!(w.box_count(), 0, "a finished task leaves nothing behind");
    let ledger = Ledger::load(&w.project, "linux");
    assert_eq!(ledger.events.len(), 1);
    assert_eq!(ledger.events[0].run_id, run.run_id);

    let next = w.start(None);
    assert!(turn(&next).contains("World so far"), "{}", turn(&next));
    assert_eq!(w.runs().len(), 2);
    assert_ne!(
        w.runs()
            .iter()
            .filter(|r| r.state == RunState::Active)
            .count(),
        0
    );
}

#[test]
fn docker_off_at_launch_touches_nothing_and_resume_brings_it_back() {
    let w = World::new("flow_docker_off");
    w.start(None);
    w.docker_off();
    w.relaunch();
    assert_eq!(
        w.only_run().state,
        RunState::Active,
        "no sweep without a runtime"
    );
    assert_eq!(w.box_count(), 1);
    assert_eq!(w.starter.calls.get(), 0, "launch itself starts nothing");

    let back = w.cmd(ScenarioCmd::Resume(None));
    assert_eq!(w.starter.calls.get(), 1, "Usta starts the runtime");
    assert!(turn(&back).starts_with("[SCENARIO RESUME]"));
    assert_eq!(w.only_run().state, RunState::Active);
    assert_eq!(w.only_run().owner_pid, w.pid.get());
    assert_eq!(w.running().len(), 1);
    assert!(!w.removed_anything());
}

#[test]
fn a_moved_project_folder_resumes_its_task() {
    let mut w = World::new("flow_moved");
    w.start(None);
    let tag = project_tag(&w.project);
    let before = w.only_run();
    w.cmd(ScenarioCmd::Pause);
    let moved = w.base.join("somewhere-else");
    std::fs::rename(&w.project, &moved).unwrap();
    w.project = moved;
    assert_eq!(project_tag(&w.project), tag);
    w.relaunch();
    let back = w.cmd(ScenarioCmd::Resume(None));
    assert!(turn(&back).starts_with("[SCENARIO RESUME]"));
    let after = w.only_run();
    assert_eq!(
        (after.run_id, after.reference, after.state),
        (before.run_id, before.reference, RunState::Active)
    );
    assert!(!w.removed_anything());
}

#[test]
fn check_with_docker_off_starts_the_runtime_and_names_the_right_command() {
    let w = World::new("flow_check_off");
    w.start(None);
    w.docker_off();
    let e = w.try_cmd(ScenarioCmd::Check).err().unwrap().to_string();
    assert_eq!(w.starter.calls.get(), 1, "check starts the runtime");
    assert!(
        e.contains("/scenario resume") && !e.contains("/scenario start"),
        "{e}"
    );
    assert_eq!(w.only_run().state, RunState::Active);
}

#[test]
fn pause_with_docker_off_starts_the_runtime_so_the_record_stays_true() {
    let w = World::new("flow_pause_off");
    w.start(None);
    w.docker_off();
    assert!(says(&w.cmd(ScenarioCmd::Pause), "Paused"));
    assert_eq!(w.starter.calls.get(), 1);
    assert!(w.docker.calls().iter().any(|c| c.starts_with("stop ")));
    assert_eq!(w.only_run().state, RunState::Paused);
}

#[test]
fn a_runtime_that_cannot_start_leaves_the_record_active_and_names_no_wrong_command() {
    let w = World::new("flow_pause_cannot");
    w.start(None);
    w.docker_off();
    struct Never;
    impl RuntimeStarter for Never {
        fn start(&self) -> bool {
            false
        }
    }
    let ctx = Ctx {
        global: w.global.clone(),
        project_root: w.project.clone(),
        topic: "linux".into(),
        pid: w.pid.get(),
    };
    let seams = Seams {
        docker: &w.docker,
        starter: &Never,
        launcher: &w.windows,
        alive: &|_| true,
        sleep: &|_| {},
    };
    for cmd in [ScenarioCmd::Pause, ScenarioCmd::Check] {
        let e = run_with(&ctx, cmd, &seams).err().unwrap().to_string();
        assert!(
            e.contains("could not start it") && !e.contains("/scenario start"),
            "{e}"
        );
    }
    assert_eq!(w.only_run().state, RunState::Active);
}

#[test]
fn check_and_pause_with_nothing_active_do_not_start_the_runtime() {
    let w = World::new("flow_nothing_active");
    w.docker_off();
    for cmd in [ScenarioCmd::Check, ScenarioCmd::Pause] {
        let e = w.try_cmd(cmd).err().unwrap().to_string();
        assert!(e.contains("no active task"), "{e}");
    }
    assert_eq!(w.starter.calls.get(), 0);
}

#[test]
fn starting_a_package_with_a_paused_run_resumes_it_and_says_so() {
    let w = World::new("flow_dup_paused");
    w.start(Some(BACKUP));
    let run = w.only_run();
    w.cmd(ScenarioCmd::Pause);
    for sel in [Some(BACKUP), None] {
        let r = w.start(sel);
        assert!(says(&r, "instead of starting over"), "{:?}", r.notices);
        assert!(says(&r, &format!("/scenario drop {} confirm", run.run_id)));
        assert!(turn(&r).starts_with("[SCENARIO RESUME]"));
        assert_eq!(w.only_run().run_id, run.run_id);
        assert_eq!(w.box_count(), 1);
        assert_eq!(w.only_run().state, RunState::Active);
        w.cmd(ScenarioCmd::Pause);
    }
}

#[test]
fn starting_the_task_already_running_only_reopens_its_window() {
    let w = World::new("flow_dup_active");
    w.start(Some(BACKUP));
    let r = w.start(Some(BACKUP));
    assert!(says(&r, "instead of starting over"), "{:?}", r.notices);
    assert_eq!(w.windows.0.get(), 2);
    assert_eq!(w.box_count(), 1);
    assert_eq!(w.only_run().state, RunState::Active);
    assert_eq!(r.turn, None, "nothing was woken");
}

#[test]
fn a_duplicate_start_of_a_vanished_task_gets_the_plain_sentence() {
    let w = World::new("flow_dup_gone");
    w.start(Some(BACKUP));
    w.cmd(ScenarioCmd::Pause);
    w.daemon.borrow_mut().boxes.clear();
    let e = w
        .try_cmd(ScenarioCmd::Start(Some(BACKUP.into())))
        .err()
        .unwrap();
    assert!(e.to_string().contains("gone"), "{e}");
    assert_eq!(w.runs().len(), 1);
}

#[test]
fn starting_another_task_says_the_running_one_was_paused() {
    let w = World::new("flow_start_pauses");
    w.open_the_story();
    w.start(Some(BACKUP));
    let old = w.only_run();
    let r = w.start(Some(CONFIG));
    assert!(says(&r, "was running, so it is paused"), "{:?}", r.notices);
    assert!(says(&r, &format!("/scenario resume {}", old.run_id)));
    assert_eq!(w.running().len(), 1);
    assert_eq!(w.runs().len(), 2);
}

#[test]
fn starting_with_nothing_running_mentions_no_pause() {
    let w = World::new("flow_start_quiet");
    let r = w.start(Some(BACKUP));
    assert!(!says(&r, "paused"), "{:?}", r.notices);
    assert!(!says(&r, "instead of starting over"));
}

#[test]
fn bare_resume_over_a_running_task_says_which_one_it_chose() {
    let w = World::new("flow_bare_resume");
    w.open_the_story();
    w.start(Some(BACKUP));
    w.cmd(ScenarioCmd::Pause);
    w.start(Some(CONFIG));
    let r = w.cmd(ScenarioCmd::Resume(None));
    let names = |id: &str| w.runs().into_iter().find(|x| x.package == id).unwrap();
    assert_eq!(names(BACKUP).state, RunState::Active, "the paused one wins");
    assert_eq!(names(CONFIG).state, RunState::Paused);
    assert!(says(&r, "Resumed:"), "{:?}", r.notices);
    assert!(says(&r, "was running, so it is paused"), "{:?}", r.notices);
    assert!(says(
        &r,
        &format!("/scenario resume {}", names(CONFIG).run_id)
    ));
}

#[test]
fn a_failing_start_script_on_resume_still_opens_the_window() {
    let w = World::new("flow_start_script");
    w.open_the_story();
    w.start(Some(CONFIG));
    w.cmd(ScenarioCmd::Pause);
    w.daemon.borrow_mut().start_script_fails = true;
    let id = w
        .runs()
        .into_iter()
        .find(|r| r.package == CONFIG)
        .unwrap()
        .run_id;
    let windows = w.windows.0.get();
    let r = w.cmd(ScenarioCmd::Resume(Some(id.clone())));
    assert_eq!(w.windows.0.get(), windows + 1);
    assert!(says(&r, "did not come back up"), "{:?}", r.notices);
    assert!(!says(&r, "bad config"), "raw tool text stays in the log");
    assert!(turn(&r).contains("did not come back up"), "{}", turn(&r));
    let log = std::fs::read_to_string(w.project.join(".usta/scenarios/last-error.log")).unwrap();
    assert!(log.contains("shopd: bad config"), "{log}");
    assert_eq!(
        Store::new(&w.project).load(&id).unwrap().state,
        RunState::Active
    );
}

#[test]
fn a_subtopic_session_finds_the_packages_and_its_own_runs() {
    let mut w = World::new("flow_subtopic");
    w.topic = "linux-security".into();
    assert!(says(&w.cmd(ScenarioCmd::List), BACKUP));
    w.start(Some(BACKUP));
    let run = w.only_run();
    w.cmd(ScenarioCmd::Pause);
    assert!(
        says(&w.cmd(ScenarioCmd::List), &run.run_id),
        "unfinished is listed"
    );
    let again = w.start(Some(BACKUP));
    assert!(says(&again, "instead of starting over"));
    w.cmd(ScenarioCmd::Pause);
    assert!(turn(&w.cmd(ScenarioCmd::Resume(None))).starts_with("[SCENARIO RESUME]"));
    assert!(says(
        &w.cmd(ScenarioCmd::Drop {
            id: None,
            confirmed: false
        }),
        "for good"
    ));
}
