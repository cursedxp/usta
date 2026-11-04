//! The synthesized `[SCENARIO ...]` turns. They are for the mentor (the model),
//! not the learner, but they tell it to keep the infrastructure words out of its replies.

use crate::anthropic::Message;
use crate::scenario::env::{CONTAINER, FOLDER};
use crate::scenario::package::{Package, Tone};

/// How much of the conversation a conversation task's judge reads.
pub(crate) const SAID_LIMIT: usize = 60;

/// The conversation since the task opened or resumed, labelled, newest last. A
/// compacted history may have lost the marker; then it is simply the latest lines.
/// A user-role message the shell injected, not something the learner said.
fn synthesized(text: &str) -> bool {
    text.starts_with(crate::tokens::CHECKPOINT)
        || text.starts_with("[SCENARIO")
        || text.starts_with("[GAME MODE")
        || text.starts_with("[EXAM MODE")
        || text.starts_with("[SESSION OPENING")
        || crate::file_feedback::is_delivery_turn(text)
}

/// The task id in a `[SCENARIO ...]` marker: the `(id)` after the quoted title.
fn marker_id(text: &str) -> Option<&str> {
    let after = &text[text.find("\" (")? + 3..];
    after.split(')').next()
}

fn marker_kind(text: &str) -> Option<&'static str> {
    ["START", "RESUME", "DONE"]
        .into_iter()
        .find(|k| text.starts_with(&format!("[SCENARIO {k}]")))
}

pub(crate) fn conversation(history: &[Message]) -> Vec<String> {
    let is_marker = |m: &Message| m.role == "user" && marker_kind(&m.text()).is_some();
    let last = history.iter().rposition(|m| {
        m.role == "user" && {
            let t = m.text();
            t.contains("[SCENARIO START]") || t.contains("[SCENARIO RESUME]")
        }
    });
    let from = match last {
        None => 0,
        Some(r) => {
            let id = marker_id(&history[r].text()).map(str::to_string);
            let mut from = r + 1;
            // Back to this task's own start, across its pauses; any other task's marker ends the walk.
            for k in (0..r).rev().filter(|&k| is_marker(&history[k])) {
                let t = history[k].text();
                if marker_id(&t).map(str::to_string) != id || marker_kind(&t) == Some("DONE") {
                    break;
                }
                from = k + 1;
                if marker_kind(&t) == Some("START") {
                    break;
                }
            }
            from
        }
    };
    let lines: Vec<String> = history[from..]
        .iter()
        .filter(|m| m.role == "assistant" || !synthesized(&m.text()))
        .map(|m| {
            format!(
                "{}: {}",
                if m.role == "assistant" {
                    "mentor"
                } else {
                    "learner"
                },
                m.text()
            )
        })
        .collect();
    lines[lines.len().saturating_sub(SAID_LIMIT)..].to_vec()
}

fn workplace(pkg: &Package) -> &'static str {
    match pkg.environment.kind.as_str() {
        CONTAINER => "They work in a terminal on the task's server that you cannot see and will run /scenario check themselves; the shell reports the result.",
        FOLDER => "They work in a task folder in their own editor that you cannot see and will run /scenario check themselves; the shell reports the result.",
        _ => "They work here in this conversation and will run /scenario check when they have answered; the shell reports the result. Do not do the task for them.",
    }
}

fn check_note(pkg: &Package) -> &'static str {
    if pkg.is_rubric() {
        "This was a rubric check, Usta's judgment, which is weaker evidence than a script. At the closing flush it can raise these skills to seen at most, never to settled."
    } else {
        "Treat this as evidence for these skills and reflect it in the curriculum map at the closing flush under the usual evidence rules."
    }
}

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
         {} \
         Say task or server to the learner; never container, image or Docker. \
         Mentor Socratically as usual: never give the fix, ask what they see, hint by the ladder. \
         Reply with ONE short sentence that invites the first step.",
        pkg.title,
        pkg.id,
        pkg.skills.join(", "),
        tone.as_str(),
        workplace(pkg)
    )
}

pub(crate) fn resume_turn(
    pkg: &Package,
    instruction: &str,
    last_check: Option<&str>,
    restarted: bool,
) -> String {
    // Only a container task has a server: nothing else was restarted.
    let survived = match pkg.environment.kind.as_str() {
        FOLDER => "Their files in the task folder are as they left them",
        CONTAINER if restarted => "Their work on the server survived; its services were restarted",
        CONTAINER => "Their work on the server survived; its services did not come back up, possibly because of their unfinished work; they are on the server and can look into it",
        _ => "Usta saved their answers so far and the check will read them, even those this conversation no longer shows",
    };
    format!(
        "[SCENARIO RESUME] The learner came back to the unfinished task \"{}\" ({}). Task: {instruction} \
         Their last check: {}. {survived}. \
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
         {} \
         Say task or server to the learner; never container, image or Docker. \
         Reply with one sentence of acknowledgement, no recap.",
        pkg.title,
        pkg.id,
        pkg.skills.join(", "),
        check_note(pkg)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversation_starts_after_the_last_marker_and_falls_back_to_the_tail() {
        let h = vec![
            Message::user("old talk"),
            Message::user("[SCENARIO START] x"),
            Message::assistant_raw(serde_json::json!("hello")),
            Message::user("my reply"),
        ];
        assert_eq!(conversation(&h), vec!["mentor: hello", "learner: my reply"]);
        let plain = vec![
            Message::user("a"),
            Message::assistant_raw(serde_json::json!("b")),
        ];
        assert_eq!(conversation(&plain), vec!["learner: a", "mentor: b"]);
        let long: Vec<Message> = (0..70).map(|i| Message::user(format!("m{i}"))).collect();
        let c = conversation(&long);
        assert_eq!(c.len(), 60);
        assert_eq!(c.last().unwrap(), "learner: m69");
    }

    #[test]
    fn synthesized_user_messages_are_not_the_learner_speaking() {
        let h = vec![
            Message::user("[SCENARIO START] x"),
            Message::user(crate::tokens::CHECKPOINT),
            Message::user("[Files changed]\nsrc/a.rs"),
            Message::user("[File saved: a.rs]\nbody"),
            Message::user("[GAME MODE ON] go"),
            Message::user("[EXAM MODE — MOCK EXAM]\nx"),
            Message::user("[SCENARIO DONE] y"),
            Message::user("[SESSION OPENING — RECALL DRILL]\nx"),
            Message::user("I think it is the port"),
        ];
        assert_eq!(conversation(&h), vec!["learner: I think it is the port"]);
    }

    fn mk(id: &str, kind: &str) -> Message {
        Message::user(format!(
            "[SCENARIO {kind}] The learner x the task \"T\" ({id}). Task: y"
        ))
    }

    #[test]
    fn an_answer_before_a_pause_still_counts_after_resume() {
        let h = vec![
            mk("t-a", "START"),
            Message::assistant_raw(serde_json::json!("question?")),
            Message::user("my answer"),
            mk("t-a", "RESUME"),
            Message::assistant_raw(serde_json::json!("welcome back")),
        ];
        let c = conversation(&h);
        assert!(c.contains(&"learner: my answer".to_string()), "{c:?}");
    }

    #[test]
    fn another_tasks_talk_never_counts() {
        let h = vec![
            mk("t-b", "START"),
            Message::user("b answer"),
            mk("t-a", "START"),
            Message::user("a answer"),
            mk("t-a", "RESUME"),
        ];
        assert_eq!(conversation(&h), vec!["learner: a answer"]);
        let h = vec![
            mk("t-a", "START"),
            Message::user("a answer"),
            mk("t-b", "START"),
            Message::user("b answer"),
            mk("t-a", "RESUME"),
        ];
        assert!(conversation(&h).is_empty(), "{:?}", conversation(&h));
    }
}
