//! Any subject, on demand. Ask the model for a package, stage it, lint it, prove
//! it; a failure goes back for at most `FEEDBACK_ROUNDS` rounds, then it is
//! discarded. Blocking; runs off the UI task. Staging is never loaded, so a crash
//! leaves nothing a learner can reach. Only a package that passed its proof is
//! installed into the library, with one named exception: `fallback_talk`, whose
//! fixed text holds no model-written file and so has nothing to prove.

use std::path::Path;
use std::time::{Duration, Instant};

use anyhow::Result;

use crate::scenario::draft::{self, Draft};
use crate::scenario::env::{internal, CONVERSATION};
use crate::scenario::ledger::Ledger;
use crate::scenario::library;
use crate::scenario::lint::learner_word_problems;
use crate::scenario::model::{model_failed, Cancel, Model};
use crate::scenario::package::Package;
use crate::scenario::prompt::{self, Brief};
use crate::scenario::validate::{Proof, Prover};

pub(crate) const FEEDBACK_ROUNDS: usize = 3;
/// Soft limit: checked between rounds, so one slow round can run past it.
const BUDGET: Duration = Duration::from_secs(30 * 60);

pub(crate) const SAY_WRITING: &str = "Writing a new task for you; this can take a few minutes…";
pub(crate) const SAY_RUBRIC: &str = "Checking that the task can be judged fairly…";
pub(crate) const SAY_PROVING: &str = "Setting up the task's server and testing that it can be solved; a new server can take a few minutes…";

fn say_again(round: usize) -> String {
    format!(
        "The test found a problem, so Usta is fixing the task (try {} of {})…",
        round + 1,
        FEEDBACK_ROUNDS + 1
    )
}

pub(crate) struct Job<'a> {
    pub brief: &'a Brief,
    pub global: &'a Path,
    pub stamp: &'a str,
    pub model: &'a dyn Model,
    pub say: &'a dyn Fn(&str),
    pub ledger: &'a Ledger,
    pub cancel: &'a Cancel,
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)] // one value per generation; the interface hands over the Package itself
pub(crate) enum Outcome {
    Ready(Package),
    NeedsSystem(String),
    /// Every problem met along the way, for the log.
    Failed(Vec<String>),
    /// Esc. The draft is gone; the line for the learner was printed when they pressed it.
    Cancelled,
}

pub(crate) fn story_problems(p: &Package, l: &Ledger) -> Vec<String> {
    let open = l.open_ids();
    let mut out = Vec::new();
    if let (Some(mine), Some(world)) = (&p.story.company, &l.company) {
        if mine != world {
            out.push(format!("story.company must be {world}"));
        }
    }
    for id in p
        .story
        .needs
        .iter()
        .chain(p.story.closes.iter())
        .filter(|id| !open.contains(id))
    {
        out.push(format!(
            "thread '{id}' is not open; use only the open threads given"
        ));
    }
    for t in p
        .story
        .opens
        .iter()
        .filter(|t| l.threads.iter().any(|x| x.id == t.id))
    {
        out.push(format!("thread '{}' already exists; use a new id", t.id));
    }
    out
}

/// The model picks an environment kind; this machine and the learner's wishes decide
/// whether it may stand.
fn static_problems(p: &Package, job: &Job) -> Vec<String> {
    let b = job.brief;
    let allow = format!("{} {}", b.topic, b.request.as_deref().unwrap_or(""));
    let mut out = p.lint();
    // The word check opens files by name: only names that lint has cleared.
    if out.is_empty() {
        out.extend(learner_word_problems(p, &allow));
    }
    out.extend(story_problems(p, job.ledger));
    if p.needs_runtime() && !b.runtime {
        out.push(
            "this computer cannot run a server now; use a folder rubric or a conversation task"
                .into(),
        );
    }
    if b.talk_only && p.environment.kind != CONVERSATION {
        out.push("use a conversation task".into());
    }
    out
}

/// One attempt. `Ok(None)` = a problem list was stored for the next round.
fn attempt(
    job: &Job,
    prover: &mut dyn Prover,
    reply: &str,
    round: usize,
    id: &str,
    problems: &mut Vec<String>,
) -> Result<Option<Outcome>> {
    let staged = library::staging_root(job.global).join(id);
    let parsed = match draft::read(reply) {
        Ok(Draft::NeedsSystem(why)) => return Ok(Some(Outcome::NeedsSystem(why))),
        Ok(Draft::Package(r)) => r,
        Err(p) => {
            *problems = p;
            return Ok(None);
        }
    };
    if let Err(p) = draft::stage(&parsed, &staged, id, &job.brief.topic, round as u32 + 1) {
        *problems = p;
        return Ok(None);
    }
    let pkg = match Package::load(&staged) {
        Ok(p) => p,
        Err(e) => {
            *problems = vec![format!("{e:#}")];
            return Ok(None);
        }
    };
    let found = static_problems(&pkg, job);
    if !found.is_empty() {
        *problems = found;
        return Ok(None);
    }
    if job.cancel.requested() {
        return Ok(Some(Outcome::Cancelled));
    }
    let proof = match prover.prove(&pkg, round) {
        Ok(p) => p,
        Err(e) => {
            prover.discard(&pkg);
            return Err(e);
        }
    };
    match proof {
        Proof::Passed if job.cancel.seal() => {
            prover.discard(&pkg);
            Ok(Some(Outcome::Cancelled))
        }
        Proof::Passed => match library::install(job.global, &staged, id) {
            Ok(p) => Ok(Some(Outcome::Ready(p))),
            Err(e) => {
                prover.discard(&pkg);
                Err(e)
            }
        },
        Proof::Problems(p) => {
            *problems = p;
            Ok(None)
        }
    }
}

/// The stamp has one-second resolution, and an install must never meet an existing
/// package. `create_dir` claims the staging folder: only one generation can win a name.
fn free_id(global: &Path, base: String) -> Result<String> {
    let staging = library::staging_root(global);
    let cannot = |e: std::io::Error| internal(format!("cannot stage a task: {e}"));
    std::fs::create_dir_all(&staging).map_err(cannot)?;
    let numbered = (2..).map(|n| format!("{base}-{n}"));
    for id in std::iter::once(base.clone()).chain(numbered) {
        if library::library_root(global).join(&id).exists() {
            continue;
        }
        match std::fs::create_dir(staging.join(&id)) {
            Ok(()) => return Ok(id),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(cannot(e)),
        }
    }
    unreachable!("an endless list of names")
}

/// `Err` = the model could not be reached (or the prover broke); no round is spent on it.
pub(crate) fn run(job: &Job, prover: &mut dyn Prover) -> Result<Outcome> {
    let id = free_id(job.global, format!("gen-{}-{}", job.brief.topic, job.stamp))?;
    let staged = library::staging_root(job.global).join(&id);
    let _open = job.cancel.open();
    let out = rounds(job, prover, &id, BUDGET);
    // After an install the folder is already gone; on every other way out it is the draft.
    let _ = std::fs::remove_dir_all(&staged);
    // An Esc during the last proof, or one followed by an error, still ends as a cancel.
    // `seal` cleared the state on the install path, so a `Ready` is never overridden.
    if job.cancel.requested() {
        return Ok(Outcome::Cancelled);
    }
    out
}

/// `budget` is checked between rounds only: a round already under way is not interrupted.
fn rounds(job: &Job, prover: &mut dyn Prover, id: &str, budget: Duration) -> Result<Outcome> {
    let system = prompt::generate_system(job.brief);
    let begun = Instant::now();
    let mut log = Vec::new();
    let mut problems = Vec::new();
    let mut last = String::new();
    for round in 0..=FEEDBACK_ROUNDS {
        if job.cancel.requested() {
            return Ok(Outcome::Cancelled);
        }
        if crate::shutdown::is_requested() || begun.elapsed() >= budget {
            log.push("stopped before the last round: shutdown or time budget".to_string());
            break;
        }
        let text = if round == 0 {
            (job.say)(SAY_WRITING);
            prompt::request(job.brief)
        } else {
            (job.say)(&say_again(round));
            prompt::retry(job.brief, &last, &problems)
        };
        let reply = job.model.ask(&system, &text).map_err(model_failed)?;
        if job.cancel.requested() {
            return Ok(Outcome::Cancelled);
        }
        if reply.truncated {
            problems = vec!["the reply was cut off; make the package smaller".into()];
        } else if let Some(done) = attempt(job, prover, &reply.text, round, id, &mut problems)? {
            return Ok(done);
        }
        last = reply.text;
        log.extend(problems.iter().map(|p| format!("round {round}: {p}")));
    }
    Ok(Outcome::Failed(log))
}

const FALLBACK_INSTRUCTION: &str =
    "Tell Usta here, in your own words, how you would approach this, and why:";

/// The one install that is not proven: a conversation task from fixed text, so a topic
/// is never left with nothing, even when every generated package failed. No
/// model-written file in it. A blank skill falls back to the topic.
pub(crate) fn fallback_talk(
    global: &Path,
    stamp: &str,
    topic: &str,
    skill: &str,
) -> Result<Package> {
    let id = free_id(global, format!("gen-{topic}-talk-{stamp}"))?;
    let staged = library::staging_root(global).join(&id);
    let line = skill.split_whitespace().collect::<Vec<_>>().join(" ");
    let line = if line.is_empty() {
        topic.to_string()
    } else {
        line
    };
    let keyword = toml::Value::String(line.to_lowercase());
    let title = toml::Value::String(format!("Talk it through: {line}"));
    let task = format!(
        "id = \"{id}\"\ntopic = \"{topic}\"\ntitle = {title}\nversion = 1\nskills = [{keyword}]\n\n[environment]\nkind = \"conversation\"\n\n[check]\nkind = \"rubric\"\nwhy_rubric = \"no task could be proven, so this is a conversation about the skill\"\n\n[[check.rubric]]\nid = \"approach\"\ntext = \"Describes concrete steps for this skill in their own words\"\n\n[[check.rubric]]\nid = \"reason\"\ntext = \"Gives a reason for at least one choice\"\n"
    );
    let write = || -> Result<Package> {
        for (rel, text) in [
            ("task.toml", task),
            ("instruction.md", format!("{FALLBACK_INSTRUCTION} {line}\n")),
            (
                "tone/adventure.md",
                format!("## open\nA colleague stops by: \"{line}. How would you go about it?\"\n\n## close\nThey nod and go back to work.\n"),
            ),
            (
                "solution/answer.md",
                "Concrete steps in order, and a reason for the first choice.\n".to_string(),
            ),
        ] {
            let path = staged.join(rel);
            std::fs::create_dir_all(path.parent().expect("file has a parent"))?;
            std::fs::write(path, text)?;
        }
        library::install(global, &staged, &id)
    };
    let out = write();
    let _ = std::fs::remove_dir_all(&staged);
    out.map_err(|e| internal(format!("{e:#}")))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    use super::*;
    use crate::scenario::draft::tests::REPLY;
    use crate::scenario::ledger::Thread;
    use crate::scenario::model::fake::FakeModel;
    use crate::scenario::package::tests::tmp;
    use crate::scenario::package::{StorySpec, ThreadSpec};

    struct Script {
        proofs: VecDeque<Proof>,
        rounds: Vec<usize>,
        cancel_while_proving: Option<Arc<Cancel>>,
        cancel_from_round: usize,
        discarded: usize,
        fail: bool,
    }

    impl Prover for Script {
        fn prove(&mut self, _: &Package, round: usize) -> Result<Proof> {
            self.rounds.push(round);
            if let Some(c) = self
                .cancel_while_proving
                .as_ref()
                .filter(|_| round >= self.cancel_from_round)
            {
                c.request();
            }
            if self.fail {
                anyhow::bail!("the practice server software stopped answering");
            }
            Ok(self.proofs.pop_front().unwrap_or(Proof::Passed))
        }
        fn discard(&mut self, _: &Package) {
            self.discarded += 1;
        }
    }

    fn prover(p: Vec<Proof>) -> Script {
        Script {
            proofs: p.into(),
            rounds: vec![],
            cancel_while_proving: None,
            cancel_from_round: 0,
            discarded: 0,
            fail: false,
        }
    }

    fn brief() -> Brief {
        Brief {
            topic: "marketing".into(),
            request: None,
            map: String::new(),
            level: String::new(),
            world: String::new(),
            finished: vec![],
            runtime: true,
            talk_only: false,
        }
    }

    fn go(
        m: &FakeModel,
        g: &Path,
        p: &mut Script,
        b: &Brief,
        l: &Ledger,
        said: &std::cell::RefCell<Vec<String>>,
    ) -> Result<Outcome> {
        let say = |s: &str| said.borrow_mut().push(s.to_string());
        let cancel = Cancel::default();
        run(
            &Job {
                brief: b,
                global: g,
                stamp: "20261104",
                model: m,
                say: &say,
                ledger: l,
                cancel: &cancel,
            },
            p,
        )
    }

    fn nothing_left(g: &Path) {
        assert!(library::load_all(g, "marketing").unwrap().pkgs.is_empty());
        let staged = std::fs::read_dir(library::staging_root(g)).map_or(0, |d| d.count());
        assert_eq!(staged, 0, "the draft is discarded");
    }

    /// `REPLY` as a container task: the same JSON, with the files a container needs.
    fn container_reply() -> String {
        let mut v: serde_json::Value = serde_json::from_str(REPLY).unwrap();
        let t = v["task_toml"]
            .as_str()
            .unwrap()
            .replace("kind = \"conversation\"", "kind = \"container\"");
        let t = t.split("[check]").next().unwrap().to_string()
            + "[check]\nkind = \"script\"\nscript = \"check.sh\"\n";
        v["task_toml"] = t.into();
        let files = v["files"].as_array_mut().unwrap();
        files.retain(|f| f["path"] != "solution/answer.md");
        for (p, c) in [
            ("environment/Dockerfile", "FROM debian:12-slim\n"),
            ("solution/solve.sh", "true\n"),
            ("tests/check.sh", "exit 0\n"),
        ] {
            files.push(serde_json::json!({"path": p, "content": c}));
        }
        v.to_string()
    }

    #[test]
    fn esc_during_the_model_call_ends_after_that_stage_and_discards_the_draft() {
        let g = tmp("gen_esc_model");
        let m = FakeModel::with(&[REPLY, REPLY]);
        let cancel = Cancel::default();
        let say = |s: &str| {
            if s == SAY_WRITING {
                assert!(cancel.request());
            }
        };
        let b = brief();
        let l = Ledger::default();
        let mut p = prover(vec![]);
        let job = Job {
            brief: &b,
            global: &g,
            stamp: "1",
            model: &m,
            say: &say,
            ledger: &l,
            cancel: &cancel,
        };
        assert!(matches!(run(&job, &mut p).unwrap(), Outcome::Cancelled));
        assert_eq!(
            m.prompts.lock().unwrap().len(),
            1,
            "the stage ended, no new one began"
        );
        assert!(p.rounds.is_empty());
        nothing_left(&g);
        assert!(!cancel.request(), "the window is closed again");
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn esc_during_a_passing_proof_still_installs_nothing() {
        let g = tmp("gen_esc_proof");
        let m = FakeModel::with(&[REPLY]);
        let cancel = Arc::new(Cancel::default());
        let say = |_: &str| {};
        let b = brief();
        let l = Ledger::default();
        let mut p = prover(vec![]);
        p.cancel_while_proving = Some(cancel.clone());
        let job = Job {
            brief: &b,
            global: &g,
            stamp: "1",
            model: &m,
            say: &say,
            ledger: &l,
            cancel: &cancel,
        };
        assert!(matches!(run(&job, &mut p).unwrap(), Outcome::Cancelled));
        assert_eq!(p.discarded, 1);
        nothing_left(&g);
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn a_passing_package_enters_the_library_and_loads_clean() {
        let g = tmp("gen_ok");
        let m = FakeModel::with(&[REPLY]);
        let said = Default::default();
        let Outcome::Ready(p) = go(
            &m,
            &g,
            &mut prover(vec![]),
            &brief(),
            &Ledger::default(),
            &said,
        )
        .unwrap() else {
            panic!()
        };
        assert_eq!(p.id, "gen-marketing-20261104");
        assert_eq!(library::load_all(&g, "marketing").unwrap().pkgs.len(), 1);
        assert!(!g.join("scenarios/staging").join(&p.id).exists());
        assert!(!said.borrow().is_empty());
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn two_runs_with_the_same_stamp_get_different_ids() {
        let g = tmp("gen_same_stamp");
        let said = Default::default();
        let ids: Vec<String> = (0..2)
            .map(|_| {
                let m = FakeModel::with(&[REPLY]);
                let Outcome::Ready(p) = go(
                    &m,
                    &g,
                    &mut prover(vec![]),
                    &brief(),
                    &Ledger::default(),
                    &said,
                )
                .unwrap() else {
                    panic!()
                };
                p.id
            })
            .collect();
        assert_ne!(ids[0], ids[1]);
        assert_eq!(library::load_all(&g, "marketing").unwrap().pkgs.len(), 2);
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn failures_go_back_for_at_most_three_rounds_and_nothing_enters_the_library() {
        let g = tmp("gen_fail");
        let m = FakeModel::with(&[REPLY, REPLY, REPLY, REPLY, REPLY]);
        let bad = || Proof::Problems(vec!["the check already passes".into()]);
        let mut p = prover(vec![bad(), bad(), bad(), bad(), bad()]);
        let said = Default::default();
        let Outcome::Failed(log) = go(&m, &g, &mut p, &brief(), &Ledger::default(), &said).unwrap()
        else {
            panic!()
        };
        assert_eq!(m.prompts.lock().unwrap().len(), 1 + FEEDBACK_ROUNDS);
        assert_eq!(p.rounds, vec![0, 1, 2, 3]);
        assert!(log.concat().contains("already passes"));
        assert!(
            m.prompts.lock().unwrap()[1].1.contains("already passes"),
            "feedback reaches the model"
        );
        assert!(library::load_all(&g, "marketing").unwrap().pkgs.is_empty());
        nothing_left(&g);
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn a_second_attempt_can_pass_and_its_version_differs() {
        let g = tmp("gen_second");
        let m = FakeModel::with(&[REPLY, REPLY]);
        let mut p = prover(vec![Proof::Problems(vec!["x".into()]), Proof::Passed]);
        let said = Default::default();
        let Outcome::Ready(pkg) = go(&m, &g, &mut p, &brief(), &Ledger::default(), &said).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            pkg.version, 2,
            "version = round + 1, so a stale image is never reused"
        );
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn lint_failures_never_reach_the_prover() {
        let g = tmp("gen_lint");
        let broken = REPLY.replace("\"solution/answer.md\"", "\"solution/other.md\"");
        let m = FakeModel::with(&[&broken, REPLY]);
        let mut p = prover(vec![]);
        let said = Default::default();
        assert!(matches!(
            go(&m, &g, &mut p, &brief(), &Ledger::default(), &said).unwrap(),
            Outcome::Ready(_)
        ));
        assert_eq!(p.rounds, vec![1]);
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn learner_text_naming_the_machinery_is_sent_back_unless_the_subject_is_that() {
        let g = tmp("gen_words");
        let wordy = REPLY.replace("Who is it for?", "Log in to the Docker container.");
        let m = FakeModel::with(&[&wordy, REPLY]);
        let mut p = prover(vec![]);
        let said = Default::default();
        go(&m, &g, &mut p, &brief(), &Ledger::default(), &said).unwrap();
        assert!(m.prompts.lock().unwrap()[1].1.contains("must not say"));
        let m = FakeModel::with(&[&wordy]);
        let mut b = brief();
        b.topic = "docker-basics".into();
        let mut p = prover(vec![]);
        assert!(matches!(
            go(&m, &g, &mut p, &b, &Ledger::default(), &said).unwrap(),
            Outcome::Ready(_)
        ));
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn a_runtime_kind_without_a_runtime_is_sent_back_unproven() {
        let g = tmp("gen_kind");
        let mut b = brief();
        b.runtime = false;
        let m = FakeModel::with(&[&container_reply(), REPLY]);
        let mut p = prover(vec![]);
        let said = Default::default();
        go(&m, &g, &mut p, &b, &Ledger::default(), &said).unwrap();
        assert_eq!(p.rounds, vec![1]);
        assert!(m.prompts.lock().unwrap()[1]
            .1
            .contains("cannot run a server"));
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn talk_only_sends_back_anything_but_a_conversation() {
        let g = tmp("gen_talk_only");
        let mut b = brief();
        b.talk_only = true;
        let m = FakeModel::with(&[&container_reply(), REPLY]);
        let mut p = prover(vec![]);
        let said = Default::default();
        go(&m, &g, &mut p, &b, &Ledger::default(), &said).unwrap();
        assert_eq!(p.rounds, vec![1]);
        assert!(m.prompts.lock().unwrap()[1]
            .1
            .contains("use a conversation task"));
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn story_references_must_fit_the_ledger() {
        let root = tmp("gen_story");
        let mut p = Package::load(&crate::scenario::package::tests::write_pkg(&root)).unwrap();
        let mut l = Ledger {
            company: Some("Shopwell".into()),
            ..Ledger::default()
        };
        l.threads.push(Thread {
            id: "open-one".into(),
            text: "t".into(),
            open: true,
        });
        l.threads.push(Thread {
            id: "shut".into(),
            text: "t".into(),
            open: false,
        });
        p.story.company = Some("Other Inc".into());
        p.story.needs = vec!["nope".into()];
        p.story.closes = vec!["shut".into()];
        p.story.opens = vec![ThreadSpec {
            id: "open-one".into(),
            text: "again".into(),
        }];
        let e = story_problems(&p, &l).join("\n");
        for k in ["Shopwell", "nope", "shut", "open-one"] {
            assert!(e.contains(k), "{k}: {e}");
        }
        p.story = StorySpec {
            company: Some("Shopwell".into()),
            needs: vec!["open-one".into()],
            ..StorySpec::default()
        };
        assert!(story_problems(&p, &l).is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_model_that_cannot_be_reached_burns_no_rounds() {
        let g = tmp("gen_down");
        let m = FakeModel::default();
        m.replies
            .lock()
            .unwrap()
            .push_back(Err("rate limited".into()));
        let mut p = prover(vec![]);
        let said = Default::default();
        assert!(go(&m, &g, &mut p, &brief(), &Ledger::default(), &said).is_err());
        assert_eq!(m.prompts.lock().unwrap().len(), 1);
        assert!(p.rounds.is_empty());
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn a_prover_error_discards_the_image_and_the_draft_and_is_not_a_round() {
        let g = tmp("gen_prover_err");
        let m = FakeModel::with(&[REPLY, REPLY]);
        let mut p = prover(vec![]);
        p.fail = true;
        let said = Default::default();
        assert!(go(&m, &g, &mut p, &brief(), &Ledger::default(), &said).is_err());
        assert_eq!(
            m.prompts.lock().unwrap().len(),
            1,
            "no feedback round for an error"
        );
        assert_eq!(p.discarded, 1, "the image is discarded");
        nothing_left(&g);
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn a_needs_system_answer_ends_the_loop() {
        let g = tmp("gen_sys");
        let m = FakeModel::with(&[r#"{"kind":"needs_system","why":"nginx must run"}"#]);
        let mut p = prover(vec![]);
        let said = Default::default();
        let Outcome::NeedsSystem(w) =
            go(&m, &g, &mut p, &brief(), &Ledger::default(), &said).unwrap()
        else {
            panic!()
        };
        assert_eq!(w, "nginx must run");
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn two_generations_in_one_second_never_share_an_id() {
        let g = tmp("gen_claim");
        let ids: Vec<String> = std::thread::scope(|s| {
            let claims: Vec<_> = (0..8)
                .map(|_| s.spawn(|| free_id(&g, "gen-x-1".to_string()).unwrap()))
                .collect();
            claims.into_iter().map(|c| c.join().unwrap()).collect()
        });
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), ids.len(), "{ids:?}");
        for id in &ids {
            assert!(
                library::staging_root(&g).join(id).is_dir(),
                "{id} is claimed"
            );
        }
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn a_fallback_that_cannot_write_is_an_internal_error() {
        let g = tmp("gen_fallback_io");
        std::fs::create_dir_all(g.join("scenarios")).unwrap();
        std::fs::write(g.join("scenarios/staging"), "a file where a folder belongs").unwrap();
        let e = fallback_talk(&g, "1", "marketing", "price").unwrap_err();
        assert!(e.downcast_ref::<crate::scenario::env::Internal>().is_some());
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn the_fixed_talk_task_lints_clean_and_quotes_the_skill_safely() {
        let g = tmp("gen_fallback");
        let p = fallback_talk(&g, "20261104", "marketing", "price \"anchoring\"\n[evil]").unwrap();
        assert_eq!(p.environment.kind, "conversation");
        assert_eq!(p.lint(), Vec::<String>::new());
        assert!(!p.needs_runtime());
        let again = fallback_talk(&g, "20261104", "marketing", "price").unwrap();
        assert_ne!(
            again.id, p.id,
            "a second fallback never meets an existing package"
        );
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn esc_during_the_last_proof_is_cancelled_not_failed() {
        let g = tmp("gen_esc_last");
        let m = FakeModel::with(&[REPLY, REPLY, REPLY, REPLY]);
        let cancel = Arc::new(Cancel::default());
        let say = |_: &str| {};
        let b = brief();
        let l = Ledger::default();
        let bad = || Proof::Problems(vec!["x".into()]);
        let mut p = prover(vec![bad(), bad(), bad(), bad()]);
        p.cancel_while_proving = Some(cancel.clone());
        p.cancel_from_round = FEEDBACK_ROUNDS;
        let job = Job {
            brief: &b,
            global: &g,
            stamp: "1",
            model: &m,
            say: &say,
            ledger: &l,
            cancel: &cancel,
        };
        assert!(matches!(run(&job, &mut p).unwrap(), Outcome::Cancelled));
        nothing_left(&g);
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn esc_followed_by_a_prover_error_is_cancelled() {
        let g = tmp("gen_esc_err");
        let m = FakeModel::with(&[REPLY]);
        let cancel = Arc::new(Cancel::default());
        let say = |_: &str| {};
        let b = brief();
        let l = Ledger::default();
        let mut p = prover(vec![]);
        p.cancel_while_proving = Some(cancel.clone());
        p.fail = true;
        let job = Job {
            brief: &b,
            global: &g,
            stamp: "1",
            model: &m,
            say: &say,
            ledger: &l,
            cancel: &cancel,
        };
        assert!(matches!(run(&job, &mut p).unwrap(), Outcome::Cancelled));
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn a_model_outage_is_typed_so_it_is_not_taken_for_a_dead_runtime() {
        let g = tmp("gen_down_typed");
        let m = FakeModel::default();
        m.replies
            .lock()
            .unwrap()
            .push_back(Err("rate limited".into()));
        let said = Default::default();
        let e = go(
            &m,
            &g,
            &mut prover(vec![]),
            &brief(),
            &Ledger::default(),
            &said,
        )
        .unwrap_err();
        assert!(
            e.downcast_ref::<crate::scenario::model::ModelFailed>()
                .is_some(),
            "{e:#}"
        );
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn a_blank_skill_falls_back_to_the_topic_and_still_loads_clean() {
        let g = tmp("gen_blank_skill");
        let p = fallback_talk(&g, "1", "marketing", " \n\t ").unwrap();
        assert!(p.title.contains("marketing"), "{}", p.title);
        assert!(
            p.skills.iter().all(|k| !k.trim().is_empty()),
            "{:?}",
            p.skills
        );
        assert_eq!(p.lint(), Vec::<String>::new());
        assert_eq!(library::load_all(&g, "marketing").unwrap().pkgs.len(), 1);
        assert!(library::load_all(&g, "marketing").unwrap().bad.is_empty());
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn a_cut_off_reply_is_feedback_and_never_parsed() {
        let g = tmp("gen_truncated");
        let m = FakeModel::default();
        m.replies.lock().unwrap().push_back(Ok(REPLY.to_string()));
        m.replies.lock().unwrap().push_back(Ok(REPLY.to_string()));
        let mut p = prover(vec![]);
        let truncating = Truncating {
            inner: &m,
            left: std::sync::atomic::AtomicUsize::new(1),
        };
        let say = |_: &str| {};
        let (b, l, cancel) = (brief(), Ledger::default(), Cancel::default());
        let job = Job {
            brief: &b,
            global: &g,
            stamp: "1",
            model: &truncating,
            say: &say,
            ledger: &l,
            cancel: &cancel,
        };
        assert!(matches!(run(&job, &mut p).unwrap(), Outcome::Ready(_)));
        assert_eq!(
            p.rounds,
            vec![1],
            "the cut-off reply never reached the prover"
        );
        assert!(m.prompts.lock().unwrap()[1].1.contains("cut off"));
        let _ = std::fs::remove_dir_all(g);
    }

    /// Marks the first `left` replies of the inner model as cut off.
    struct Truncating<'a> {
        inner: &'a FakeModel,
        left: std::sync::atomic::AtomicUsize,
    }

    impl Model for Truncating<'_> {
        fn ask(&self, s: &str, p: &str) -> Result<crate::scenario::model::ModelText> {
            let mut r = self.inner.ask(s, p)?;
            let left = self.left.load(Ordering::SeqCst);
            r.truncated = left > 0;
            self.left.store(left.saturating_sub(1), Ordering::SeqCst);
            Ok(r)
        }
    }

    #[test]
    fn an_exhausted_budget_stops_before_another_round_and_says_so() {
        let g = tmp("gen_budget");
        let m = FakeModel::with(&[REPLY]);
        let mut p = prover(vec![]);
        let (b, l, cancel) = (brief(), Ledger::default(), Cancel::default());
        let say = |_: &str| {};
        let job = Job {
            brief: &b,
            global: &g,
            stamp: "1",
            model: &m,
            say: &say,
            ledger: &l,
            cancel: &cancel,
        };
        let Outcome::Failed(log) = rounds(&job, &mut p, "gen-x-1", Duration::ZERO).unwrap() else {
            panic!()
        };
        assert!(log.concat().contains("time budget"), "{log:?}");
        assert!(m.prompts.lock().unwrap().is_empty());
        let _ = std::fs::remove_dir_all(g);
    }

    #[test]
    fn a_failed_install_discards_the_image_and_the_draft() {
        let g = tmp("gen_install_fail");
        // A file where the library folder should be: the install cannot create it.
        std::fs::create_dir_all(g.join("scenarios")).unwrap();
        std::fs::write(library::library_root(&g), "x").unwrap();
        let m = FakeModel::with(&[REPLY]);
        let mut p = prover(vec![]);
        let said = Default::default();
        assert!(go(&m, &g, &mut p, &brief(), &Ledger::default(), &said).is_err());
        assert_eq!(p.discarded, 1);
        let staged = std::fs::read_dir(library::staging_root(&g)).map_or(0, |d| d.count());
        assert_eq!(staged, 0);
        let _ = std::fs::remove_dir_all(g);
    }
}
