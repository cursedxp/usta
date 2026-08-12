//! Visual explainer (/show): embedded HTML skeleton (player shell) + vendored
//! anime.js tween layer + model-produced scene JSON. The model writes ONLY the
//! scene array; the shell validates and injects it.

use anyhow::{bail, Context, Result};

const SKELETON: &str = include_str!("visual_skeleton.html");
const ANIME: &str = include_str!("vendor/anime.min.js");
const ROUGH: &str = include_str!("vendor/rough.min.js");
const PLACEHOLDER_SCENES: &str = "/*__SCENES__*/[]";
const PLACEHOLDER_ANIME: &str = "/*__ANIME__*/";
const PLACEHOLDER_ROUGH: &str = "/*__ROUGH__*/";

/// Validate the scene JSON and inject anime.js + scenes into the skeleton.
/// Errors: not JSON, not an array, empty array, any scene missing a `caption`.
pub fn build_visual_html(scenes_json: &str) -> Result<String> {
    let v: serde_json::Value =
        serde_json::from_str(scenes_json).context("scene data is not valid JSON")?;
    let arr = v.as_array().context("scene data must be a JSON array")?;
    if arr.is_empty() {
        bail!("scene array is empty");
    }
    for (i, s) in arr.iter().enumerate() {
        if s.get("caption").and_then(|c| c.as_str()).is_none() {
            bail!("scene {i} has no caption");
        }
    }
    Ok(SKELETON
        .replacen(PLACEHOLDER_ROUGH, ROUGH, 1)
        .replacen(PLACEHOLDER_ANIME, ANIME, 1)
        .replacen(PLACEHOLDER_SCENES, &v.to_string().replace("</", "<\\/"), 1))
}

/// `/show` → Some(None) (visualize the last explanation); `/show <topic>` →
/// Some(Some(topic)). Anything else → None. Slash lines never reach the LLM session.
/// Case-insensitive on the command token (`/SHOW dns` works); the topic argument
/// keeps its original casing.
pub fn parse_show_command(line: &str) -> Option<Option<String>> {
    let t = line.trim();
    if t.eq_ignore_ascii_case("/show") {
        return Some(None);
    }
    // "/show " prefix is pure ASCII, so ASCII-lowercasing preserves byte offsets —
    // slicing the ORIGINAL string at 6 keeps the argument's casing intact.
    if !t.to_ascii_lowercase().starts_with("/show ") {
        return None;
    }
    Some(Some(t[6..].trim().to_string()))
}

/// System prompt for the visual mini-session: scene-JSON contract + pedagogy.
pub fn visual_system() -> String {
    "You produce animation scenes for a visual explainer. Output ONLY a JSON array \
     of scenes — no prose, no markdown fences, no HTML.\n\
     \n\
     Stage: 800 x 450 coordinate space.\n\
     Scene: {\"caption\": string, \"duration\": ms (optional, default 3500), \"ops\": [...]}\n\
     Ops:\n\
     - {\"op\":\"add\",\"el\":{\"id\",\"type\":\"node|circle|text\",...}} — node: x,y,w,h,label; \
       circle: cx,cy,r,label; text: x,y,text,size\n\
     - {\"op\":\"arrow\",\"id\",\"from\",\"to\",\"label\"?} — animated arrow between two elements\n\
     - {\"op\":\"packet\",\"along\":<arrow id>,\"label\"?} — a dot travelling along an arrow (use for flows)\n\
     - {\"op\":\"move\",\"id\",\"x\",\"y\"} · {\"op\":\"pulse\",\"id\"} · \
       {\"op\":\"highlight\",\"id\",\"on\":bool} · {\"op\":\"remove\",\"id\"} · \
       {\"op\":\"note\",\"id\",\"x\",\"y\",\"text\"} — short callout\n\
     \n\
     Composition (binding):\n\
     - Align to an 8px grid: all x/y/w/h values are multiples of 8.\n\
     - Keep at least 40px margin from stage edges; distribute elements, don't cluster.\n\
     - Flow direction: left→right for processes, top→down for hierarchies. Never zigzag.\n\
     - Same kind = same size (e.g. all server nodes share identical w/h).\n\
     - Labels are at most 3 words; longer explanations go in captions or notes, never inside nodes.\n\
     - One focal point per scene: at most one pulse or new highlight at a time.\n\
     \n\
     Pedagogy (binding):\n\
     - 6-12 scenes; each scene makes exactly ONE idea visible. Build up cumulatively.\n\
     - Captions in the same language as the user's conversation; short, concrete, no jargon dumps.\n\
     - Keep at most ~6 visible elements at a time; remove what is no longer needed.\n\
     - Prefer motion that carries meaning (packets for data flow, pulse for 'this reacts', \
       highlight for 'remember this').\n\
     - Use a concrete analogy where it helps, in a note.\n\
     - End with a summary scene that shows the whole picture once more."
        .to_string()
}

/// Target file: `.usta/visuals/<topic>/<timestamp>-<concept-slug>.html`.
pub fn visual_path(project_root: &std::path::Path, topic: &str, concept: &str) -> std::path::PathBuf {
    let stamp = chrono::Local::now().format("%Y-%m-%d-%H%M%S");
    let slug = crate::slugify_topic(concept);
    project_root
        .join(".usta/visuals")
        .join(topic)
        .join(format!("{stamp}-{slug}.html"))
}

/// Best-effort browser open; false = caller should just print the path.
pub fn open_in_browser(path: &std::path::Path) -> bool {
    let cmd = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    std::process::Command::new(cmd)
        .arg(path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .is_ok()
}

/// Recognizes `[[show: <topic>]]` only when trimmed to exactly this shape (no
/// leading/trailing text on the same line); case-insensitive on `show`.
fn match_marker_line(line: &str) -> Option<String> {
    let t = line.trim();
    if t.len() < 4 || !t.starts_with("[[") || !t.ends_with("]]") {
        return None;
    }
    let inner = t[2..t.len() - 2].trim_start();
    if inner.len() < 4 || !inner[..4].eq_ignore_ascii_case("show") {
        return None;
    }
    let rest = inner[4..].trim_start();
    let rest = rest.strip_prefix(':')?;
    let topic = rest.trim();
    if topic.is_empty() {
        return None;
    }
    Some(topic.to_string())
}

/// Extracts a natural-language `[[show: <topic>]]` trigger from a reply
/// (Görev 4). Recognized ONLY as the reply's own LAST line (trailing
/// whitespace on that line tolerated); case-insensitive on `show`. If the
/// last line qualifies, EVERY standalone marker line in the reply is
/// stripped (not just the last), and the LAST one's topic wins — this is
/// what makes "two markers" collapse into a single trigger. A `[[show:`
/// that isn't alone on the reply's final line (mid-text, or on an earlier
/// line only) is left completely untouched: returns the text unchanged and
/// `None`. Callers must run this BEFORE displaying/recording a reply — the
/// marker never reaches the screen or session history.
///
/// Marker-only reply: if stripping leaves nothing, a short synthetic
/// stand-in (`(visual explainer: <topic>)`) is returned instead of an empty
/// string — every call site pushes the clean text into session history, and
/// an empty assistant message would make the NEXT API turn fail (the
/// Messages API rejects empty content). The stand-in doubles as useful
/// context: future turns can see a visual was shown here.
pub fn extract_show_marker(reply: &str) -> (String, Option<String>) {
    let lines: Vec<&str> = reply.lines().collect();
    let Some(last) = lines.last() else {
        return (reply.to_string(), None);
    };
    let Some(topic) = match_marker_line(last) else {
        return (reply.to_string(), None);
    };
    let kept: Vec<&str> = lines.iter().filter(|l| match_marker_line(l).is_none()).copied().collect();
    let clean = kept.join("\n").trim_end().to_string();
    if clean.trim().is_empty() {
        return (format!("(visual explainer: {topic})"), Some(topic));
    }
    (clean, Some(topic))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"[
      {"caption":"Browser and server appear","duration":3000,"ops":[
        {"op":"add","el":{"id":"b","type":"node","x":80,"y":200,"w":140,"h":70,"label":"Browser"}},
        {"op":"add","el":{"id":"s","type":"node","x":580,"y":200,"w":140,"h":70,"label":"Server"}}
      ]},
      {"caption":"A request travels","ops":[
        {"op":"arrow","id":"a1","from":"b","to":"s","label":"GET /"},
        {"op":"packet","along":"a1","label":"GET"},
        {"op":"pulse","id":"s"}
      ]},
      {"caption":"The response comes back","ops":[
        {"op":"remove","id":"a1"},
        {"op":"arrow","id":"a2","from":"s","to":"b","label":"200 OK"},
        {"op":"packet","along":"a2"},
        {"op":"highlight","id":"b","on":true},
        {"op":"note","id":"n1","x":400,"y":60,"text":"Every page load works like this"}
      ]}
    ]"#;

    #[test]
    fn build_injects_anime_and_scenes_into_full_document() {
        let html = build_visual_html(SAMPLE).unwrap();
        assert!(html.contains("<!doctype html>") || html.contains("<!DOCTYPE html>"));
        assert!(html.contains("anime.js v3.2.2"), "vendored anime.js (with MIT header) must be inlined");
        assert!(html.contains("\"caption\":\"A request travels\""));
        assert!(!html.contains(PLACEHOLDER_ANIME), "anime placeholder must be consumed");
        assert!(!html.contains("/*__SCENES__*/"), "scenes placeholder must be consumed");
        // rough.js (vendored Görev 2): MIT header inlined, placeholder consumed.
        assert!(html.contains("rough.js v4.6.6"), "vendored rough.js (with MIT header) must be inlined");
        assert!(!html.contains(PLACEHOLDER_ROUGH), "rough placeholder must be consumed");
    }

    #[test]
    fn build_rejects_invalid_json() {
        assert!(build_visual_html("not json").is_err());
        assert!(build_visual_html("{\"a\":1}").is_err()); // not an array
        assert!(build_visual_html("[]").is_err()); // empty
        assert!(build_visual_html("[{\"ops\":[]}]").is_err()); // no caption
    }

    #[test]
    fn skeleton_is_selfcontained_player() {
        assert!(SKELETON.contains(PLACEHOLDER_SCENES));
        assert!(SKELETON.contains(PLACEHOLDER_ANIME));
        assert!(SKELETON.contains(PLACEHOLDER_ROUGH), "skeleton missing /*__ROUGH__*/ placeholder");
        assert_eq!(SKELETON.matches(PLACEHOLDER_SCENES).count(), 1);
        assert_eq!(SKELETON.matches(PLACEHOLDER_ANIME).count(), 1);
        assert_eq!(SKELETON.matches(PLACEHOLDER_ROUGH).count(), 1);
        assert!(SKELETON.contains("prefers-color-scheme"));
        for marker in ["id=\"prev\"", "id=\"play\"", "id=\"next\"", "id=\"caption\"", "<svg"] {
            assert!(SKELETON.contains(marker), "skeleton missing {marker}");
        }
        // Görev 2 (frozen design tokens): Excalifont embedded as data-URI @font-face, no
        // network loads — verify the marker text and license note are present in-source.
        assert!(SKELETON.contains("@font-face"), "skeleton missing @font-face");
        assert!(SKELETON.contains("Excalifont"), "skeleton missing Excalifont font-family");
        assert!(SKELETON.contains("data:font/woff2;base64,"), "font must be embedded as data-URI, not linked");
        assert!(SKELETON.contains("OFL-1.1"), "skeleton missing OFL-1.1 license note for the embedded font");
        // Offline guarantee: no external loads at runtime. (Plain `http://` cannot be
        // asserted away — the SVG namespace URI legitimately contains it.)
        assert!(!SKELETON.contains("<script src"));
        assert!(!SKELETON.contains("<link "));
        assert!(!SKELETON.contains("fetch("));
        assert!(!ANIME.contains("<script src"));
        assert!(!ROUGH.contains("<script src"));
    }

    /// Writes a demo to temp — run with `--nocapture` and open the printed path
    /// in a browser to eyeball the player (manual smoke aid, always passes).
    #[test]
    fn demo_html_for_manual_check() {
        let html = build_visual_html(SAMPLE).unwrap();
        let p = std::env::temp_dir().join("usta-visual-demo.html");
        std::fs::write(&p, html).unwrap();
        println!("demo: {}", p.display());
    }

    #[test]
    fn build_escapes_script_closing_tag_in_captions() {
        let json = r#"[{"caption":"the </script> tag closes it","ops":[]}]"#;
        let html = build_visual_html(json).unwrap();
        // The raw breakout sequence must not survive into the document …
        assert!(!html.contains("</script> tag closes it"));
        // … it is escaped instead.
        assert!(html.contains("<\\/script> tag closes it"));
    }

    #[test]
    fn parse_show_variants() {
        assert_eq!(parse_show_command("/show"), Some(None));
        assert_eq!(parse_show_command("  /show  "), Some(None));
        assert_eq!(parse_show_command("/show tcp handshake"), Some(Some("tcp handshake".to_string())));
        assert_eq!(parse_show_command("/show  dns  "), Some(Some("dns".to_string())));
        assert_eq!(parse_show_command("/showx"), None);
        assert_eq!(parse_show_command("show"), None);
        assert_eq!(parse_show_command("/watch"), None);
        // Case-insensitive command token; argument casing preserved.
        assert_eq!(parse_show_command("/Show"), Some(None));
        assert_eq!(parse_show_command("/SHOW DNS Kaydı"), Some(Some("DNS Kaydı".to_string())));
    }

    #[test]
    fn visual_system_carries_schema_and_pedagogy() {
        let s = visual_system();
        for needle in ["JSON array", "caption", "\"op\"", "node", "arrow", "packet",
                       "6-12 scenes", "ONE idea", "same language as the user", "800", "450",
                       "8px grid", "focal point", "3 words"] {
            assert!(s.contains(needle), "visual_system missing: {needle}");
        }
        // The model must NOT be told to write files or HTML.
        assert!(!s.contains("<html"));
        assert!(!s.contains("write the file"));
    }

    #[test]
    fn visual_path_shape() {
        let p = visual_path(std::path::Path::new("/proj"), "rust", "How ownership works!");
        let s = p.to_string_lossy();
        assert!(s.starts_with("/proj/.usta/visuals/rust/"));
        assert!(s.ends_with(".html") && s.contains("how-ownership-works"));
    }

    // --- extract_show_marker (Görev 4) ---------------------------------

    #[test]
    fn extract_show_marker_strips_trailing_marker_and_returns_topic() {
        let (clean, topic) = extract_show_marker("TCP is a handshake.\n[[show: tcp handshake]]");
        assert_eq!(clean, "TCP is a handshake.");
        assert_eq!(topic, Some("tcp handshake".to_string()));
    }

    #[test]
    fn extract_show_marker_no_marker_returns_unchanged() {
        let text = "Just a plain explanation, nothing more.";
        let (clean, topic) = extract_show_marker(text);
        assert_eq!(clean, text);
        assert_eq!(topic, None);
    }

    #[test]
    fn extract_show_marker_is_case_insensitive_on_show() {
        let (clean, topic) = extract_show_marker("Here it is.\n[[SHOW: DNS records]]");
        assert_eq!(clean, "Here it is.");
        assert_eq!(topic, Some("DNS records".to_string()));

        let (clean2, topic2) = extract_show_marker("Here it is.\n[[Show: dns]]");
        assert_eq!(clean2, "Here it is.");
        assert_eq!(topic2, Some("dns".to_string()));
    }

    #[test]
    fn extract_show_marker_mid_text_not_last_line_is_untouched() {
        // The marker text appears, but NOT alone on the final line — left as-is.
        let text = "Check this [[show: tag]] inline mention out.";
        let (clean, topic) = extract_show_marker(text);
        assert_eq!(clean, text);
        assert_eq!(topic, None);

        // Marker-shaped line exists, but it's not the LAST line — untouched.
        let text2 = "intro\n[[show: topic]]\nmore text after the marker";
        let (clean2, topic2) = extract_show_marker(text2);
        assert_eq!(clean2, text2);
        assert_eq!(topic2, None);
    }

    #[test]
    fn extract_show_marker_multiple_markers_last_wins_all_stripped() {
        let text = "part one\n[[show: cats]]\npart two\n[[show: dogs]]";
        let (clean, topic) = extract_show_marker(text);
        assert_eq!(clean, "part one\npart two");
        assert_eq!(topic, Some("dogs".to_string()));
    }

    #[test]
    fn extract_show_marker_tolerates_trailing_whitespace_on_marker_line() {
        let (clean, topic) = extract_show_marker("done explaining\n[[show: dns]]   ");
        assert_eq!(clean, "done explaining");
        assert_eq!(topic, Some("dns".to_string()));
    }

    #[test]
    fn extract_show_marker_marker_only_reply_yields_synthetic_standin() {
        // A reply that is NOTHING but the marker must not produce an empty
        // clean text — an empty assistant message would 400 the next API turn.
        let (clean, topic) = extract_show_marker("[[show: tcp handshake]]");
        assert_eq!(clean, "(visual explainer: tcp handshake)");
        assert_eq!(topic, Some("tcp handshake".to_string()));

        // Same when only whitespace / blank lines surround the marker.
        let (clean2, topic2) = extract_show_marker("\n  \n[[show: dns]]   ");
        assert_eq!(clean2, "(visual explainer: dns)");
        assert_eq!(topic2, Some("dns".to_string()));
    }

    #[test]
    fn extract_show_marker_empty_topic_is_rejected() {
        // `[[show:]]` (no topic) is not a marker — text untouched, no trigger.
        let text = "explanation\n[[show:]]";
        let (clean, topic) = extract_show_marker(text);
        assert_eq!(clean, text);
        assert_eq!(topic, None);

        // Colon followed by only whitespace is equally empty.
        let text2 = "explanation\n[[show:   ]]";
        let (clean2, topic2) = extract_show_marker(text2);
        assert_eq!(clean2, text2);
        assert_eq!(topic2, None);
    }

    #[test]
    fn extract_show_marker_turkish_topic() {
        let (clean, topic) = extract_show_marker("işte bu.\n[[show: linux dosya ağacı]]");
        assert_eq!(clean, "işte bu.");
        assert_eq!(topic, Some("linux dosya ağacı".to_string()));
    }
}
