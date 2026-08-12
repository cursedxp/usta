# Visual Explainer (/show) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `/show [topic]` generates an animated, self-contained HTML explainer (embedded skeleton + model-produced scene JSON) and opens it in the browser; Usta proactively suggests `/show` on confusion signals.

**Architecture:** A hand-written HTML/JS animation skeleton is embedded via `include_str!`. The model outputs ONLY a declarative scene-JSON array (validated with serde_json), which the shell injects into the skeleton's `/*__SCENES__*/` placeholder and writes to `.usta/visuals/`. The LLM call is an isolated mini-session (same pattern as slug generation: own system prompt, `backend.reset_session()` after). Both loops intercept `/show` like `/watch` — the line never enters session history.

**Tech Stack:** Rust 2021, serde_json (already a dep), chrono (already a dep), vanilla HTML/CSS/JS (no CDN — fully offline).

**Spec:** `docs/superpowers/specs/2026-08-12-visual-explainer-design.md` — binding contract for schema and flow.

## Global Constraints

- The skeleton HTML is fully self-contained: inline CSS+JS, NO external requests (no CDN, no fonts, no fetch).
- Model output contract: a bare JSON array of scenes (may arrive fenced — strip with existing `progress::clean_markdown_reply`). Invalid JSON / empty array / scene without `caption` → `Err`, no file written, user notified. No auto-retry (v1).
- `/show` is intercepted in BOTH loops before any `session.push_user` — never sent to the main LLM session (like `/watch`). The mini-session uses its own system prompt and calls `backend.reset_session()` afterwards (success, cancel, and error paths alike).
- Usta never opens the browser proactively — SOUL.md only gains a one-line suggestion to offer `/show` on confusion signals.
- No session-history writeback in v1: the /show turn adds nothing to `session` or `recorder`.
- Preserve structural tokens as ever (`## Hedef`, `===DOSYA:`, curriculum statuses) — this feature must not touch them.
- Every task ends green: `cargo build -p usta` and `cargo test -p usta`.

---

### Task 1: Skeleton + scene engine + `build_visual_html`

**Files:**
- Create: `src/visual_skeleton.html`
- Create: `src/visual.rs` (module core: `SKELETON`, `build_visual_html`, tests)
- Modify: `src/main.rs` (add `mod visual;` next to the other `mod` declarations)

**Interfaces:**
- Produces: `pub fn build_visual_html(scenes_json: &str) -> anyhow::Result<String>`; `const SKELETON: &str = include_str!("visual_skeleton.html");` (private). Placeholder contract: skeleton contains exactly one `/*__SCENES__*/[]` token; injection replaces it with the JSON array.
- Consumes: nothing from other tasks.

- [ ] **Step 1: Write the failing tests** — create `src/visual.rs`:

```rust
//! Visual explainer (/show): embedded HTML skeleton + model-produced scene JSON.
//! The model writes ONLY the scene array; the shell validates and injects it.

use anyhow::{bail, Context, Result};

const SKELETON: &str = include_str!("visual_skeleton.html");
const PLACEHOLDER: &str = "/*__SCENES__*/[]";

/// Validate the scene JSON and inject it into the skeleton. Errors: not JSON,
/// not an array, empty array, or any scene missing a `caption` string.
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
    Ok(SKELETON.replacen(PLACEHOLDER, &v.to_string(), 1))
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
    fn build_injects_scenes_into_full_document() {
        let html = build_visual_html(SAMPLE).unwrap();
        assert!(html.contains("<!doctype html>") || html.contains("<!DOCTYPE html>"));
        assert!(html.contains("\"caption\":\"A request travels\""));
        assert!(!html.contains("/*__SCENES__*/"), "placeholder must be consumed");
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
        assert!(SKELETON.contains(PLACEHOLDER));
        assert!(SKELETON.contains("prefers-color-scheme"));
        for marker in ["id=\"prev\"", "id=\"play\"", "id=\"next\"", "id=\"caption\"", "<svg"] {
            assert!(SKELETON.contains(marker), "skeleton missing {marker}");
        }
        // Offline guarantee: no external URLs.
        assert!(!SKELETON.contains("http://") && !SKELETON.contains("https://"));
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
```

- [ ] **Step 2: Create a stub skeleton so it compiles, run tests to verify they FAIL on content** — write `src/visual_skeleton.html` containing only `/*__SCENES__*/[]`, add `mod visual;` to `src/main.rs`, then:

Run: `cargo test -p usta --lib visual`
Expected: `build_injects_scenes_into_full_document` and `skeleton_is_selfcontained_player` FAIL (no doctype/controls yet); `build_rejects_invalid_json` PASSES (validation logic is complete).

- [ ] **Step 3: Write the real skeleton** — replace `src/visual_skeleton.html` with the complete player below. This is the reference implementation; keep the IDs, the placeholder line, and the op semantics exactly, polish freely.

```html
<!doctype html>
<html lang="en">
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>Usta — visual explainer</title>
<style>
  :root { --bg:#f6f7f9; --fg:#1c1e21; --panel:#fff; --line:#9aa1ac; --accent:#e8862e; --note:#fff6e8; }
  @media (prefers-color-scheme: dark) {
    :root { --bg:#16181d; --fg:#e8eaed; --panel:#1f2229; --line:#8a919e; --accent:#f09040; --note:#2a2620; }
  }
  * { box-sizing:border-box; margin:0 }
  body { background:var(--bg); color:var(--fg); font:16px/1.45 system-ui,sans-serif;
         min-height:100vh; display:flex; flex-direction:column; align-items:center; padding:24px; gap:14px }
  main { width:min(880px,100%); background:var(--panel); border-radius:14px; padding:16px;
         box-shadow:0 4px 24px rgba(0,0,0,.12) }
  svg { width:100%; height:auto; display:block }
  #caption { min-height:3em; text-align:center; font-size:18px; padding:8px 16px; max-width:840px }
  #bar { display:flex; gap:10px; align-items:center }
  button { background:var(--panel); color:var(--fg); border:1px solid var(--line); border-radius:8px;
           padding:8px 16px; font-size:15px; cursor:pointer }
  button:hover { border-color:var(--accent) }
  #counter { opacity:.6; font-variant-numeric:tabular-nums }
  .node rect,.circ circle { fill:var(--panel); stroke:var(--line); stroke-width:2 }
  .hl rect,.hl circle { stroke:var(--accent); stroke-width:3 }
  .lbl { fill:var(--fg); font:14px system-ui,sans-serif; text-anchor:middle; dominant-baseline:middle }
  .arrow { stroke:var(--line); stroke-width:2; fill:none; marker-end:url(#ah) }
  .alabel { fill:var(--fg); font:12px system-ui,sans-serif; text-anchor:middle; opacity:.75 }
  .notebox rect { fill:var(--note); stroke:var(--accent); stroke-width:1 }
  .packet { fill:var(--accent) }
  g { transition:opacity .45s, transform .6s }
  @keyframes pulse { 50% { transform:scale(1.12) } }
  .pulsing { animation:pulse .7s ease-in-out 2; transform-origin:center; transform-box:fill-box }
</style>
<main><svg id="stage" viewBox="0 0 800 450">
  <defs><marker id="ah" viewBox="0 0 10 10" refX="9" refY="5" markerWidth="7" markerHeight="7" orient="auto">
    <path d="M0 0 L10 5 L0 10 z" fill="var(--line)"/></marker></defs>
  <g id="root"></g>
</svg></main>
<div id="caption"></div>
<div id="bar">
  <button id="prev">◀</button>
  <button id="play">▶ play</button>
  <button id="next">▶|</button>
  <span id="counter"></span>
</div>
<script>
"use strict";
const SCENES = /*__SCENES__*/[];
const NS = "http://www.w3.org/2000/svg";
const root = document.getElementById("root");
const cap = document.getElementById("caption");
const counter = document.getElementById("counter");
const meta = {};          // id → {cx, cy} centers for arrows/moves
let cur = -1, playing = false, playToken = 0;

const sleep = ms => new Promise(r => setTimeout(r, ms));
function el(tag, attrs) { const e = document.createElementNS(NS, tag);
  for (const k in attrs) e.setAttribute(k, attrs[k]); return e; }
function centerOf(id) { return meta[id] || { cx: 400, cy: 225 }; }

function makeElement(spec) {
  const g = el("g", { id: "el-" + spec.id });
  if (spec.type === "node") {
    g.classList.add("node");
    g.appendChild(el("rect", { x: spec.x, y: spec.y, width: spec.w || 140, height: spec.h || 70, rx: 10 }));
    const t = el("text", { x: spec.x + (spec.w || 140) / 2, y: spec.y + (spec.h || 70) / 2, class: "lbl" });
    t.textContent = spec.label || ""; g.appendChild(t);
    meta[spec.id] = { cx: spec.x + (spec.w || 140) / 2, cy: spec.y + (spec.h || 70) / 2 };
  } else if (spec.type === "circle") {
    g.classList.add("circ");
    g.appendChild(el("circle", { cx: spec.cx, cy: spec.cy, r: spec.r || 34 }));
    const t = el("text", { x: spec.cx, y: spec.cy, class: "lbl" });
    t.textContent = spec.label || ""; g.appendChild(t);
    meta[spec.id] = { cx: spec.cx, cy: spec.cy };
  } else { // text
    const t = el("text", { x: spec.x, y: spec.y, class: "lbl", "font-size": spec.size || 16 });
    t.textContent = spec.text || spec.label || ""; g.appendChild(t);
    meta[spec.id] = { cx: spec.x, cy: spec.y };
  }
  return g;
}

async function apply(op, animate) {
  const find = id => document.getElementById("el-" + id);
  switch (op.op) {
    case "add": {
      const g = makeElement(op.el);
      if (animate) { g.style.opacity = 0; root.appendChild(g); void g.getBBox(); g.style.opacity = 1; await sleep(480); }
      else root.appendChild(g);
      break; }
    case "remove": {
      const g = find(op.id); if (!g) break;
      if (animate) { g.style.opacity = 0; await sleep(460); }
      g.remove(); break; }
    case "move": {
      const g = find(op.id); if (!g) break;
      const c = centerOf(op.id);
      const nx = op.x !== undefined ? op.x : c.cx, ny = op.y !== undefined ? op.y : c.cy;
      if (!animate) g.style.transition = "none";
      g.style.transform = `translate(${nx - c.cx}px, ${ny - c.cy}px)`;
      // meta keeps the ORIGINAL center + accumulated transform would compound;
      // simplest correct model: moves are absolute per element, so update meta and bake:
      meta[op.id] = { cx: nx, cy: ny };
      if (animate) await sleep(650); else { void g.getBBox(); g.style.transition = ""; }
      break; }
    case "arrow": {
      const a = centerOf(op.from), b = centerOf(op.to);
      const g = el("g", { id: "el-" + op.id });
      const dx = b.cx - a.cx, dy = b.cy - a.cy, len = Math.hypot(dx, dy) || 1;
      const pad = 46; // don't pierce the boxes
      const x1 = a.cx + dx / len * pad, y1 = a.cy + dy / len * pad;
      const x2 = b.cx - dx / len * pad, y2 = b.cy - dy / len * pad;
      const line = el("line", { x1, y1, x2, y2, class: "arrow" });
      g.appendChild(line);
      if (op.label) { const t = el("text", { x: (x1 + x2) / 2, y: (y1 + y2) / 2 - 8, class: "alabel" });
        t.textContent = op.label; g.appendChild(t); }
      meta[op.id] = { cx: (x1 + x2) / 2, cy: (y1 + y2) / 2, x1, y1, x2, y2 };
      if (animate) { const d = Math.hypot(x2 - x1, y2 - y1);
        line.style.strokeDasharray = d; line.style.strokeDashoffset = d;
        root.appendChild(g); void line.getBBox();
        line.style.transition = "stroke-dashoffset .6s"; line.style.strokeDashoffset = 0; await sleep(640);
        line.style.strokeDasharray = ""; }
      else root.appendChild(g);
      break; }
    case "packet": {
      if (!animate) break; // transient — nothing to replay
      const m = meta[op.along]; if (!m || m.x1 === undefined) break;
      const g = el("g", {});
      g.appendChild(el("circle", { cx: 0, cy: 0, r: 7, class: "packet" }));
      if (op.label) { const t = el("text", { x: 0, y: -14, class: "alabel" }); t.textContent = op.label; g.appendChild(t); }
      root.appendChild(g);
      const t0 = performance.now(), dur = 1100;
      await new Promise(res => { (function step(now) {
        const k = Math.min(1, (now - t0) / dur);
        g.setAttribute("transform", `translate(${m.x1 + (m.x2 - m.x1) * k}, ${m.y1 + (m.y2 - m.y1) * k})`);
        k < 1 ? requestAnimationFrame(step) : res(); })(t0); });
      g.remove(); break; }
    case "pulse": {
      if (!animate) break;
      const g = find(op.id); if (!g) break;
      g.classList.add("pulsing"); await sleep(1450); g.classList.remove("pulsing"); break; }
    case "highlight": {
      const g = find(op.id); if (!g) break;
      g.classList.toggle("hl", op.on !== false); break; }
    case "note": {
      const g = el("g", { id: "el-" + (op.id || "note") });
      g.classList.add("notebox");
      const w = Math.max(120, (op.text || "").length * 7.2 + 24);
      g.appendChild(el("rect", { x: op.x - w / 2, y: op.y - 18, width: w, height: 36, rx: 8 }));
      const t = el("text", { x: op.x, y: op.y, class: "lbl", "font-size": 13 });
      t.textContent = op.text || ""; g.appendChild(t);
      meta[op.id || "note"] = { cx: op.x, cy: op.y };
      if (animate) { g.style.opacity = 0; root.appendChild(g); void g.getBBox(); g.style.opacity = 1; await sleep(420); }
      else root.appendChild(g);
      break; }
  }
}

async function showScene(i, animate) {
  cur = i;
  cap.textContent = SCENES[i].caption || "";
  counter.textContent = (i + 1) + "/" + SCENES.length;
  for (const op of SCENES[i].ops || []) await apply(op, animate);
}

async function renderUpTo(i) { // deterministic replay, instant
  root.innerHTML = ""; for (const k in meta) delete meta[k];
  for (let s = 0; s < i; s++) { cap.textContent = SCENES[s].caption;
    for (const op of SCENES[s].ops || []) await apply(op, false); }
  await showScene(i, true);
}

function stopPlay() { playing = false; playToken++; document.getElementById("play").textContent = "▶ play"; }

document.getElementById("next").onclick = async () => { stopPlay(); if (cur + 1 < SCENES.length) await showScene(cur + 1, true); };
document.getElementById("prev").onclick = async () => { stopPlay(); if (cur > 0) await renderUpTo(cur - 1); };
document.getElementById("play").onclick = async function () {
  if (playing) { stopPlay(); return; }
  playing = true; this.textContent = "❚❚ pause"; const my = ++playToken;
  if (cur + 1 >= SCENES.length) await renderUpTo(0).then(() => sleep(SCENES[0].duration || 3500));
  while (playing && my === playToken && cur + 1 < SCENES.length) {
    await showScene(cur + 1, true);
    await sleep(SCENES[cur].duration || 3500);
  }
  if (my === playToken) stopPlay();
};

if (SCENES.length) showScene(0, true); else cap.textContent = "no scenes";
</script>
```

**Known simplification to keep (v1):** `move` uses CSS `transform translate` deltas computed from `meta`; because `meta` is updated to the new absolute center, arrows drawn AFTER a move stay correct, and replay is deterministic since ops replay in order. Do not "improve" this into compounding transforms.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p usta --lib visual`
Expected: all 4 PASS.

- [ ] **Step 5: Manual eyeball of the player**

Run: `cargo test -p usta --lib visual::tests::demo_html_for_manual_check -- --nocapture`
Open the printed path in a browser. Check: 3 scenes; ▶ play auto-advances; ◀ replays deterministically; packet dot travels both arrows; dark/light theme follows OS. (If no browser in the execution environment, note it in the report — the human verifies.)

- [ ] **Step 6: Commit**

```bash
git add src/visual.rs src/visual_skeleton.html src/main.rs
git commit -m "usta: görsel iskelet + sahne motoru — build_visual_html, gömülü offline oynatıcı"
```

---

### Task 2: Command plumbing — parse, prompt, path, browser

**Files:**
- Modify: `src/visual.rs` (add `parse_show_command`, `visual_system`, `visual_path`, `open_in_browser` + tests)

**Interfaces:**
- Produces:
  - `pub fn parse_show_command(line: &str) -> Option<Option<String>>` — `Some(None)` for bare `/show`, `Some(Some(topic))` for `/show <topic>`, `None` otherwise.
  - `pub fn visual_system() -> String` — mini-session system prompt.
  - `pub fn visual_path(project_root: &std::path::Path, topic: &str, concept: &str) -> std::path::PathBuf` — `.usta/visuals/<topic>/<YYYY-MM-DD-HHMMSS>-<slug>.html`, slug via `crate::slugify_topic`.
  - `pub fn open_in_browser(path: &std::path::Path) -> bool` — spawn `open` (macOS) / `xdg-open` (other unix); `false` on spawn failure.
- Consumes: `crate::slugify_topic` (exists in main.rs), `chrono::Local`.

- [ ] **Step 1: Write the failing tests** (append to `src/visual.rs mod tests`):

```rust
#[test]
fn parse_show_variants() {
    assert_eq!(parse_show_command("/show"), Some(None));
    assert_eq!(parse_show_command("  /show  "), Some(None));
    assert_eq!(parse_show_command("/show tcp handshake"), Some(Some("tcp handshake".to_string())));
    assert_eq!(parse_show_command("/show  dns  "), Some(Some("dns".to_string())));
    assert_eq!(parse_show_command("/showx"), None);
    assert_eq!(parse_show_command("show"), None);
    assert_eq!(parse_show_command("/watch"), None);
}

#[test]
fn visual_system_carries_schema_and_pedagogy() {
    let s = visual_system();
    for needle in ["JSON array", "caption", "\"op\"", "node", "arrow", "packet",
                   "6-12 scenes", "ONE idea", "same language as the user", "800", "450"] {
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
    assert!(s.ends_with("-how-ownership-works.html") || s.contains("how-ownership-works"));
}
```

- [ ] **Step 2: Run to verify they fail**

Run: `cargo test -p usta --lib visual`
Expected: FAIL — functions missing.

- [ ] **Step 3: Implement** (append to `src/visual.rs`):

```rust
/// `/show` → Some(None) (visualize the last explanation); `/show <topic>` →
/// Some(Some(topic)). Anything else → None. Slash lines never reach the LLM session.
pub fn parse_show_command(line: &str) -> Option<Option<String>> {
    let t = line.trim();
    if t == "/show" { return Some(None); }
    let rest = t.strip_prefix("/show ")?;
    let topic = rest.trim();
    if topic.is_empty() { Some(None) } else { Some(Some(topic.to_string())) }
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
```

Adjust the `visual_system` literal so every substring the Step-1 test asserts is present verbatim (e.g. include the phrases `JSON array`, `6-12 scenes`, `ONE idea`, `same language as the user`) — prompt and test must agree.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p usta --lib visual`
Expected: PASS (all visual tests).

- [ ] **Step 5: Commit**

```bash
git add src/visual.rs
git commit -m "usta: /show komut altyapısı — parse, sistem promptu, dosya yolu, tarayıcı açma"
```

---

### Task 3: Loop integration + help + SOUL proactive line

**Files:**
- Modify: `src/main.rs` (shared runner `run_show` + plain-loop intercept)
- Modify: `src/tui/run.rs` (TUI intercept via `ask_live`)
- Modify: `src/help.rs` (add `/show` line + test needle)
- Modify: `SOUL.md` (one-line proactive suggestion)

**Interfaces:**
- Consumes: Task 2's `parse_show_command`, `visual_system`, `visual_path`, `open_in_browser`; Task 1's `build_visual_html`; existing `progress::clean_markdown_reply`, `backend.reset_session()`, `ask_usta` (main.rs), `ask_live`/`AskOutcome` (run.rs), `Session::history()`.
- Produces: `pub(crate) fn last_assistant_text(session: &Session) -> Option<String>` in `main.rs` — helper both loops use to find the concept for bare `/show`. (Look at `src/session.rs` for how history messages store role + text; implement accordingly — likely `session.history().iter().rev().find(|m| m.role == "assistant")`.)

- [ ] **Step 1: Add the helper + build the mini-session user message.** In `src/main.rs`:

```rust
/// Last assistant reply in this session — the concept a bare `/show` visualizes.
pub(crate) fn last_assistant_text(session: &Session) -> Option<String> {
    session
        .history()
        .iter()
        .rev()
        .find(|m| m.role == "assistant")
        .map(|m| m.text_for_display()) // ← adapt to the real accessor in anthropic.rs/session.rs
}

/// Compose the visual mini-session user turn. `explicit` = `/show <topic>` argument.
pub(crate) fn show_request(explicit: Option<String>, last_reply: Option<&str>) -> Option<String> {
    match (explicit, last_reply) {
        (Some(t), last) => Some(match last {
            Some(l) => format!(
                "Create scenes that visually explain: {t}\n\nRecent explanation for context:\n{l}"
            ),
            None => format!("Create scenes that visually explain: {t}"),
        }),
        (None, Some(l)) => Some(format!(
            "Create scenes that visually explain the following explanation:\n{l}"
        )),
        (None, None) => None, // nothing to visualize yet
    }
}
```

Check `src/anthropic.rs`/`src/session.rs` for the actual `Message` shape (role field + content accessor) and adapt `last_assistant_text` — do not invent an accessor; read the struct.

Unit tests for `show_request` (in `main.rs mod tests`):

```rust
#[test]
fn show_request_composition() {
    assert!(show_request(None, None).is_none());
    let bare = show_request(None, Some("ownership explained")).unwrap();
    assert!(bare.contains("ownership explained"));
    let explicit = show_request(Some("dns".into()), Some("prior".into())).unwrap();
    assert!(explicit.contains("dns") && explicit.contains("prior"));
    let cold = show_request(Some("dns".into()), None).unwrap();
    assert!(cold.contains("dns"));
}
```

- [ ] **Step 2: Plain-loop intercept.** In `run_plain_loop`'s `InputEvent::Line` arm, next to `/watch` and `/help` (before `/quit`, before `push_user`):

```rust
if let Some(arg) = visual::parse_show_command(&line) {
    match show_request(arg, last_assistant_text(session).as_deref()) {
        None => ui::notice("nothing to visualize yet — explain something first, or use /show <topic>"),
        Some(req) => {
            match ask_usta(backend, &visual::visual_system(), &[Message::user(req.as_str())]).await {
                Ok(reply) => {
                    let json = progress::clean_markdown_reply(&reply.text);
                    match visual::build_visual_html(&json) {
                        Ok(html) => {
                            let path = visual::visual_path(project_root, topic, &req);
                            if let Some(dir) = path.parent() { let _ = std::fs::create_dir_all(dir); }
                            match std::fs::write(&path, html) {
                                Ok(()) => {
                                    let opened = visual::open_in_browser(&path);
                                    ui::notice(&format!("visual saved: {}{}", path.display(),
                                        if opened { "" } else { " (open it in your browser)" }));
                                }
                                Err(e) => ui::warn(&format!("error: {e}")),
                            }
                        }
                        Err(e) => ui::warn(&format!("visual generation failed ({e}) — try /show again")),
                    }
                }
                Err(e) => ui::warn(&format!("error: {e}")),
            }
            backend.reset_session(); // mini-session must not leak into the CLI session (slug parity)
        }
    }
    let _ = ready_tx.send(());
    continue;
}
```

Note: `visual_path`'s concept argument — for bare `/show` the `req` string is long; pass the explicit topic when present, else a fixed `"visual"` concept: adapt to `let concept = arg_clone.unwrap_or_else(|| "visual".into())` captured BEFORE `show_request` consumes `arg` (clone it). Keep filenames short.

- [ ] **Step 3: TUI intercept.** In `src/tui/run.rs` main loop `Action::Submit(line)` arm, next to `/watch` and `/help`:

```rust
if let Some(arg) = crate::visual::parse_show_command(&line) {
    page_user_echo(&mut tui, &line)?;
    let concept = arg.clone().unwrap_or_else(|| "visual".to_string());
    match crate::show_request(arg, crate::last_assistant_text(&session).as_deref()) {
        None => page_notice(&mut tui, "nothing to visualize yet — explain something first, or use /show <topic>")?,
        Some(req) => {
            match ask_live(&mut tui, &mut editor, &mut events, backend,
                           &crate::visual::visual_system(),
                           &[Message::user(req.as_str())], last_tokens).await {
                Ok(AskOutcome::Reply(reply)) => {
                    let json = crate::progress::clean_markdown_reply(&reply.text);
                    match crate::visual::build_visual_html(&json) {
                        Ok(html) => {
                            let path = crate::visual::visual_path(project_root, &topic, &concept);
                            if let Some(dir) = path.parent() { let _ = std::fs::create_dir_all(dir); }
                            match std::fs::write(&path, html) {
                                Ok(()) => {
                                    let opened = crate::visual::open_in_browser(&path);
                                    page_notice(&mut tui, &format!("visual saved: {}{}", path.display(),
                                        if opened { "" } else { " (open it in your browser)" }))?;
                                }
                                Err(e) => page_notice(&mut tui, &format!("error: {e}"))?,
                            }
                        }
                        Err(e) => page_notice(&mut tui, &format!("visual generation failed ({e}) — try /show again"))?,
                    }
                }
                Ok(AskOutcome::Cancelled) => page_notice(&mut tui, "visual generation cancelled")?,
                Err(e) => page_notice(&mut tui, &format!("error: {e}"))?,
            }
            backend.reset_session(); // all paths — slug parity
        }
    }
    continue;
}
```

Borrow care: `topic` in `run` may be a `String` consumed earlier — check how the existing code references it near the main loop (it's the `topic` binding from the match) and pass `&topic`. If `session` is borrowed mutably elsewhere in the arm, hoist `let last = crate::last_assistant_text(&session);` before the `match`.

- [ ] **Step 4: help.rs.** Add to the In-session commands block of `help_text()`:

```
  /show [topic]    animated visual explainer (opens in browser)
```

and extend the help test's needle list with `"/show [topic]"`.

- [ ] **Step 5: SOUL.md proactive line.** In SOUL.md's confusion-signal bullet ("Anlaşılmadı sinyali" — now in English after the brain translation; find the bullet about re-explaining with a different analogy), append one sentence:

```
If the concept is visual or spatial (flows, architectures, protocols, layouts), offer the animation: "want me to show this visually? type /show".
```

Content-only edit; no structural token touched.

- [ ] **Step 6: Full suite + build**

Run: `cargo test -p usta && cargo build -p usta`
Expected: PASS, no warnings.

- [ ] **Step 7: Manual smoke** (interactive — defer to human if no TTY): `cargo install --path .`, open a session, ask something, type `/show` → browser opens with an animated explainer; `/show how does dns work` → explicit topic; Esc during generation cancels; `/help` lists `/show`.

- [ ] **Step 8: Commit**

```bash
git add src/main.rs src/tui/run.rs src/help.rs SOUL.md
git commit -m "usta: /show entegrasyonu — iki loop, help, SOUL proaktif öneri"
```

---

## Self-Review

**Spec coverage:** skeleton+engine (T1), JSON contract+validation (T1), parse/system-prompt/path/browser (T2), both-loop intercepts + mini-session isolation + reset_session on all paths (T3), no-context fallback notice (T3), help line (T3), SOUL proactive one-liner (T3), error paths (invalid JSON / open failure / Esc cancel) (T3). Out-of-scope items in spec (listing command, history writeback, retry) deliberately absent. ✓

**Placeholder scan:** `/*__SCENES__*/` is the designed injection token, not a plan placeholder. Two adapt-to-reality notes are explicit implementer instructions with the file to read named (`Message` accessor in session.rs/anthropic.rs; `topic` borrow in run.rs) — flagged, not vague. The deliberate CSS typo is called out with its exact fix. ✓

**Type consistency:** `parse_show_command -> Option<Option<String>>` used identically in both loops; `build_visual_html(&str) -> Result<String>` from T1 consumed in T3; `visual_path(&Path, &str, &str)` matches both call sites; `show_request`/`last_assistant_text` defined in T3-Step1 and used in T3-Steps 2-3. ✓
