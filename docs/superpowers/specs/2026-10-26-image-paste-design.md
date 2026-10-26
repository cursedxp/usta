# Image paste (screenshots) — Design

Date: 2026-10-26 · Status: approved · Branch: `image-paste` (cut from `fullscreen` after it lands)

## Goal
User can attach screenshots/images to a message, like Claude Code / opencode.

## Decisions (user-approved)
1. **Two inputs:** Ctrl+V (clipboard read) and drag-and-drop (terminal pastes a file path).
2. **Both backends** carry images: Anthropic API and `claude` CLI.

## Behavior
- **Ctrl+V:** read clipboard via `arboard`. Image present → insert `[Image #N]` token in the editor and store the image. No image → paste clipboard text as today.
- **Drag-and-drop:** an `Event::Paste` whose trimmed content is a single existing path (quotes / backslash-escaped spaces handled) with extension png/jpg/jpeg/gif/webp → replaced by `[Image #N]` token. Otherwise normal text paste.
- **Delete:** if a `[Image #N]` token is removed from the editor text, that image is dropped from the message at submit time (submit sends only images whose token remains).
- **Send:** message = text + images.
  - API: `content` becomes an array of blocks (`image` base64 + `text`); plain-text messages may stay string.
  - CLI: mechanism to be verified by a spike before planning tasks (candidates: `--input-format stream-json` with image content blocks; or temp file path + allowing `Read`). If no mechanism works, stop and report to user with a fallback proposal.
- **Chat echo:** user echo shows a line per image, e.g. `🖼 image 1 (1280×720)`. No inline image rendering.
- **Size limit:** images whose long edge exceeds the API's recommended max (verify current limit) are downscaled before encoding; encode PNG/JPEG preserving type where possible.
- **History:** images stay in the conversation (API: kept in `messages`; CLI: carried by `--resume`).

## Out of scope
Inline image preview in terminal, image output from the model, non-image files (PDF etc.).

## Testing
- Unit: path detection (quotes, escaped spaces, nonexistent file, non-image ext), token↔image reconciliation on submit, API request serialization with image block, downscale math.
- Integration spike result recorded for CLI.
- Manual: iTerm2/Ghostty — Ctrl+V screenshot (Cmd+Ctrl+Shift+4), Finder drag, delete token, both backends answer about the image.
