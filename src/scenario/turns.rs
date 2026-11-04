//! The synthesized `[SCENARIO ...]` turns. They are for the mentor (the model),
//! not the learner, but they tell it to keep the infrastructure words out of its replies.

use crate::scenario::package::{Package, Tone};

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
