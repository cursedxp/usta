usta — macOS (universal: Apple Silicon + Intel)
===============================================

1) Put the binary on your PATH, e.g.:
        mkdir -p ~/.local/bin && mv usta ~/.local/bin/
   (make sure ~/.local/bin is in your PATH)

2) The binary is not notarized. If macOS blocks it ("cannot be opened"
   / "unidentified developer"), clear the download quarantine once:
        xattr -d com.apple.quarantine ~/.local/bin/usta

3) LLM backend — ONE of these is required:
   A) Claude Code — found automatically (PATH or its install folders, e.g.
      ~/.local/bin), no key needed. Lives somewhere unusual? Point usta at it:
        export USTA_CLAUDE=/path/to/claude
   B) export ANTHROPIC_API_KEY=sk-ant-...

4) Run:
        usta start

Tip: give it a link ("look at this page: https://...") and it reads the page.
Updating: replace the binary with the new one.

Latest version: https://github.com/cursedxp/usta/releases/latest
