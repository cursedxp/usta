# USTA — Index

This file contains NO behavior and is NOT loaded into the model — it's a human map. Find which file to check when you want to fix something.

## Intervention Map

| Symptom | File |
|---|---|
| Tone/personality/voice not landing, not connecting | `SOUL.md` |
| Wrong behavior: wrote code, made something up, narrated mechanics, overwrote a file | `RULES.md` |
| Teaching style: drills, hint timing, spec cadence, introductions | `TEACHING.md` |
| Exam/goal tracking, pacing, format practice | `GOAL.md` |
| Wrong/missing info about the user | `USER.md` (or `usta reset --profile`) |

## Loading Order (`brain.rs`)

```
SOUL.md → RULES.md → TEACHING.md → [GOAL.md, only if approach has "## Hedef"]
→ approaches/(software|_default).md → approaches/<topic>.md
→ USER.md → progress → curriculum → [TODAY section as-is]
```

Behavior changes are made in the RELEVANT file; behavior sentences are NOT written here. After a change: `cargo install --path .`
