//! Visual explainer (/show): embedded HTML skeleton (player shell) + vendored
//! anime.js tween layer + model-produced scene JSON. The model writes ONLY the
//! scene array; the shell validates and injects it.

use anyhow::{bail, Context, Result};

const SKELETON: &str = include_str!("visual_skeleton.html");
const ANIME: &str = include_str!("vendor/anime.min.js");
const PLACEHOLDER_SCENES: &str = "/*__SCENES__*/[]";
const PLACEHOLDER_ANIME: &str = "/*__ANIME__*/";

/// Validate the scene JSON and inject anime.js + scenes into the skeleton.
/// Errors: not JSON, not an array, empty array, any scene missing a `caption`.
#[allow(dead_code)]
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
        .replacen(PLACEHOLDER_ANIME, ANIME, 1)
        .replacen(PLACEHOLDER_SCENES, &v.to_string(), 1))
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
        assert!(SKELETON.contains("prefers-color-scheme"));
        for marker in ["id=\"prev\"", "id=\"play\"", "id=\"next\"", "id=\"caption\"", "<svg"] {
            assert!(SKELETON.contains(marker), "skeleton missing {marker}");
        }
        // Offline guarantee: no external loads at runtime. (Plain `http://` cannot be
        // asserted away — the SVG namespace URI legitimately contains it.)
        assert!(!SKELETON.contains("<script src"));
        assert!(!SKELETON.contains("<link "));
        assert!(!SKELETON.contains("fetch("));
        assert!(!ANIME.contains("<script src"));
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
}
