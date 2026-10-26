# Image paste — `claude` CLI spike

Date: 2026-10-26 · claude CLI 2.1.310 · model `haiku` (resolved to `claude-haiku-5-5`)
Scratch dir: `/private/tmp/claude-502/spike` (not in repo). Cost per call ≈ $0.003–0.006.

## Verdict
**Works.** `claude -p --input-format stream-json --output-format stream-json --verbose` accepts a user
message whose `content` is an array with a base64 `image` block plus a `text` block. The model described
the image correctly (top red / bottom blue test PNG, 200×100). `--resume <session-id>` works with it, in
both directions (session created by plain `--output-format json`, then resumed with a stream-json image
turn; and a later text-only turn recalled the image).

## Exact working command (matches usta's current flags + the three stream-json flags)

```
claude -p \
  --input-format stream-json --output-format stream-json --verbose \
  --append-system-prompt "<system>" \
  --model haiku --allowedTools WebSearch,WebFetch --setting-sources "" \
  [--resume <session-id>]  < in.jsonl
```

`in.jsonl` — ONE line of JSON, then stdin EOF (usta already closes stdin after writing):

```json
{"type":"user","message":{"role":"user","content":[{"type":"image","source":{"type":"base64","media_type":"image/png","data":"<BASE64>"}},{"type":"text","text":"Describe this image: its colors by position. One sentence."}]}}
```

## Constraints discovered (exact error strings)
- `--output-format stream-json` with `-p` requires `--verbose`:
  `Error: When using --print, --output-format=stream-json requires --verbose`
- `--input-format stream-json` requires stream-json output:
  `Error: --input-format=stream-json requires output-format=stream-json.` (so no `--output-format json` + image input)
- Empty stdin with no prompt arg: `Error: Input must be provided either through stdin or as a prompt argument when using --print`
- A bad `--resume` id in stream-json mode prints a single `{"type":"result","subtype":"error_during_execution","is_error":true,...}` line (no `result` field) and exits rc=1.

## Output parsing vs current json mode
Current: stdout is one JSON object `{result, session_id, usage, ...}`.
stream-json: stdout is NDJSON (hook/ui_status/init/thinking_tokens/assistant/rate_limit_event lines, ~11 lines),
the **last line** is `{"type":"result","subtype":"success","is_error":false,"result":"<text>","session_id":"...","usage":{...},...}`.
That final line has the same `result` / `session_id` / `usage` fields, so the existing
`backend::parse_cli_output(line)` can parse it unchanged once the `"type":"result"` line is picked out.
Observed: `session_id` is stable across `--resume` (same id returned). `usage.input_tokens` + cache fields present
(image turn: input 2, cache_creation 28437, cache_read 11335 with the plugin-heavy default; usta's
`--setting-sources ""` keeps it small).

Sample result line (trimmed): `{"type":"result","subtype":"success","is_error":false,"result":"Top red, bottom blue; two equal horizontal bands, red (~top half) over blue (~bottom half), no other colors or elements.","session_id":"375a80bc-24e7-4d0c-b629-05aeae03d351","stop_reason":"end_turn","usage":{...}}`

Resume check (session `852d658a-…` created with "Remember the word PELICAN", then resumed with the image turn
+ question) → `"Red and blue. The word you asked me to remember was **PELICAN**."`

## Alternative (not chosen): file path + `Read`
`claude -p --allowedTools Read "Read /abs/t.png and describe it"` — one attempt in the spike returned
non-JSON output (not investigated further); it would also require allowing `Read`, which breaks the
"Usta doesn't touch files" tool-level guarantee (`CLI_ALLOWED_TOOLS`). Rejected: stream-json works and keeps
the tool allow-list unchanged.

## Design consequences
- Use stream-json **only when the turn has images**; text-only turns keep the existing `--output-format json` path (zero regression risk).
- Images ride only on the last user message. On the no-resume path the transcript is plain text, so earlier
  turns' images are not re-sent (they show only as `[Image #N]` tokens) — acceptable for the stale-session fallback.
- Error detection: in stream mode check `is_error` on the result line (json mode path never did).
