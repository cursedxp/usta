//! The synthesized `[SCENARIO ...]` turns. They are for the mentor (the model),
//! not the learner, but they tell it to keep the infrastructure words out of its replies.

use crate::anthropic::Message;
use crate::scenario::package::{Package, Tone};

/// How much of the conversation a conversation task's judge reads.
const SAID_LIMIT: usize = 30;

/// The conversation since the task opened or resumed, labelled, newest last. A
/// compacted history may have lost the marker; then it is simply the latest lines.
/// A user-role message the shell injected, not something the learner said.
fn synthesized(text: &str) -> bool {
    text.starts_with(crate::tokens::CHECKPOINT)
        || text.starts_with("[SCENARIO")
        || text.starts_with("[GAME MODE")
        || text.starts_with("[EXAM MODE")
        || crate::file_feedback::is_delivery_turn(text)
}

pub(crate) fn conversation(history: &[Message]) -> Vec<String> {
    let from = history
        .iter()
        .rposition(|m| {
            m.role == "user" && {
                let t = m.text();
                t.contains("[SCENARIO START]") || t.contains("[SCENARIO RESUME]")
            }
        })
        .map_or(0, |i| i + 1);
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

pub(crate) fn resume_turn(
    pkg: &Package,
    instruction: &str,
    last_check: Option<&str>,
    restarted: bool,
) -> String {
    let services = if restarted {
        "its services were restarted"
    } else {
        "its services did not come back up, possibly because of their unfinished work; they are on the server and can look into it"
    };
    format!(
        "[SCENARIO RESUME] The learner came back to the unfinished task \"{}\" ({}). Task: {instruction} \
         Their last check: {}. Their work on the server survived; {services}. \
         Say task or server to the learner; never container, image or Docker. \
         In two sentences, say where they left off if this conversation shows it, otherwise restate the task. \
         Do not give the fix.",
        pkg.title,
        pkg.id,
        last_check.unwrap_or("none yet")
    )
}

pub(crate) fn resume_files_turn(
    pkg: &Package,
    instruction: &str,
    last_check: Option<&str>,
) -> String {
    format!(
        "[SCENARIO RESUME] The learner came back to the unfinished task \"{}\" ({}). Task: {instruction} \
         Their last check: {}. Their files in the task folder are as they left them. \
         In two sentences, say where they left off if this conversation shows it, otherwise restate the task. \
         Do not give the answer.",
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
        let long: Vec<Message> = (0..40).map(|i| Message::user(format!("m{i}"))).collect();
        let c = conversation(&long);
        assert_eq!(c.len(), 30);
        assert_eq!(c.last().unwrap(), "learner: m39");
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
            Message::user("I think it is the port"),
        ];
        assert_eq!(conversation(&h), vec!["learner: I think it is the port"]);
    }
}
