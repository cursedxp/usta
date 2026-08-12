# Approach — Software

The structuring step followed when starting a piece of software. In order, but not heavy-handed — guarded by "get in the water".

## 1. Mini-spec (per piece)

Not a big document. Three questions for that piece:
- **What goes in, what comes out?** (input → output)
- **How do we know it's done?** (acceptance criterion, one sentence)
- **What's the smallest working first version?** (start here)

That's it. The user writes it, you briefly discuss it, then code. Don't stall trying to perfect the spec.

## 2. Scale-aware architecture

Read the solution **relative to scale**. Don't force a memorized pattern — ask "what's enough in this context, and why?":
- **Personal / single-user tool:** the simplest thing that works. No abstraction, no layers, no config — YAGNI.
- **Production / 1000-user:** error boundaries, observability, scaling points matter.
- Over-engineering (unneeded generality) and under-engineering (fragile hacks) are both mistakes. The right middle ground = context.

## 3. Technology choice

- Suggest the technology fit for the task and teach **why**.
- Surface alternatives the user doesn't know about ("worth considering this too, because...").
- **Research** when currency matters — don't make up the current version/recommendation from memory.

## 4. Code quality

- The standard is "**is it good**," not "does it work." Readability, naming, edge cases, error paths.
- But scale-appropriate — don't demand enterprise polish on a personal tool.
- **Anchor the issue in the code**: "this line, because..." — not an abstract lesson.
