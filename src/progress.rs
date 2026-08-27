//! Persistent memory: at session close we have Usta summarize the session and
//! rewrite `.usta/learner/progress/<topic>.md` with its FULL content.
//! The next session loads this file into the system prompt (brain.rs) → Usta
//! doesn't re-explain what it already knows, it targets the gaps. Implements SPEC §9.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::tokens;

/// Progress file path for a topic: `<project>/.usta/learner/progress/<topic>.md`.
pub fn progress_path(project_root: &Path, topic: &str) -> PathBuf {
    project_root
        .join(".usta/learner/progress")
        .join(format!("{topic}.md"))
}

/// Topic-specific approach file: `.usta/approaches/<topic>.md`.
pub fn approach_path(project_root: &Path, topic: &str) -> PathBuf {
    project_root
        .join(".usta/approaches")
        .join(format!("{topic}.md"))
}

/// The topic's curriculum map: `.usta/learner/curriculum/<topic>.md`.
pub fn curriculum_path(project_root: &Path, topic: &str) -> PathBuf {
    project_root
        .join(".usta/learner/curriculum")
        .join(format!("{topic}.md"))
}

/// User-facing project definition: `<project>/mentor/PROJECT.md`.
/// Lives OUTSIDE `.usta/` on purpose — visible and hand-editable (spec: mentor layer).
pub fn project_md_path(project_root: &Path) -> PathBuf {
    project_root.join("mentor/PROJECT.md")
}

/// User-facing project status + decision log: `<project>/mentor/PROGRESS.md`.
pub fn project_progress_path(project_root: &Path) -> PathBuf {
    project_root.join("mentor/PROGRESS.md")
}

/// Closing-reply delimiter — the model starts every file with this.
pub const FILE_DELIM: &str = tokens::FILE_DIVIDER;

/// Introduction instruction appended to opening turns while the profile is empty (spec Ç3a).
const MEET_BLOCK: &str = "\n[PROFILE EMPTY] You don't know the user yet. Introduce \
yourself briefly at the start of the conversation — ask their name, their background \
with this topic, how they like to learn. At most 1-2 questions, not a form; don't \
delay getting into the topic. If the user already introduced themselves, don't ask \
again. What you learn will be written to their profile at session close.\n";

/// Split the closing reply into (name, content) pairs. If there's no delimiter,
/// the whole reply counts as a single "progress" file — keeps the old format backward compatible.
pub fn split_files(reply: &str) -> Vec<(String, String)> {
    if !reply.contains(FILE_DELIM) {
        return vec![("progress".to_string(), clean_markdown_reply(reply))];
    }
    let mut out = Vec::new();
    for chunk in reply.split(FILE_DELIM).skip(1) {
        let Some((header, body)) = chunk.split_once("===") else {
            continue;
        };
        let name = header.trim().to_string();
        if name.is_empty() {
            continue;
        }
        out.push((name, clean_markdown_reply(body)));
    }
    out
}

/// User-turn content for the closing call: the current state of the three files +
/// generation rules. progress is always generated; approach/curriculum are living
/// documents — generated on the first session or when they change (TEACHING.md "Scope Guarding").
pub fn closing_prompt(
    topic: &str,
    progress: Option<&str>,
    approach: Option<&str>,
    curriculum: Option<&str>,
    profile: Option<&str>,
    project: Option<&str>,
    project_progress: Option<&str>,
) -> String {
    let p = progress.unwrap_or("(dosya henüz yok)");
    let a = approach.unwrap_or("(dosya henüz yok)");
    let c = curriculum.unwrap_or("(dosya henüz yok)");
    let pr = profile.unwrap_or("(dosya henüz yok)");
    let prj = project.unwrap_or("(dosya henüz yok)");
    let ppg = project_progress.unwrap_or("(dosya henüz yok)");
    let states = tokens::STATES.join("/");
    format!(
        "[SESSION CLOSING — FILE UPDATE]\n\
         Task: produce whichever of the files below need updating. Start each file with \
         this line: `{delim} <name>===` (name: progress | approach | curriculum | \
         profile | project | project-progress — e.g. if generating the profile, \
         `{delim} profile===`).\n\n\
         Current progress ({topic}):\n---\n{p}\n---\n\n\
         Current approach:\n---\n{a}\n---\n\n\
         Current curriculum:\n---\n{c}\n---\n\n\
         Current profile:\n---\n{pr}\n---\n\n\
         Current project definition (mentor/PROJECT.md):\n---\n{prj}\n---\n\n\
         Current project status (mentor/PROGRESS.md):\n---\n{ppg}\n---\n\n\
         Rules:\n\
         - `progress` is ALWAYS generated. Structure: `# {topic} {heading_suffix}` heading + \
         `## {lvl}` / `## {ret}` / `## {gaps}` (PROVE IT) / \
         `## {recall}` (each bullet: `- <question> — <one-line answer> | \
         due: YYYY-MM-DD | ivl: <days>`. Simplified spaced repetition, interval ladder \
         in days: 1, 3, 7, 16, 35, 90. Compute dates from the TODAY section. A question \
         recalled comfortably in this session's drill moves one rung up and gets `due = \
         today + new ivl`; a question answered wrong or with struggle resets to `ivl: 1` \
         (due tomorrow); a question not drilled this session keeps its tail UNCHANGED; a \
         new question starts at `ivl: 1` (due tomorrow); a legacy bullet without a tail \
         gets `ivl: 1` (due tomorrow). A question passed comfortably at `ivl: 90` \
         retires: move it to `{ret}` as a one-line summary and remove it from \
         this list.) / `## {errlog}` (`type | count | \
         last example`, 3+ repeats = GAP CANDIDATE) / `## {hint}` / `{goal_status}` \
         — write ONLY if approach defines a `{goal}`: time remaining (compute \
         from the TODAY section), map progress (%), pace assessment (on track / at risk \
         / behind + one-line rationale), measurement log (`date | measurement | score` — \
         mock exam, writing assessment, etc.). If a mock exam (/exam) ran this session, \
         append its result to the measurement log (date | mock exam | score) and record \
         weak items as gaps. If there's no goal, don't write this \
         section at all. / `## {open_ex}` — ONLY if an exercise was assigned this \
         session (or an earlier one is still open) and not completed: `- <file> | \
         <one-line assignment> | assigned YYYY-MM-DD`. A completed exercise moves to \
         `{ret}` as a normal item and leaves this section.\n\
         - `approach` is generated only on the first session or if the approach changed \
         this session — a living document that answers the three questions from \
         _default.md (practice / output / feedback). For goal-directed learning, approach \
         includes a `{goal}` section: what (certificate/level/output), exam/assessment \
         date (YYYY-MM-DD), passing threshold, exam/assessment format.\n\
         - `curriculum` is extracted as the FULL map on the first session (topic/subtopic \
         tree; each item with a `{states}` status; draw on \
         web research if needed); on later sessions it's generated only if a status \
         changed. An uncovered critical item must stay visible on the map. If the map \
         was anchored to course material, KEEP the source refs (`{src} <file> \
         §<section>`) on every item; items added from web research are marked `{src} \
         web`.\n\
         - Don't let the files bloat: if `{ret}` exceeds 20 items, collapse the \
         oldest into a one-line period summary; in `{errlog}`, remove resolved \
         entries not seen in a long time; keep curriculum sections that haven't changed \
         as-is (don't regenerate them).\n\
         - Don't add anything without evidence from this session; keep the valid \
         information already in the existing files (the user may have hand-edited them \
         — don't overwrite their edits).\n\
         - `profile` is generated only if new/changed permanent information about the \
         user was learned this session: name, background/experience, learning style, \
         preferences, recurring strengths/weaknesses. NO TOPIC KNOWLEDGE — 'learned \
         concept X' is progress's job; 'likes to learn from examples' goes in profile. \
         Evidence rule: a profile fact must come from what the USER wrote or \
         demonstrated in THIS session's transcript. Your own analogies, guesses and \
         inferences are NOT evidence — never record them as fact. If an inference is \
         worth keeping, mark it explicitly: append `(tahmin — kullanıcı doğrulamadı)` \
         to that line, and remove it if the user later contradicts it. \
         KEEP the valid information already in the current profile (the user may have \
         hand-edited it), ~1 page cap, merge duplicates. If nothing changed, don't \
         generate this file at all.\n\
         - `project` is the USER-FACING project definition, written to `mentor/PROJECT.md` \
         at the project root. Generate it ONLY when (a) the file doesn't exist yet and a \
         concrete project was discussed this session, or (b) the project definition \
         materially changed this session. Structure: `# <Project name> — Proje Tanımı` \
         heading + `## Ne` (1-2 sentences: what is being built) / `## Neden` \
         (goal/motivation, tie to the learning goal) / `## Ölçek` (solo-scale vs \
         1000-user scale — architecture advice anchors to this) / `## Stack` (language, \
         tools, WHY chosen) / `## Kapsam Dışı` (deliberate non-goals). For non-software \
         domains keep the same skeleton but adapt content (e.g. channels/tools instead \
         of stack). KEEP the user's hand-edits. If no project was discussed, do NOT \
         generate this file.\n\
         - `project-progress` is the USER-FACING project status, written to \
         `mentor/PROGRESS.md`. Generate it in every session where work happened ON THE \
         PROJECT (not for pure concept-learning sessions). Structure: `# <Project name> \
         — Durum` heading + `## Bitti` / `## Yapılıyor` / `## Sırada` (rewrite these \
         three with the CURRENT state — they are a pointer, not a journal) + \
         `## Kararlar` (append-only decision log: `- YYYY-MM-DD | decision | one-line \
         why`; append ONLY decisions taken this session; NEVER delete or rewrite \
         existing lines). This tracks the PROJECT's state — the learner's knowledge \
         belongs in `progress`, not here.\n\
         - Write no explanation/greeting outside the delimiter lines; every file is pure \
         markdown.",
        delim = tokens::FILE_DIVIDER,
        heading_suffix = tokens::PROGRESS_HEADING_SUFFIX,
        lvl = tokens::S_LEVEL,
        ret = tokens::S_RETIRED,
        gaps = tokens::S_GAPS,
        recall = tokens::S_RECALL,
        errlog = tokens::S_ERROR_LOG,
        hint = tokens::S_HINT_LADDER,
        goal_status = tokens::H_GOAL_STATUS,
        goal = tokens::H_GOAL,
        open_ex = tokens::S_OPEN_EXERCISE,
        src = tokens::SOURCE_DASH,
    )
}

/// Opening-drill turn: if progress exists, Usta speaks first at the start of the
/// session and asks a recall question (testing effect — TEACHING.md "Opening Drill" rule).
/// Where it hooks into plain.rs: Task 3 (opening-drill trigger).
///
/// `due`/`has_questions` are shell-selected: the shell already scanned the progress
/// file's `due:` tails (`welcome_data::due_questions` / `welcome_data::drill_count`), sorted and
/// capped them — date filtering/sorting is pure and deterministic, so it doesn't belong
/// in a model instruction. This function only decides HOW to phrase the turn for the
/// three possible shapes of that shell-computed state.
pub fn opening_prompt(
    topic: &str,
    profile_generic: bool,
    project_known: bool,
    game_streak: Option<&str>,
    due: &[String],
    has_questions: bool,
) -> String {
    let meet_block = if profile_generic { MEET_BLOCK } else { "" };
    let project_block = if project_known {
        "\nThe project files mentor/PROJECT.md and mentor/PROGRESS.md are in your \
         system prompt — do NOT re-ask project basics. After the drill, add one \
         sentence on where the PROJECT left off, taken from the `## Sırada` section \
         of mentor/PROGRESS.md.\n"
    } else {
        ""
    };
    let drill_block = if !due.is_empty() {
        let items = due.join("\n");
        format!(
            "ASK me these due recall questions, one at a time; don't answer them \
             yourself:\n{items}\nKeep it short: a 2-minute warm-up, then we move to \
             today's work."
        )
    } else if has_questions {
        "Say exactly one sentence: 'no reviews due today', skip the drill and move \
         straight to today's work."
            .to_string()
    } else {
        "If progress has no questions, come up with 2 small recall questions suited \
         to my level. Keep it short: a 2-minute warm-up, then we move to today's \
         work."
            .to_string()
    };
    let base = format!(
        "[SESSION OPENING — RECALL DRILL]\n{meet_block}\
         Topic: {topic}. {drill_block} When the \
         drill is done, say one sentence from the map: where we are, what's next (your \
         curriculum file is in the system prompt). If your progress file has an `## {open_ex}` \
         section, remind me in ONE sentence after the drill: open exercise: \
         <file> — continue or discuss it.{project_block}",
        open_ex = tokens::S_OPEN_EXERCISE,
    );
    match game_streak {
        Some(s) => format!("{base}\n[GAME] {s}\n"),
        None => base,
    }
}

/// Mock-exam session turn (`/exam`, goal-mode only — gated by `topic_has_goal` in
/// plain.rs before this is injected). Suspends the hint ladder/teaching for the
/// duration of the exam; scoring + gap recording happens at closing (SPEC §9).
pub fn exam_prompt(topic: &str) -> String {
    format!(
        "[EXAM MODE — MOCK EXAM]\n\
         Topic: {topic}. Build a mock exam from your curriculum map, following the exam\n\
         format defined under `{goal}` in your approach (format, question style, time\n\
         budget, passing threshold). Weight questions toward items not yet `{settled}` and\n\
         known gaps. State the number of questions and the time budget up front. Then:\n\
         ask ONE question at a time and wait for my answer; during the exam NO hints, NO\n\
         teaching, NO feedback between questions — the hint ladder is SUSPENDED until\n\
         the exam ends. After my last answer: score against the goal's threshold, give a\n\
         short per-map-item breakdown (strong/weak), name the weak items as gap\n\
         candidates, and remind me the result is recorded at session close. If I say\n\
         'stop the exam', end it early and score what was answered.",
        goal = tokens::H_GOAL,
        settled = tokens::STATE_SETTLED,
    )
}

/// New-topic introduction turn: no approach + curriculum map yet — Usta
/// derives them through open conversation (TEACHING.md "New Topic Introduction"). Not a fixed
/// form: it's derived from what the user says, direction stays with the user.
pub fn onboarding_prompt(
    topic: &str,
    intro: Option<&str>,
    profile_generic: bool,
    project_known: bool,
    materials: Option<&str>,
) -> String {
    // The raw text the user typed when opening the topic IS the FIRST ANSWER of the
    // introduction — if it's reduced to a slug and discarded, the model just re-asks
    // what was already said ("I'll set up Coolify for my client, Fedora..." → "what are you after?" disaster).
    let intro_block = match intro {
        Some(s) if !s.trim().is_empty() => format!(
            "\nWhen the user opened the topic, they wrote this — treat it as the FIRST \
             ANSWER of the introduction:\n\
             \"{}\"\n\
             USE this information: don't ask again what they already said; start by \
             picking up on what they said and only ask about what's still missing.\n",
            s.trim()
        ),
        _ => String::new(),
    };
    let meet_block = if profile_generic { MEET_BLOCK } else { "" };
    let project_block = if project_known {
        "\nThe project files mentor/PROJECT.md and mentor/PROGRESS.md are in your \
         system prompt — do NOT re-ask project basics; connect this new topic to \
         the existing project context.\n"
    } else {
        "\nThere is no mentor/PROJECT.md for this project yet. During the \
         introduction also find out, naturally (not as a form): what they're \
         building, why, rough scale, stack/tools and why. At session close you'll \
         be asked for a `project` file — the shell writes it; don't write files \
         yourself during the session.\n"
    };
    let material_block = match materials {
        Some(d) => format!(
            "\n[COURSE MATERIAL FOUND]\nThe user has material under materials/ — \
             outline digests below. ASK whether to anchor this topic's curriculum \
             to this material (it may belong to another topic). If yes: build the \
             curriculum map FROM its chapters/sections — each map item carries a \
             source ref (`{src} <file> §<section>`); assign reading from it \
             (the USER reads — you don't summarize the material into the chat); \
             still add critical items the material lacks, from web research \
             (scope guarding). If no: proceed normally.\n---\n{d}\n---\n",
            src = tokens::SOURCE_DASH,
        ),
        None => String::new(),
    };
    format!(
        "[NEW TOPIC — INTRODUCTION]\n\
         Topic: {topic}. This topic has no approach or curriculum map yet.\n{intro_block}{meet_block}{project_block}\
         Have a short, NATURAL introduction — this is not a form: ask at most two \
         questions in a single message, continue based on the answer; don't dump a \
         numbered question list. Find out: what they want to do/learn, what they \
         already have. Whether this is exploration or goal-directed — infer it \
         YOURSELF, don't ask the user using these terms; if it's not clear from what \
         they said, ask one jargon-free question: 'are you preparing for a deadline or \
         exam, or is this just out of curiosity?'. If it's goal-directed, gather the \
         what/date/threshold/format info during the conversation — it will go into the \
         approach's `{goal}` section; the map is built from the official framework \
         (exam syllabus / exam guide / CEFR) — research it on the web. If you don't \
         know the domain well enough, research it on the web. At session close you'll \
         be asked for the approach + FULL curriculum map CONTENT; the shell writes the \
         files, don't try to write files yourself during the session (Hard Rule 6) — \
         deepen the introduction accordingly but don't turn it into a lecture, keep it \
         short.{material_block}",
        goal = tokens::H_GOAL,
    )
}

/// Strip any ```-fence wrapper from the model's reply — pure markdown is written to the file.
pub fn clean_markdown_reply(reply: &str) -> String {
    let t = reply.trim();
    if let Some(rest) = t.strip_prefix("```") {
        // First line is the fence tag (```markdown etc.) — drop it.
        let body = rest.split_once('\n').map(|(_, b)| b).unwrap_or("");
        let body = body.trim_end();
        let body = body.strip_suffix("```").unwrap_or(body);
        return body.trim().to_string();
    }
    t.to_string()
}

/// Atomic write: write to tmp, then move it over the target — never leaves a half-written file.
pub fn write_atomic(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory: {}", parent.display()))?;
    }
    // Back up the previous version — a bad model output can be restored from the single copy.
    if path.exists() {
        let bak = path.with_extension("md.bak");
        let _ = std::fs::copy(path, &bak);
    }
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, content).with_context(|| format!("failed to write: {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("failed to move: {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
#[path = "progress_tests.rs"]
mod tests;
