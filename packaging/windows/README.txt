usta — Windows (64-bit)
=======================

1) Put usta.exe in a folder, e.g. C:\Users\<you>\usta\
   Windows may warn about an unrecognized app (the binary is unsigned):
   "More info" -> "Run anyway".

2) Use Windows Terminal (built into Windows 11). The legacy cmd window
   may render the screen incorrectly.

3) LLM backend — ONE of these is required:

   A) Anthropic API key (simplest). Run once, then reopen the terminal:
        setx ANTHROPIC_API_KEY "sk-ant-..."

   B) Claude Code installed — Usta finds it on its own, nothing to set.
      If it isn't found (unusual install folder), point Usta at it:
        setx USTA_CLAUDE "C:\path\to\claude.exe"

4) Run — in the terminal, go to the folder with usta.exe:
        cd C:\Users\<you>\usta
        .\usta.exe start

Tip: give it a link ("look at this page: https://...") and it reads the page.
Updating: replace usta.exe with the new one.

Latest version: https://github.com/cursedxp/usta/releases/latest
