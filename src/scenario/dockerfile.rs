//! The small Dockerfile language a generated package may use. Model-written
//! recipes never reach `docker build`: it has no memory, CPU or pids limits, and
//! features such as `# syntax=`, `ADD <url>` or `RUN --mount` reach past the context.
//! `limited_build.rs` runs the parsed steps inside a limited container.

use crate::scenario::lint::safe_rel;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Step {
    Run(String),
    Copy { srcs: Vec<String>, dest: String },
    Workdir(String),
    Env(String, String),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Recipe {
    pub base: String,
    pub steps: Vec<Step>,
}

/// Bases a recipe may start from. Exact names only: a flag such as `-p8080:80`,
/// a local image or a registry path is never on this list. Images that declare a
/// VOLUME (postgres, mysql, redis, ...) are left out: the volume would be created
/// anonymously and live outside the limits.
pub(crate) const BASES: [&str; 8] = [
    "debian", "ubuntu", "alpine", "python", "node", "rust", "golang", "nginx",
];

/// An allowed official image with an explicit version.
fn base_ok(image: &str) -> bool {
    let Some((name, tag)) = image.split_once(':') else {
        return false;
    };
    BASES.contains(&name)
        && tag.starts_with(|c: char| c.is_ascii_alphanumeric())
        && tag
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        && tag != "latest"
}

fn is_directive(line: &str) -> bool {
    let body = line
        .trim_start_matches('#')
        .trim_start()
        .to_ascii_lowercase();
    ["syntax", "escape", "check"].iter().any(|k| {
        body.strip_prefix(k)
            .is_some_and(|r| r.trim_start().starts_with('='))
    })
}

/// Logical lines: comments and blank lines dropped (also inside a continuation,
/// as Docker does), `\` continuations joined.
fn logical_lines(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for raw in text.lines() {
        let line = raw.trim_end();
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let piece = if cur.is_empty() {
            line
        } else {
            line.trim_start()
        };
        match piece.strip_suffix('\\') {
            Some(head) => {
                cur.push_str(head.trim_end());
                cur.push(' ');
            }
            None => {
                cur.push_str(piece);
                out.push(std::mem::take(&mut cur));
            }
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn parse_line(line: &str) -> Result<Step, String> {
    let (word, rest) = line
        .trim()
        .split_once(char::is_whitespace)
        .unwrap_or((line.trim(), ""));
    let rest = rest.trim();
    match word.to_ascii_uppercase().as_str() {
        "RUN" => {
            if rest.is_empty()
                || rest.starts_with("--")
                || rest.starts_with('[')
                || rest.contains("<<")
            {
                return Err(format!(
                    "RUN must be a plain shell command, no flags or heredocs: {line}"
                ));
            }
            Ok(Step::Run(rest.to_string()))
        }
        "COPY" => {
            let parts: Vec<&str> = rest.split_whitespace().collect();
            let Some((dest, srcs)) = parts.split_last().filter(|(_, s)| !s.is_empty()) else {
                return Err(format!("COPY needs a source and a destination: {line}"));
            };
            let bad =
                |s: &str| !safe_rel(s) || s.contains(['*', '?', '[', '<']) || s.starts_with('-');
            if srcs.iter().any(|s| bad(s))
                || !dest.starts_with('/')
                || (srcs.len() > 1 && !dest.ends_with('/'))
            {
                return Err(format!(
                    "COPY takes plain files from the context to an absolute path, several only into a directory ending in /: {line}"
                ));
            }
            Ok(Step::Copy {
                srcs: srcs.iter().map(|s| s.to_string()).collect(),
                dest: dest.to_string(),
            })
        }
        "WORKDIR" if rest.starts_with('/') && !rest.contains(char::is_whitespace) => {
            Ok(Step::Workdir(rest.to_string()))
        }
        "ENV" => match rest.split_once('=') {
            Some((k, v))
                if !k.is_empty()
                    && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && !v.contains([' ', '"', '\'', '\t']) =>
            {
                Ok(Step::Env(k.to_string(), v.to_string()))
            }
            _ => Err(format!(
                "ENV takes one NAME=value without spaces or quotes: {line}"
            )),
        },
        other => Err(format!(
            "{other} is not allowed; a task recipe uses only FROM, RUN, COPY, WORKDIR and ENV"
        )),
    }
}

pub(crate) fn parse(text: &str) -> Result<Recipe, Vec<String>> {
    let mut problems: Vec<String> = text
        .lines()
        .filter(|l| l.trim_start().starts_with('#') && is_directive(l))
        .map(|l| format!("parser directives are not allowed: {}", l.trim()))
        .collect();
    let lines = logical_lines(text);
    let mut it = lines.iter();
    let base = match it.next().map(|l| l.split_whitespace().collect::<Vec<_>>()) {
        Some(w) if w.len() == 2 && w[0].eq_ignore_ascii_case("FROM") && base_ok(w[1]) => {
            w[1].to_string()
        }
        _ => {
            problems.push(format!(
                "the first line must be FROM <image>:<version> with an image from {}, version not latest",
                BASES.join(", ")
            ));
            String::new()
        }
    };
    let mut steps = Vec::new();
    for line in it {
        match parse_line(line) {
            Ok(s) => steps.push(s),
            Err(e) => problems.push(e),
        }
    }
    if problems.is_empty() {
        Ok(Recipe { base, steps })
    } else {
        Err(problems)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_recipe_parses_in_order() {
        let r = parse(
            "FROM debian:12-slim\nRUN apt-get update && \\\n  apt-get install -y nginx\nCOPY setup.sh start.sh /usr/local/lib/usta/\nENV MODE=fast\nWORKDIR /root\n",
        )
        .unwrap();
        assert_eq!(r.base, "debian:12-slim");
        assert_eq!(
            r.steps[0],
            Step::Run("apt-get update && apt-get install -y nginx".into())
        );
        assert_eq!(
            r.steps[1],
            Step::Copy {
                srcs: vec!["setup.sh".into(), "start.sh".into()],
                dest: "/usr/local/lib/usta/".into()
            }
        );
        assert_eq!(r.steps[2], Step::Env("MODE".into(), "fast".into()));
        assert_eq!(r.steps[3], Step::Workdir("/root".into()));
    }

    #[test]
    fn everything_outside_the_small_language_is_refused() {
        for bad in [
            "FROM debian:latest\n",
            "FROM debian\n",
            "FROM evil.example/x:1\n",
            "FROM someone/image:1\n",
            "FROM debian:12 AS a\nFROM debian:12\n",
            "# syntax=evil/frontend:1\nFROM debian:12\n",
            "# escape=`\nFROM debian:12\n",
            "FROM debian:12\nADD http://x/y /y\n",
            "FROM debian:12\nVOLUME /data\n",
            "FROM debian:12\nUSER nobody\n",
            "FROM debian:12\nENTRYPOINT [\"x\"]\n",
            "FROM debian:12\nCOPY --from=other:1 /a /b\n",
            "FROM debian:12\nCOPY ../secret /b\n",
            "FROM debian:12\nCOPY a* /b/\n",
            "FROM debian:12\nCOPY a relative/\n",
            "FROM debian:12\nCOPY a b /c\n",
            "FROM -p8080:80\n",
            "FROM mycompany-api:dev\n",
            "FROM postgres:16\n",
            "FROM redis:7\n",
            "FROM Debian:12\n",
            "FROM debian:-x\n",
            "FROM debian:12@sha256:abc\n",
            "FROM debian:\n",
            "FROM scratch:1\n",
            "FROM debian:12\nRUN --network=host curl x\n",
            "FROM debian:12\nRUN <<EOF\nid\nEOF\n",
            "FROM debian:12\nRUN [\"id\"]\n",
            "FROM debian:12\nENV A=\"b c\"\n",
            "RUN id\n",
            "",
        ] {
            assert!(parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn the_base_refusal_names_the_allowed_images() {
        let e = parse("FROM mycompany-api:dev\n").unwrap_err().join("\n");
        for b in BASES {
            assert!(e.contains(b), "{b}: {e}");
        }
    }
}
