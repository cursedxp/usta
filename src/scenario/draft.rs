//! The model's reply as a package on disk. Everything here is untrusted: files land
//! only under the known package folders, and the identity fields are Usta's.

use std::path::Path;

use serde::Deserialize;

use crate::scenario::lint::safe_rel;
use crate::scenario::rubric::parse_json;

pub(crate) const MAX_FILE: usize = 64 * 1024;
const MAX_TASK: usize = 16 * 1024;
const MAX_FILES: usize = 60;
const MAX_TOTAL: usize = 256 * 1024;
const ROOTS: &[&str] = &["environment/", "solution/", "tests/", "tone/"];

#[derive(Debug, Deserialize)]
pub(crate) struct FileOut {
    pub path: String,
    pub content: String,
}

/// Only these fields are read; an `id`, `topic` or `version` the model sends is dropped.
#[derive(Debug, Deserialize)]
struct Raw {
    kind: String,
    why: Option<String>,
    task_toml: Option<String>,
    files: Option<Vec<FileOut>>,
}

#[derive(Debug)]
pub(crate) struct Reply {
    pub task_toml: String,
    pub files: Vec<FileOut>,
}

#[derive(Debug)]
pub(crate) enum Draft {
    Package(Reply),
    NeedsSystem(String),
}

pub(crate) fn read(text: &str) -> Result<Draft, Vec<String>> {
    let raw: Raw =
        parse_json(text).map_err(|e| vec![format!("the reply must be the JSON described: {e}")])?;
    match (raw.kind.as_str(), raw.task_toml, raw.files) {
        ("needs_system", _, _) => Ok(Draft::NeedsSystem(raw.why.unwrap_or_default())),
        ("package", Some(task_toml), Some(files)) => Ok(Draft::Package(Reply { task_toml, files })),
        _ => Err(vec!["the reply needs kind, task_toml and files".into()]),
    }
}

/// A plain path under a known package folder. Any component starting with a dot is refused,
/// as lint does.
fn path_ok(p: &str) -> bool {
    safe_rel(p)
        && !p.split('/').any(|c| c.starts_with('.'))
        && (p == "instruction.md" || ROOTS.iter().any(|r| p.starts_with(r) && p.len() > r.len()))
}

fn stamped(toml_text: &str, id: &str, topic: &str, version: u32) -> Result<String, String> {
    let mut t: toml::Table = toml_text
        .parse()
        .map_err(|e| format!("task_toml does not parse: {e}"))?;
    t.insert("id".into(), toml::Value::String(id.into()));
    t.insert("topic".into(), toml::Value::String(topic.into()));
    t.insert("version".into(), toml::Value::Integer(version.into()));
    if let Some(env) = t.get_mut("environment").and_then(toml::Value::as_table_mut) {
        env.remove("context"); // always `environment`
    }
    toml::to_string(&t).map_err(|e| e.to_string())
}

/// Write the package to `dir`, a scratch folder Usta built. Nothing is written
/// unless every path, size and the task text pass; a failed write removes `dir`.
pub(crate) fn stage(
    r: &Reply,
    dir: &Path,
    id: &str,
    topic: &str,
    version: u32,
) -> Result<(), Vec<String>> {
    let mut problems: Vec<String> = r
        .files
        .iter()
        .filter(|f| !path_ok(&f.path))
        .map(|f| format!("path '{}' is not allowed in a package", f.path))
        .collect();
    problems.extend(
        r.files
            .iter()
            .filter(|f| f.content.len() > MAX_FILE)
            .map(|f| format!("{} is too large; keep files under 64 KB", f.path)),
    );
    if r.files.iter().map(|f| f.content.len()).sum::<usize>() > MAX_TOTAL {
        problems.push("the package is too large; keep it under 256 KB".into());
    }
    if r.task_toml.len() > MAX_TASK {
        problems.push("task_toml is too large; keep it under 16 KB".into());
    }
    if r.files.len() > MAX_FILES {
        problems.push(format!(
            "too many files; keep it to {MAX_FILES} files or fewer"
        ));
    }
    let mut seen = std::collections::HashSet::new();
    for f in &r.files {
        if !seen.insert(f.path.as_str()) {
            problems.push(format!("path '{}' appears twice", f.path));
        }
    }
    let task = stamped(&r.task_toml, id, topic, version);
    if let Err(e) = &task {
        problems.push(e.clone());
    }
    if !problems.is_empty() {
        return Err(problems);
    }
    let write = || -> std::io::Result<()> {
        let _ = std::fs::remove_dir_all(dir);
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join("task.toml"), task.as_deref().unwrap_or_default())?;
        for f in &r.files {
            let path = dir.join(&f.path);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, &f.content)?;
        }
        Ok(())
    };
    write().map_err(|e| {
        let _ = std::fs::remove_dir_all(dir);
        vec![format!("the package could not be written: {e}")]
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::scenario::package::tests::tmp;
    use crate::scenario::package::Package;

    pub(crate) const REPLY: &str = r###"{"kind":"package",
 "task_toml":"title = \"T\"\nskills = [\"positioning\"]\nid = \"model-picked\"\ntopic = \"other\"\nversion = 9\n\n[environment]\nkind = \"conversation\"\ncontext = \"../..\"\n\n[check]\nkind = \"rubric\"\nwhy_rubric = \"judgment\"\n\n[[check.rubric]]\nid = \"a\"\ntext = \"names one audience\"\n\n[[check.rubric]]\nid = \"b\"\ntext = \"gives a reason\"\n",
 "files":[{"path":"instruction.md","content":"Who is it for?\n"},
          {"path":"tone/adventure.md","content":"## open\nGo.\n\n## close\nDone.\n"},
          {"path":"solution/answer.md","content":"Bakers, because of price.\n"}]}"###;

    pub(crate) const REVIEW_OK: &str = r#"{"ok":true,"problems":[]}"#;

    /// A judge's reply with every named criterion met.
    pub(crate) fn met(ids: &[&str]) -> String {
        let criteria: Vec<_> = ids
            .iter()
            .map(|id| serde_json::json!({"id": id, "met": true, "note": ""}))
            .collect();
        serde_json::json!({ "criteria": criteria }).to_string()
    }

    fn reply_with(task_toml: &str, files: &[(&str, &str)]) -> String {
        let mut all = vec![
            ("instruction.md", "Fix the task.\n"),
            ("tone/adventure.md", "## open\nGo.\n\n## close\nDone.\n"),
        ];
        all.extend_from_slice(files);
        let files: Vec<_> = all
            .iter()
            .map(|(path, content)| serde_json::json!({"path": path, "content": content}))
            .collect();
        serde_json::json!({"kind": "package", "task_toml": task_toml, "files": files}).to_string()
    }

    pub(crate) fn container_reply() -> String {
        reply_with(
            "title = \"Serve it\"\nskills = [\"serving\"]\n\n[environment]\nkind = \"container\"\n\n[check]\nkind = \"script\"\nscript = \"check.sh\"\n",
            &[
                ("environment/Dockerfile", "FROM debian:12-slim\nRUN true\n"),
                ("solution/solve.sh", "#!/bin/sh\ntrue\n"),
                ("tests/check.sh", "#!/bin/sh\nexit 1\n"),
            ],
        )
    }

    pub(crate) fn folder_rubric_reply() -> String {
        reply_with(
            "title = \"Plan it\"\nskills = [\"positioning\"]\n\n[environment]\nkind = \"folder\"\n\n[check]\nkind = \"rubric\"\ndeliverables = [\"plan.md\"]\nwhy_rubric = \"judgment\"\n\n[[check.rubric]]\nid = \"a\"\ntext = \"names one audience\"\nevidence = [\"brief.md\"]\n\n[[check.rubric]]\nid = \"b\"\ntext = \"gives a reason\"\nevidence = [\"brief.md\"]\n",
            &[
                ("environment/files/brief.md", "Audience: bakers.\n"),
                ("solution/files/plan.md", "Bakers, because of price.\n"),
            ],
        )
    }

    pub(crate) fn folder_script_reply() -> String {
        reply_with(
            "title = \"Count it\"\nskills = [\"counting\"]\n\n[environment]\nkind = \"folder\"\n\n[check]\nkind = \"script\"\nscript = \"check.sh\"\n",
            &[
                ("environment/files/brief.md", "Count the rows.\n"),
                ("solution/files/answer.txt", "3\n"),
                ("tests/check.sh", "#!/bin/sh\nexit 1\n"),
            ],
        )
    }

    #[test]
    fn usta_assigns_identity_and_the_model_cannot() {
        let d = tmp("draft_id");
        let Draft::Package(r) = read(&format!("Here it is:\n```json\n{REPLY}\n```")).unwrap()
        else {
            panic!()
        };
        stage(&r, &d.join("gen-m-1"), "gen-m-1", "marketing", 2).unwrap();
        let p = Package::load(&d.join("gen-m-1")).unwrap();
        assert_eq!(
            (p.id.as_str(), p.topic.as_str(), p.version),
            ("gen-m-1", "marketing", 2)
        );
        assert_eq!(p.environment.context, "environment");
        assert_eq!(p.lint(), Vec::<String>::new());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_file_path_that_leaves_the_package_is_refused_and_nothing_is_written_outside() {
        let d = tmp("draft_escape");
        for bad in [
            "../evil.md",
            "/etc/x",
            "environment/../../evil",
            "task.toml",
            "evil.sh",
            "tests/../../x",
            ".hidden.md",
            "tone/.x.md",
            "environment/.git/config",
            "environment/",
            "solution/./a",
        ] {
            let text = REPLY.replace("\"instruction.md\"", &format!("\"{bad}\""));
            let Draft::Package(r) = read(&text).unwrap() else {
                panic!()
            };
            let e = stage(&r, &d.join("p"), "p", "t", 1).unwrap_err();
            assert!(e.concat().contains("path"), "{bad}: {e:?}");
        }
        assert!(!d.join("evil.md").exists() && !d.join("p").exists());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn oversized_or_unreadable_replies_are_feedback() {
        assert!(read("no json at all").is_err());
        assert!(read("{\"kind\":\"package\"}").is_err());
        let big = serde_json::json!({"kind":"package","task_toml":"title=\"t\"",
            "files":[{"path":"instruction.md","content":"x".repeat(MAX_FILE + 1)}]})
        .to_string();
        let Draft::Package(r) = read(&big).unwrap() else {
            panic!()
        };
        let d = tmp("draft_big");
        assert!(stage(&r, &d.join("p"), "p", "t", 1).is_err());
        let _ = std::fs::remove_dir_all(d);
    }

    #[test]
    fn a_task_that_truly_needs_a_running_system_says_so() {
        let Draft::NeedsSystem(why) =
            read(r#"{"kind":"needs_system","why":"nginx must run"}"#).unwrap()
        else {
            panic!()
        };
        assert_eq!(why, "nginx must run");
    }

    #[test]
    fn counts_task_text_and_duplicate_paths_are_capped() {
        let d = tmp("draft_caps");
        let file = |p: &str| FileOut {
            path: p.into(),
            content: "x".into(),
        };
        let many = Reply {
            task_toml: "title = \"t\"".into(),
            files: (0..=MAX_FILES)
                .map(|i| file(&format!("tone/{i}.md")))
                .collect(),
        };
        let e = stage(&many, &d.join("a"), "a", "t", 1)
            .unwrap_err()
            .concat();
        assert!(e.contains("files"), "{e}");
        let long = Reply {
            task_toml: format!("title = \"{}\"", "x".repeat(MAX_TASK)),
            files: vec![],
        };
        assert!(stage(&long, &d.join("b"), "b", "t", 1).is_err());
        let dup = Reply {
            task_toml: "title = \"t\"".into(),
            files: vec![file("instruction.md"), file("instruction.md")],
        };
        let e = stage(&dup, &d.join("c"), "c", "t", 1).unwrap_err().concat();
        assert!(e.contains("twice"), "{e}");
        assert!(!d.join("a").exists() && !d.join("b").exists() && !d.join("c").exists());
        let _ = std::fs::remove_dir_all(d);
    }
}
