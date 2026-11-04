//! Whole-flow tests: `/scenario` commands through `run_with` on a fake runtime
//! that behaves like a tiny daemon. No Docker, no real window, no real waiting.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::scenario::command::{failure, run_with, Ctx, Reply, ScenarioCmd, Seams};
use crate::scenario::docker::fake::{fail, ok, FakeDocker};
use crate::scenario::docker::{Out, RuntimeStarter};
use crate::scenario::draft::tests::{
    container_reply, folder_rubric_reply, folder_script_reply, met, REPLY, REVIEW_OK,
};
use crate::scenario::ledger::{Ledger, Thread};
use crate::scenario::library;
use crate::scenario::model::fake::FakeModel;
use crate::scenario::model::{Cancel, Live};
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
    /// Answers for the check script, in order; then `check_code`.
    check_codes: VecDeque<i32>,
    start_script_fails: bool,
    /// The runtime stops answering the moment a check has run.
    dies_at_check: bool,
    stop_fails: bool,
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
        } else if l.starts_with("stop ") && self.stop_fails {
            fail(1, "cannot stop")
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
        } else if l.contains(" timeout ") && l.contains(".usta-solution") {
            ok("")
        } else if l.contains(" timeout ") {
            let check = l.contains(".usta-check");
            let c = match self.check_codes.pop_front() {
                Some(c) if check => c,
                other => {
                    self.check_codes.extend(other);
                    self.check_code
                }
            };
            self.up = !(check && self.dies_at_check);
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
    stuck: Cell<bool>,
}
impl RuntimeStarter for Starter {
    fn start(&self) -> bool {
        self.calls.set(self.calls.get() + 1);
        if self.stuck.get() {
            return false;
        }
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
    missing: Rc<Cell<bool>>,
    starter: Starter,
    windows: Windows,
    pid: Rc<Cell<u32>>,
    model: Arc<FakeModel>,
    calls_seen: Cell<usize>,
    /// What the live session reported, and the line that presses Esc.
    progress: Arc<Mutex<Vec<String>>>,
    esc_at: Arc<Mutex<Option<String>>>,
    cancel: Arc<Cancel>,
    conversation: RefCell<Vec<String>>,
    stopping: Cell<bool>,
    /// Other process ids count as live Ustas.
    others_alive: Cell<bool>,
    /// Esc arrives while Usta waits for the runtime.
    esc_in_wait: Cell<bool>,
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
        let missing = Rc::new(Cell::new(false));
        World {
            global: base.join("global"),
            base,
            project,
            topic: "linux".into(),
            docker: FakeDocker::switchable(missing.clone(), move |l| d2.borrow_mut().answer(l)),
            missing,
            starter: Starter {
                daemon: daemon.clone(),
                calls: Cell::new(0),
                stuck: Cell::new(false),
            },
            windows: Windows::default(),
            pid: Rc::new(Cell::new(101)),
            model: Arc::default(),
            calls_seen: Cell::new(0),
            progress: Arc::default(),
            esc_at: Arc::default(),
            cancel: Arc::default(),
            conversation: RefCell::default(),
            stopping: Cell::new(false),
            others_alive: Cell::new(false),
            esc_in_wait: Cell::new(false),
            daemon,
        }
    }

    fn live(&self) -> Live {
        let (said, esc_at, cancel) = (
            self.progress.clone(),
            self.esc_at.clone(),
            self.cancel.clone(),
        );
        Live {
            model: self.model.clone(),
            said: self.conversation.borrow().clone(),
            progress: Arc::new(move |m: &str| {
                said.lock().unwrap().push(m.to_string());
                if esc_at
                    .lock()
                    .unwrap()
                    .as_deref()
                    .is_some_and(|t| m.contains(t))
                {
                    cancel.request();
                }
            }),
            cancel: self.cancel.clone(),
        }
    }

    fn topic_is(&mut self, topic: &str) {
        self.topic = topic.into();
    }

    fn model_says(&self, replies: &[&str]) {
        let mut queue = self.model.replies.lock().unwrap();
        queue.extend(replies.iter().map(|r| Ok(r.to_string())));
    }

    fn model_fails(&self, why: &str) {
        let mut queue = self.model.replies.lock().unwrap();
        queue.push_back(Err(why.to_string()));
    }

    /// The system prompt and the request of the `i`th model call.
    fn model_prompt(&self, i: usize) -> (String, String) {
        self.model.prompts.lock().unwrap()[i].clone()
    }

    fn model_calls_since_mark(&self) -> usize {
        let all = self.model.prompts.lock().unwrap().len();
        all - self.calls_seen.replace(all)
    }

    fn cancel_when_said(&self, text: &str) {
        *self.esc_at.lock().unwrap() = Some(text.to_string());
    }

    /// The progress lines the session showed.
    fn said(&self) -> Vec<String> {
        self.progress.lock().unwrap().clone()
    }

    /// The learner answers in the conversation.
    fn say(&self, text: &str) {
        self.conversation
            .borrow_mut()
            .push(format!("learner: {text}"));
    }

    fn docker_missing(&self) {
        self.missing.set(true);
    }

    fn windows_opened(&self) -> u32 {
        self.windows.0.get()
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
        let alive = |p: u32| p == pid.get() || self.others_alive.get();
        let live = self.live();
        let seams = Seams {
            docker: &self.docker,
            starter: &self.starter,
            launcher: &self.windows,
            alive: &alive,
            sleep: &|_| {
                if self.esc_in_wait.get() {
                    self.cancel.request();
                }
            },
            stopping: &|| self.stopping.get(),
            live: &live,
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
    let live = w.live();
    let seams = Seams {
        docker: &w.docker,
        starter: &Never,
        launcher: &w.windows,
        alive: &|_| true,
        sleep: &|_| {},
        stopping: &|| false,
        live: &live,
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

#[test]
fn a_copied_project_and_its_original_never_touch_each_others_work() {
    let mut w = World::new("flow_copied");
    w.start(Some(BACKUP));
    w.cmd(ScenarioCmd::Pause);
    let original = w.project.clone();
    let tag = project_tag(&original);
    let kept = w.only_run();

    let copy = w.base.join("copy");
    let cp = std::process::Command::new("cp")
        .arg("-R")
        .arg(&original)
        .arg(&copy)
        .status()
        .unwrap();
    assert!(cp.success());

    // The copy opens: fresh identity, and the copied record is foreign, not resumable.
    w.project = copy.clone();
    w.relaunch();
    let fresh = project_tag(&copy);
    assert_ne!(fresh, tag);
    assert_eq!(w.only_run().state, RunState::Dropped);
    assert_eq!(w.box_count(), 1, "the original's container is untouched");

    // Its own new task runs under the fresh tag.
    let r = w.start(Some(BACKUP));
    assert!(!says(&r, "instead of starting over"), "{:?}", r.notices);
    w.cmd(ScenarioCmd::Pause);
    assert_eq!(w.box_count(), 2);
    let copys_box = format!("usta-{fresh}-");
    let has_box = w
        .daemon
        .borrow()
        .boxes
        .keys()
        .any(|n| n.starts_with(&copys_box));
    assert!(has_box);

    // The original's sweep sees none of that, and its own task is as it was.
    w.project = original;
    w.relaunch();
    assert_eq!(project_tag(&w.project), tag);
    assert_eq!(w.box_count(), 2);
    assert!(!w.removed_anything(), "{:?}", w.docker.calls());
    assert_eq!(w.only_run().state, RunState::Paused);
    assert_eq!(w.only_run().reference, kept.reference);
    assert!(turn(&w.cmd(ScenarioCmd::Resume(None))).starts_with("[SCENARIO RESUME]"));
    assert_eq!(w.box_count(), 2);
    assert!(!w.removed_anything());
}

/// Pause a task, then move the project away, leaving its old path free.
fn moved_away(w: &mut World) -> (PathBuf, PathBuf, String) {
    w.start(Some(BACKUP));
    w.cmd(ScenarioCmd::Pause);
    let old = w.project.clone();
    let tag = project_tag(&old);
    let moved = w.base.join("moved");
    std::fs::rename(&old, &moved).unwrap();
    std::fs::create_dir_all(&old).unwrap();
    (old, moved, tag)
}

fn assert_moved_work_survives(w: &mut World, moved: PathBuf, tag: &str) {
    w.project = moved;
    w.relaunch();
    assert_eq!(project_tag(&w.project), tag, "the move kept its identity");
    let paused: Vec<RunRecord> = w
        .runs()
        .into_iter()
        .filter(|r| r.state == RunState::Paused)
        .collect();
    assert_eq!(paused.len(), 1, "{:?}", w.runs());
    assert!(!w.removed_anything(), "{:?}", w.docker.calls());
    assert!(turn(&w.cmd(ScenarioCmd::Resume(None))).starts_with("[SCENARIO RESUME]"));
    assert!(!w.removed_anything());
}

#[test]
fn a_new_project_at_the_old_path_leaves_the_moved_work_alone() {
    let mut w = World::new("flow_new_at_old");
    let (old, moved, tag) = moved_away(&mut w);

    // The new project opens first and works: its sweep must not see the moved container.
    w.project = old;
    assert_ne!(project_tag(&w.project), tag);
    w.start(Some(BACKUP));
    w.relaunch();
    assert_eq!(w.box_count(), 2);
    assert!(!w.removed_anything(), "{:?}", w.docker.calls());

    assert_moved_work_survives(&mut w, moved, &tag);
}

#[test]
fn a_tone_only_folder_at_the_old_path_does_not_make_a_move_a_copy() {
    let mut w = World::new("flow_tone_at_old");
    let (old, moved, tag) = moved_away(&mut w);

    // `/scenario tone` makes .usta/scenarios/ without any tag file.
    w.project = old.clone();
    w.cmd(ScenarioCmd::Tone(Some("adventure".into())));
    assert!(old.join(".usta/scenarios/tone").exists());
    assert!(!old.join(".usta/scenarios/tag").exists());

    assert_moved_work_survives(&mut w, moved, &tag);
}

#[test]
fn a_lost_tag_file_does_not_drop_the_task() {
    let w = World::new("flow_lost_tag");
    w.start(Some(BACKUP));
    w.cmd(ScenarioCmd::Pause);
    let tag = project_tag(&w.project);
    std::fs::remove_file(w.project.join(".usta/scenarios/tag")).unwrap();
    w.relaunch();
    assert_eq!(project_tag(&w.project), tag);
    assert_eq!(w.only_run().state, RunState::Paused);
    assert!(turn(&w.cmd(ScenarioCmd::Resume(None))).starts_with("[SCENARIO RESUME]"));
    assert!(!w.removed_anything());
}

/// A world whose only task is a folder task (topic "marketing").
fn folder_world(name: &str) -> World {
    let mut w = World::new(name);
    w.topic = "marketing".into();
    let src = w.base.join("src");
    let pkg = crate::scenario::lint::tests::folder_pkg(&src);
    let lib = crate::scenario::library::library_root(&w.global).join("demo-pkg");
    crate::scenario::folder::copy_data(&pkg, &lib).unwrap();
    w
}

fn inspects(w: &World) -> usize {
    w.docker
        .calls()
        .iter()
        .filter(|c| c.starts_with("inspect"))
        .count()
}

#[test]
fn a_folder_task_resumes_without_asking_the_runtime_and_opens_no_window() {
    let w = folder_world("flow_folder_resume");
    w.start(Some("demo-pkg"));
    let run = w.only_run();
    assert_eq!(run.env, "folder");
    w.cmd(ScenarioCmd::Pause);
    assert_eq!(w.only_run().state, RunState::Paused);
    let before = inspects(&w);
    let back = w.cmd(ScenarioCmd::Resume(None));
    assert_eq!(inspects(&w), before, "{:?}", w.docker.calls());
    assert_eq!(w.windows.0.get(), 0);
    assert!(says(&back, "usta-tasks"), "{:?}", back.notices);
    assert!(!says(&back, "terminal window"), "{:?}", back.notices);
    assert!(turn(&back).starts_with("[SCENARIO RESUME]"));
    assert!(!turn(&back).contains("services"), "{}", turn(&back));
    assert_eq!(w.only_run().state, RunState::Active);
}

#[test]
fn a_folder_task_whose_files_are_gone_gets_the_plain_sentence() {
    let w = folder_world("flow_folder_gone");
    w.start(Some("demo-pkg"));
    w.cmd(ScenarioCmd::Pause);
    let dir = Store::new(&w.project).tasks_dir().join(w.only_run().run_id);
    std::fs::remove_dir_all(dir).unwrap();
    let e = w.try_cmd(ScenarioCmd::Resume(None)).err().unwrap();
    assert!(
        e.to_string().contains("files for this task are gone"),
        "{e}"
    );
    assert_eq!(w.only_run().state, RunState::Paused);
}

#[test]
fn an_own_active_folder_task_says_back_in_and_wakes_nothing() {
    let w = folder_world("flow_folder_active");
    w.start(Some("demo-pkg"));
    let r = w.start(Some("demo-pkg"));
    assert!(
        says(&r, "Back in") || says(&r, "instead of starting over"),
        "{:?}",
        r.notices
    );
    assert!(says(&r, "usta-tasks"), "{:?}", r.notices);
    assert_eq!(r.turn, None);
    assert_eq!(w.windows.0.get(), 0);
    assert_eq!(w.only_run().state, RunState::Active);
}

#[test]
fn any_topic_gets_a_task_on_start_with_no_package_at_all() {
    let mut w = World::new("flow_gen_rubric");
    w.topic_is("pottery");
    w.model_says(&[REPLY, REVIEW_OK, &met(&["a", "b"]), &met(&["a", "b"])]);
    let r = w.start(None);
    assert!(
        says(&r, "Who is it for?") && says(&r, "/scenario check"),
        "{:?}",
        r.notices
    );
    assert!(turn(&r).contains("in this conversation"));
    assert_eq!(w.only_run().env, "conversation");
    assert_eq!(w.box_count(), 0, "a conversation task starts no server");
    assert!(w.said().iter().any(|s| s.contains("Writing a new task")));
}

#[test]
fn a_used_up_topic_generates_the_next_task_and_the_library_keeps_it() {
    let mut w = World::new("flow_gen_next");
    w.topic_is("pottery");
    w.model_says(&[REPLY, REVIEW_OK, &met(&["a", "b"]), &met(&["a", "b"])]);
    w.start(None);
    assert_eq!(
        library::load_all(&w.global, "pottery").unwrap().pkgs.len(),
        1
    );
    w.say("Bakers, for the price.");
    w.model_says(&[&met(&["a", "b"]), &met(&["a", "b"])]);
    w.model_calls_since_mark();
    let done = w.cmd(ScenarioCmd::Check);
    assert!(says(&done, "check: rubric"), "{:?}", done.notices);
    assert!(!says(&done, "check: script"), "{:?}", done.notices);
    assert!(turn(&done).contains("seen"), "{}", turn(&done));
    assert_eq!(w.model_calls_since_mark(), 2);
    w.model_says(&[REPLY, REVIEW_OK, &met(&["a", "b"]), &met(&["a", "b"])]);
    w.start(None);
    assert_eq!(w.runs().len(), 2);
}

#[test]
fn a_package_that_never_proves_itself_is_discarded_and_a_conversation_starts() {
    let mut w = World::new("flow_gen_fail");
    w.topic_is("nginx-ops");
    let c = container_reply();
    w.model_says(&[&c, &c, &c, &c]);
    w.daemon.borrow_mut().check_code = 0; // the check passes on a fresh task, every round
    let r = w.start(None);
    assert!(says(&r, "as a conversation"), "{:?}", r.notices);
    assert!(!says(&r, "no tasks"));
    assert_eq!(w.only_run().env, "conversation", "the learner has a task");
    assert!(turn(&r).starts_with("[SCENARIO START]"));
    let left = library::load_all(&w.global, "nginx-ops").unwrap().pkgs;
    assert_eq!(left.len(), 1, "only the fixed-text task");
    assert_eq!(left[0].environment.kind, "conversation");
    assert_eq!(w.box_count(), 0, "validation left nothing behind");
    let log = std::fs::read_to_string(w.project.join(".usta/scenarios/last-error.log")).unwrap();
    assert!(log.contains("already passes"), "{log}");
}

#[test]
fn a_proven_container_task_starts_on_the_validated_image() {
    let mut w = World::new("flow_gen_box");
    w.topic_is("nginx-ops");
    w.model_says(&[&container_reply()]);
    w.daemon.borrow_mut().check_codes.extend([1, 1, 0]);
    let r = w.start(None);
    assert!(says(&r, "terminal window"), "{:?}", r.notices);
    assert_eq!(w.only_run().env, "container");
    assert_eq!(w.running().len(), 1);
    assert!(w.runs().iter().all(|r| !r.run_id.starts_with("val-")));
}

#[test]
fn talk_works_with_no_runtime_and_falls_back_to_fixed_text() {
    let mut w = World::new("flow_talk_fallback");
    w.topic_is("pottery");
    w.docker_missing();
    w.model_says(&["nonsense", "nonsense", "nonsense", "nonsense"]);
    let r = w.cmd(ScenarioCmd::Talk(Some("glazing".into())));
    assert!(says(&r, "glazing"), "{:?}", r.notices);
    assert_eq!(w.only_run().env, "conversation");
}

#[test]
fn talk_that_generates_starts_that_task_without_the_runtime() {
    let mut w = World::new("flow_talk_gen");
    w.topic_is("pottery");
    w.docker_missing();
    w.model_says(&[REPLY, REVIEW_OK, &met(&["a", "b"]), &met(&["a", "b"])]);
    let r = w.cmd(ScenarioCmd::Talk(None));
    assert!(says(&r, "Who is it for?"), "{:?}", r.notices);
    assert_eq!(w.only_run().env, "conversation");
}

#[test]
fn a_model_that_asks_for_a_system_during_talk_still_gets_the_fallback() {
    let mut w = World::new("flow_talk_needs");
    w.topic_is("pottery");
    w.model_says(&[r#"{"kind":"needs_system","why":"a kiln must run"}"#]);
    let r = w.cmd(ScenarioCmd::Talk(Some("glazing".into())));
    assert!(says(&r, "glazing"), "{:?}", r.notices);
    assert_eq!(w.only_run().env, "conversation");
}

#[test]
fn esc_during_talk_installs_no_fallback() {
    let mut w = World::new("flow_talk_esc");
    w.topic_is("pottery");
    w.cancel_when_said("Writing a new task");
    w.model_says(&["nonsense"]);
    let r = w.cmd(ScenarioCmd::Talk(Some("glazing".into())));
    assert!(r.turn.is_none() && r.notices.is_empty(), "{:?}", r.notices);
    assert!(w.runs().is_empty());
    let left = library::load_all(&w.global, "pottery").unwrap().pkgs;
    assert!(left.is_empty(), "nothing was installed");
}

#[test]
fn a_shutdown_during_talk_installs_no_fallback() {
    let mut w = World::new("flow_talk_stop");
    w.topic_is("pottery");
    w.stopping.set(true);
    w.model_says(&["nonsense", "nonsense", "nonsense", "nonsense"]);
    let r = w.cmd(ScenarioCmd::Talk(Some("glazing".into())));
    assert!(r.turn.is_none(), "{:?}", r.notices);
    assert!(w.runs().is_empty());
    let left = library::load_all(&w.global, "pottery").unwrap().pkgs;
    assert!(left.is_empty(), "nothing was installed");
}

#[test]
fn without_docker_a_generated_task_needs_none_and_usta_says_so_once() {
    let mut w = World::new("flow_no_docker");
    w.topic_is("pottery");
    w.docker_missing();
    w.model_says(&[REPLY, REVIEW_OK, &met(&["a", "b"]), &met(&["a", "b"])]);
    let first = w.start(None);
    assert!(says(&first, "needs none"), "{:?}", first.notices);
    assert!(w.model_prompt(0).0.contains("cannot run a server now"));
    w.cmd(ScenarioCmd::Drop {
        id: None,
        confirmed: true,
    });
    w.model_says(&[REPLY, REVIEW_OK, &met(&["a", "b"]), &met(&["a", "b"])]);
    let second = w.start(Some("another one"));
    assert!(!says(&second, "needs none"), "{:?}", second.notices);
}

#[test]
fn a_bare_start_with_only_server_tasks_and_no_docker_generates_instead_of_refusing() {
    let w = World::new("flow_no_docker_library");
    w.docker_missing();
    w.model_says(&[REPLY, REVIEW_OK, &met(&["a", "b"]), &met(&["a", "b"])]);
    let r = w.start(None);
    assert_eq!(w.only_run().env, "conversation", "{:?}", r.notices);
    assert!(!says(&r, "docs.docker.com"), "{:?}", r.notices);
}

#[test]
fn an_explicit_pick_of_a_server_task_without_docker_gets_guidance_and_a_talk_on_it() {
    let w = World::new("flow_no_docker_explicit");
    w.docker_missing();
    w.model_says(&["nonsense", "nonsense", "nonsense", "nonsense"]);
    let r = w.start(Some(BACKUP));
    assert!(says(&r, "docs.docker.com"), "{:?}", r.notices);
    assert_eq!(w.only_run().env, "conversation");
    let title = library::load_all(&w.global, "linux")
        .unwrap()
        .pkgs
        .into_iter()
        .find(|p| p.id == BACKUP)
        .unwrap()
        .title;
    assert!(says(&r, &title), "{:?}", r.notices);
}

#[test]
fn a_runtime_that_cannot_come_up_sends_a_bare_start_to_a_task_that_needs_none() {
    let w = World::new("flow_runtime_stuck");
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
    let live = w.live();
    let seams = Seams {
        docker: &w.docker,
        starter: &Never,
        launcher: &w.windows,
        alive: &|_| true,
        sleep: &|_| {},
        stopping: &|| false,
        live: &live,
    };
    w.model_says(&[REPLY, REVIEW_OK, &met(&["a", "b"]), &met(&["a", "b"])]);
    let r = run_with(&ctx, ScenarioCmd::Start(None), &seams).unwrap();
    assert_eq!(w.only_run().env, "conversation", "{:?}", r.notices);
}

#[test]
fn a_subject_that_truly_needs_a_system_gets_install_guidance_and_a_conversation() {
    let mut w = World::new("flow_needs_system");
    w.topic_is("nginx-ops");
    w.docker_missing();
    w.model_says(&[r#"{"kind":"needs_system","why":"nginx must run"}"#]);
    let r = w.start(None);
    assert!(says(&r, "docs.docker.com"), "{:?}", r.notices);
    assert!(!says(&r, "needs none"), "{:?}", r.notices);
    assert_eq!(w.only_run().env, "conversation");
}

#[test]
fn a_folder_task_never_needs_the_runtime_and_keeps_the_running_task_rule() {
    let mut w = World::new("flow_folder");
    w.topic_is("marketing");
    w.model_says(&[
        &folder_rubric_reply(),
        REVIEW_OK,
        &met(&["a", "b"]),
        &met(&["a", "b"]),
    ]);
    w.docker_missing();
    let r = w.start(None);
    assert!(says(&r, "usta-tasks"), "{:?}", r.notices);
    assert!(says(&r, "your files move to usta-tasks/done/"));
    let run = w.only_run();
    let dir = w.project.join("usta-tasks").join(&run.run_id);
    assert!(dir.join("brief.md").is_file());
    let p = w.cmd(ScenarioCmd::Pause);
    assert!(says(&p, "Paused"));
    let b = w.cmd(ScenarioCmd::Resume(None));
    assert!(says(&b, "usta-tasks"));
    assert_eq!(w.windows_opened(), 0);
    let warned = w.cmd(ScenarioCmd::Drop {
        id: None,
        confirmed: false,
    });
    assert!(
        says(&warned, "usta-tasks/done/") && !says(&warned, "for good"),
        "{:?}",
        warned.notices
    );
    std::fs::write(dir.join("plan.md"), "mine\n").unwrap();
    let dropped = w.cmd(ScenarioCmd::Drop {
        id: None,
        confirmed: true,
    });
    assert!(
        says(&dropped, "kept in usta-tasks/done/"),
        "{:?}",
        dropped.notices
    );
    assert!(!says(&dropped, "server"), "{:?}", dropped.notices);
    assert!(!dir.exists());
    let done = w.project.join("usta-tasks/done").join(&run.run_id);
    assert!(done.join("plan.md").is_file());
}

#[test]
fn a_finished_folder_task_keeps_the_learners_work() {
    let mut w = World::new("flow_folder_done");
    w.topic_is("marketing");
    w.model_says(&[
        &folder_rubric_reply(),
        REVIEW_OK,
        &met(&["a", "b"]),
        &met(&["a", "b"]),
    ]);
    w.start(None);
    let run = w.only_run();
    let dir = w.project.join("usta-tasks").join(&run.run_id);
    std::fs::write(dir.join("plan.md"), "mine\n").unwrap();
    w.docker_missing();
    w.model_says(&[&met(&["a", "b"]), &met(&["a", "b"])]);
    let r = w.cmd(ScenarioCmd::Check);
    assert!(says(&r, "kept in usta-tasks/done/"), "{:?}", r.notices);
    assert!(says(&r, "check: rubric") && !says(&r, "check: script"));
    assert!(!dir.exists());
    let kept = w
        .project
        .join("usta-tasks/done")
        .join(&run.run_id)
        .join("plan.md");
    assert_eq!(std::fs::read_to_string(kept).unwrap(), "mine\n");
}

#[test]
fn a_rubric_failure_is_labelled_as_judgment_not_script() {
    let mut w = World::new("flow_rubric_fail");
    w.topic_is("pottery");
    w.model_says(&[REPLY, REVIEW_OK, &met(&["a", "b"]), &met(&["a", "b"])]);
    w.start(None);
    let r = w.cmd(ScenarioCmd::Check);
    assert!(says(&r, "Not yet. check: rubric"), "{:?}", r.notices);
}

#[test]
fn a_conversation_task_needs_no_runtime_for_any_command() {
    let mut w = World::new("flow_talk_no_runtime");
    w.topic_is("pottery");
    w.docker_missing();
    w.model_says(&[REPLY, REVIEW_OK, &met(&["a", "b"]), &met(&["a", "b"])]);
    let r = w.start(None);
    assert!(says(&r, "Answer here"), "{:?}", r.notices);
    assert!(says(&w.cmd(ScenarioCmd::Pause), "Paused"));
    let back = w.cmd(ScenarioCmd::Resume(None));
    assert!(says(&back, "Resumed"), "{:?}", back.notices);
    assert!(says(&back, "Answer here"), "{:?}", back.notices);
    assert!(turn(&back).starts_with("[SCENARIO RESUME]"));
    assert!(!turn(&back).contains("server survived"), "{}", turn(&back));
    assert_eq!(w.windows_opened(), 0);
    let again = w.cmd(ScenarioCmd::Resume(None));
    assert!(
        turn(&again).starts_with("[SCENARIO RESUME]"),
        "a fresh mentor turn"
    );
    w.say("Bakers.");
    w.model_says(&[&met(&["a", "b"]), &met(&["a", "b"])]);
    assert!(says(&w.cmd(ScenarioCmd::Check), "check: rubric"));
    assert_eq!(w.windows_opened(), 0);
}

#[test]
fn dropping_a_conversation_task_promises_nothing_about_a_server_or_files() {
    let mut w = World::new("flow_talk_drop");
    w.topic_is("pottery");
    w.model_says(&[REPLY, REVIEW_OK, &met(&["a", "b"]), &met(&["a", "b"])]);
    w.start(None);
    let warned = w.cmd(ScenarioCmd::Drop {
        id: None,
        confirmed: false,
    });
    assert!(
        !says(&warned, "server") && !says(&warned, "files"),
        "{:?}",
        warned.notices
    );
    let gone = w.cmd(ScenarioCmd::Drop {
        id: None,
        confirmed: true,
    });
    assert_eq!(gone.notices, vec!["Dropped 'T'.".to_string()]);
}

#[test]
fn a_crashed_session_leaves_a_folder_task_resumable_without_docker() {
    let w = folder_world("flow_folder_crash");
    w.start(Some("demo-pkg"));
    w.docker_missing();
    w.relaunch();
    assert_eq!(
        w.only_run().state,
        RunState::Active,
        "no sweep without a runtime"
    );
    let back = w.cmd(ScenarioCmd::Resume(None));
    assert!(turn(&back).starts_with("[SCENARIO RESUME]"));
    assert_eq!(w.only_run().owner_pid, w.pid.get());
}

#[test]
fn a_paused_server_task_does_not_make_a_folder_resume_wait_for_the_runtime() {
    let w = folder_world("flow_folder_beside_box");
    w.start(Some("demo-pkg"));
    let folder = w.only_run().run_id;
    w.cmd(ScenarioCmd::Pause);
    let mut paused = Store::new(&w.project).load(&folder).unwrap();
    paused.run_id = "other-run".into();
    paused.env = "container".into();
    paused.reference = format!("usta-{}-other-run", project_tag(&w.project));
    Store::new(&w.project).save(&paused).unwrap();
    w.docker_missing();
    let back = w.cmd(ScenarioCmd::Resume(Some(folder)));
    assert!(says(&back, "usta-tasks"), "{:?}", back.notices);
}

#[test]
fn leaving_a_running_server_task_for_a_folder_task_stops_it_first() {
    let mut w = World::new("flow_box_then_folder");
    w.start(Some(BACKUP));
    w.docker_off();
    w.topic = "marketing".into();
    let src = w.base.join("src");
    let pkg = crate::scenario::lint::tests::folder_pkg(&src);
    let lib = library::library_root(&w.global).join("demo-pkg");
    crate::scenario::folder::copy_data(&pkg, &lib).unwrap();
    let r = w.start(Some("demo-pkg"));
    assert!(says(&r, "was running, so it is paused"), "{:?}", r.notices);
    assert_eq!(
        w.starter.calls.get(),
        1,
        "the runtime came up for the pause"
    );
    assert!(w.docker.calls().iter().any(|c| c.starts_with("stop ")));
}

#[test]
fn no_runc_warning_is_looked_for_on_a_task_that_needs_no_runtime() {
    let w = folder_world("flow_no_runc");
    w.start(Some("demo-pkg"));
    let asked = w.docker.calls();
    assert!(
        asked
            .iter()
            .all(|c| !c.contains("{{json .Server.Components}}")),
        "{asked:?}"
    );
    assert_eq!(w.starter.calls.get(), 0);
}

#[test]
fn esc_during_generation_changes_nothing_the_learner_can_see() {
    let w = World::new("flow_esc");
    w.open_the_story();
    w.start(Some(BACKUP));
    let running = w.only_run();
    w.cancel_when_said("Writing a new task");
    w.model_says(&[REPLY]);
    let r = w.start(Some("something new"));
    assert!(r.turn.is_none() && r.notices.is_empty(), "{:?}", r.notices);
    assert_eq!(w.only_run().run_id, running.run_id, "no new run");
    assert_eq!(
        w.only_run().state,
        RunState::Active,
        "the running task was not paused"
    );
    assert_eq!(w.running().len(), 1);
    let all = library::load_all(&w.global, "linux").unwrap().pkgs;
    assert!(all.iter().all(|p| !p.is_generated()));
}

#[test]
fn a_script_proof_pauses_the_running_task_and_says_so_even_if_it_then_fails() {
    let w = World::new("flow_pause_late");
    w.open_the_story();
    w.start(Some(BACKUP));
    let c = container_reply();
    w.model_says(&[&c, &c, &c, &c]);
    w.daemon.borrow_mut().check_code = 0; // every round fails: the check passes on a fresh task
    let r = w.start(Some("something new"));
    assert!(says(&r, "paused now"), "{:?}", r.notices);
    let old = w.runs().into_iter().find(|r| r.package == BACKUP).unwrap();
    assert_eq!(old.state, RunState::Paused);
}

#[test]
fn a_folder_script_check_starts_the_runtime_but_a_folder_rubric_does_not() {
    let mut w = World::new("flow_folder_script");
    w.topic_is("marketing");
    w.model_says(&[&folder_script_reply()]);
    w.daemon.borrow_mut().check_codes.extend([1, 1, 0]);
    let r = w.start(None);
    assert_eq!(w.only_run().env, "folder", "{:?}", r.notices);
    w.docker_off();
    w.daemon.borrow_mut().check_codes.push_back(1);
    let r = w.cmd(ScenarioCmd::Check);
    assert!(says(&r, "Not yet. check: script"), "{:?}", r.notices);
    assert!(w.starter.calls.get() > 0);
}

#[test]
fn a_bad_library_package_is_reported_on_start() {
    let w = World::new("flow_bad_pkg");
    library::sync(&w.global).unwrap();
    let broken = library::library_root(&w.global).join("broken");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(broken.join("task.toml"), "not toml [").unwrap();
    let r = w.start(None);
    assert!(says(&r, "could not be used"), "{:?}", r.notices);
}

#[test]
fn an_unreachable_model_gets_one_plain_sentence_and_changes_nothing() {
    let mut w = World::new("flow_model_down");
    w.topic_is("pottery");
    w.model_fails("401 invalid x-api-key SECRET");
    let r = w.start(None);
    assert!(
        says(&r, "could not reach the model") && !says(&r, "SECRET"),
        "{:?}",
        r.notices
    );
    assert_eq!(r.notices.len(), 1, "{:?}", r.notices);
    assert!(w.runs().is_empty());
    let log = std::fs::read_to_string(w.project.join(".usta/scenarios/last-error.log")).unwrap();
    assert!(log.contains("SECRET"), "{log}");
}

#[test]
fn a_dead_runtime_during_a_proof_is_not_called_a_model_outage() {
    let w = World::new("flow_dead_runtime");
    w.open_the_story();
    w.start(Some(BACKUP));
    w.daemon.borrow_mut().dies_at_check = true;
    w.model_says(&[&container_reply()]);
    let r = w.start(Some("something new"));
    assert!(!says(&r, "could not reach the model"), "{:?}", r.notices);
    assert!(says(&r, "stopped answering"), "{:?}", r.notices);
    assert!(says(&r, "paused now"), "{:?}", r.notices);
}

#[test]
fn a_model_outage_on_check_keeps_the_task_and_never_says_no_task_was_started() {
    let mut w = World::new("flow_check_outage");
    w.topic_is("pottery");
    w.model_says(&[REPLY, REVIEW_OK, &met(&["a", "b"]), &met(&["a", "b"])]);
    w.start(None);
    w.say("Bakers.");
    w.model_fails("502 SECRET");
    let e = w.try_cmd(ScenarioCmd::Check).err().unwrap();
    let ctx = Ctx {
        global: w.global.clone(),
        project_root: w.project.clone(),
        topic: w.topic.clone(),
        pid: w.pid.get(),
    };
    let r = failure(&ctx, e);
    assert!(
        says(&r, "was not judged") && !says(&r, "no task was started"),
        "{:?}",
        r.notices
    );
    assert!(!says(&r, "SECRET"), "{:?}", r.notices);
    assert_eq!(w.only_run().state, RunState::Active);
}

#[test]
fn a_check_on_gone_files_keeps_its_own_sentence() {
    let w = folder_world("flow_check_gone");
    w.start(Some("demo-pkg"));
    let dir = Store::new(&w.project).tasks_dir().join(w.only_run().run_id);
    std::fs::remove_dir_all(dir).unwrap();
    let e = w.try_cmd(ScenarioCmd::Check).err().unwrap().to_string();
    assert!(e.contains("files for this task are gone"), "{e}");
}

#[test]
fn generation_with_docker_stopped_starts_nothing_for_a_conversation_task() {
    let mut w = World::new("flow_gen_stopped");
    w.topic_is("pottery");
    w.docker_off();
    w.model_says(&[REPLY, REVIEW_OK, &met(&["a", "b"]), &met(&["a", "b"])]);
    let r = w.start(None);
    assert_eq!(w.only_run().env, "conversation", "{:?}", r.notices);
    assert_eq!(w.starter.calls.get(), 0);
    assert!(!w.model_prompt(0).0.contains("cannot run a server now"));
    assert!(!says(&r, "needs none"), "{:?}", r.notices);
}

#[test]
fn a_script_proof_is_what_brings_a_stopped_runtime_up() {
    let mut w = World::new("flow_gen_stopped_box");
    w.topic_is("nginx-ops");
    w.docker_off();
    w.model_says(&[&container_reply()]);
    w.daemon.borrow_mut().check_codes.extend([1, 1, 0]);
    let r = w.start(None);
    assert_eq!(w.only_run().env, "container", "{:?}", r.notices);
    assert_eq!(w.starter.calls.get(), 1);
    assert_eq!(w.running().len(), 1);
}

#[test]
fn esc_during_the_wait_for_the_runtime_stops_generation() {
    let mut w = World::new("flow_esc_wait");
    w.topic_is("nginx-ops");
    w.docker_off();
    w.esc_in_wait.set(true);
    w.model_says(&[&container_reply()]);
    let r = w.start(None);
    assert!(r.turn.is_none() && r.notices.is_empty(), "{:?}", r.notices);
    assert!(w.runs().is_empty());
    let left = library::load_all(&w.global, "nginx-ops").unwrap().pkgs;
    assert!(left.is_empty());
}

#[test]
fn a_runtime_that_will_not_come_up_for_a_proof_ends_in_a_conversation() {
    let mut w = World::new("flow_proof_stuck");
    w.topic_is("nginx-ops");
    w.docker_off();
    w.starter.stuck.set(true);
    w.model_says(&[&container_reply()]);
    let r = w.start(None);
    assert!(says(&r, "could not start it"), "{:?}", r.notices);
    assert!(says(&r, "as a conversation"), "{:?}", r.notices);
    assert_eq!(w.only_run().env, "conversation");
}

#[test]
fn a_shutdown_during_a_start_fallback_installs_nothing() {
    let mut w = World::new("flow_start_stop");
    w.topic_is("pottery");
    w.stopping.set(true);
    w.model_says(&["nonsense", "nonsense", "nonsense", "nonsense"]);
    let r = w.start(None);
    assert!(r.turn.is_none(), "{:?}", r.notices);
    assert!(!says(&r, "as a conversation"), "{:?}", r.notices);
    assert!(w.runs().is_empty());
    let left = library::load_all(&w.global, "pottery").unwrap().pkgs;
    assert!(left.is_empty(), "nothing was installed");
}

#[test]
fn a_stale_server_record_does_not_block_talk_once_docker_is_gone() {
    let w = World::new("flow_stale_talk");
    w.start(Some(BACKUP));
    w.docker_missing();
    w.relaunch();
    assert_eq!(w.only_run().state, RunState::Active);
    w.model_says(&[REPLY, REVIEW_OK, &met(&["a", "b"]), &met(&["a", "b"])]);
    let r = w.cmd(ScenarioCmd::Talk(None));
    assert!(says(&r, "Who is it for?"), "{:?}", r.notices);
    let old = w.runs().into_iter().find(|r| r.package == BACKUP).unwrap();
    assert_eq!(old.state, RunState::Paused);
}

#[test]
fn a_stale_server_record_does_not_block_start_once_docker_is_gone() {
    let w = World::new("flow_stale_start");
    w.start(Some(BACKUP));
    w.docker_missing();
    w.relaunch();
    w.model_says(&[REPLY, REVIEW_OK, &met(&["a", "b"]), &met(&["a", "b"])]);
    let r = w.start(Some("something new"));
    assert!(says(&r, "Who is it for?"), "{:?}", r.notices);
    let dropped = w.cmd(ScenarioCmd::Drop {
        id: Some(
            w.runs()
                .into_iter()
                .find(|r| r.package == BACKUP)
                .unwrap()
                .run_id,
        ),
        confirmed: true,
    });
    assert!(says(&dropped, "Dropped"), "{:?}", dropped.notices);
}

/// Another Usta project has a task running on this machine.
fn busy_elsewhere(w: &World) {
    let mut d = w.daemon.borrow_mut();
    d.boxes
        .insert("usta-otherproj-x".into(), (true, "otherproj".into()));
}

#[test]
fn another_projects_task_does_not_block_a_task_that_needs_no_server() {
    let mut w = World::new("flow_other_project");
    w.topic_is("pottery");
    busy_elsewhere(&w);
    w.model_says(&[REPLY, REVIEW_OK, &met(&["a", "b"]), &met(&["a", "b"])]);
    let r = w.start(None);
    assert_eq!(w.only_run().env, "conversation", "{:?}", r.notices);
    assert!(w.model_prompt(0).0.contains("cannot run a server now"));
    assert!(!says(&r, "needs none"), "{:?}", r.notices);
    assert!(Store::new(&w.project).note_once("no-runtime"), "not spent");
}

#[test]
fn another_projects_task_shows_its_refusal_when_a_subject_needs_a_system() {
    let mut w = World::new("flow_other_needs");
    w.topic_is("nginx-ops");
    busy_elsewhere(&w);
    w.model_says(&[r#"{"kind":"needs_system","why":"nginx must run"}"#]);
    let r = w.start(None);
    assert!(says(&r, "another Usta project"), "{:?}", r.notices);
    assert_eq!(w.only_run().env, "conversation");
}

#[test]
fn a_bare_start_passes_over_a_server_task_while_another_project_runs_one() {
    let w = World::new("flow_other_bare");
    busy_elsewhere(&w);
    w.model_says(&[REPLY, REVIEW_OK, &met(&["a", "b"]), &met(&["a", "b"])]);
    let r = w.start(None);
    assert_eq!(w.only_run().env, "conversation", "{:?}", r.notices);
}

#[test]
fn a_named_server_task_keeps_the_other_projects_refusal() {
    let w = World::new("flow_other_named");
    busy_elsewhere(&w);
    let e = w
        .try_cmd(ScenarioCmd::Start(Some(BACKUP.into())))
        .err()
        .unwrap();
    assert!(e.to_string().contains("another Usta project"), "{e}");
    assert!(w.runs().is_empty());
}

#[test]
fn a_task_that_could_not_be_paused_is_not_reported_as_paused() {
    let w = World::new("flow_pause_fails");
    w.open_the_story();
    w.start(Some(BACKUP));
    w.daemon.borrow_mut().stop_fails = true;
    w.model_says(&[&container_reply()]);
    let r = w.start(Some("something new"));
    assert!(!says(&r, "paused now"), "{:?}", r.notices);
    assert_eq!(w.only_run().state, RunState::Active);
}

#[test]
fn a_task_paused_before_a_failed_start_is_still_reported() {
    let w = World::new("flow_pause_then_fail");
    w.open_the_story();
    w.start(Some(BACKUP));
    busy_elsewhere(&w);
    let r = w.cmd(ScenarioCmd::Start(Some(CONFIG.into())));
    assert!(says(&r, "paused now"), "{:?}", r.notices);
    assert!(says(&r, "another Usta project"), "{:?}", r.notices);
}

#[test]
fn another_live_usta_is_refused_before_any_model_call() {
    let mut w = World::new("flow_other_usta");
    w.start(Some(BACKUP));
    w.pid.set(w.pid.get() + 1);
    w.others_alive.set(true);
    w.topic_is("linux");
    for cmd in [
        ScenarioCmd::Start(Some("something new".into())),
        ScenarioCmd::Talk(None),
    ] {
        let e = w.try_cmd(cmd).err().unwrap().to_string();
        assert!(e.contains("another usta"), "{e}");
    }
    assert_eq!(w.model_calls_since_mark(), 0);
}
