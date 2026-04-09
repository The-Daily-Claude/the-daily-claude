# The Daily Claude (public monorepo)

Public monorepo for The Daily Claude. Home of **Trawl** — the session miner CLI that extracts anonymized, scored entries from Claude Code session JSONL files. Future slots in this monorepo: a static site (`site/`), backend API, web frontend. Shipped open source under AGPL-3.0-or-later. Copyright holder: **La Bande à Bonnot OÜ**.

## Session Continuity

Read `docs/HANDOFF.md` at session start — it has current state, priorities, and known issues. Update it before context compaction or session end.

## Working Style

- **Ship, don't narrate.** Lead with the action, not the reasoning. No preamble, no trailing "hope this helps", no "let me check" prefaces. If you're about to summarize what you just did, delete the summary.
- **Terse over polite.** One-sentence acknowledgements beat paragraph explanations. Milestones get ~5-line status updates, not essays.
- **Never `git add .` / `git add -A`.** The working tree routinely has unrelated dirty files. Enumerate the files you touched and stage them explicitly.
- **Preserve quota via subagents.** Delegate repo research, prose drafting, and bulk file edits to subagents (`Agent` tool) or to Gemini/Codex/OpenCode CLIs. Reserve the main context for orchestration, commits, and user-facing responses.
- **Autonomous loops are wrong for destructive refactors.** Ralph-loop / SLFG / auto-work-through-the-plan workflows are fine for greenfield features, NOT for rewriting security-sensitive code (PII, credentials, file deletion, schema changes). Stop and ask when the diff would be hard to reverse.
- **Bot reviews get triaged, not blindly applied.** When CodeRabbit / Gemini / Copilot flag something, decide: fix inline, defer to a todo with rationale, or reject with a reply explaining why. Reflexive acceptance is how fixes introduce new bugs (see `docs/solutions/process-patterns/review-loop-discipline-20260408.md`).
- **Approved cleanup scope includes same-class follow-ons.** When a subagent surfaces additional entries with the same bug, more files needing the same fix, or more tests of the same kind, dispatch the next pass immediately. Don't return with "want me to dispatch the sweep for the remaining files?" — that's permission friction inside a scope the user already approved. Save explicit permission gates for genuinely new directions, destructive choices outside the approved scope, or branching architectural decisions.
- **PII sweeps span the whole repo, not just one crate.** Home paths and identifying strings live in `CLAUDE.md`, `HANDOFF.md`, `todos/`, `docs/`, and crate sources too — `git grep` across the whole tree, not a single directory. A sweep isn't done until a whole-repo grep for the leak patterns comes back empty. See `docs/solutions/process-patterns/pii-sweeps-span-whole-repo-20260409.md`.
- **LLM for cognition, code for deterministic transformation.** Don't hand the model problems that aren't classification. Path slugification, regex-able patterns, byte-level ops, value normalization — write code, not prompts. See Key Principles below.
- **All knowledge lives in the repo.** If a learning is worth keeping for the next session, it belongs in `docs/solutions/`, `docs/plans/`, `docs/HANDOFF.md`, or this file. What isn't committed doesn't exist. Auto-memory is a local cache — a session on a different machine will not see it.

## Tools

- **Cargo** for the Rust workspace (`crates/trawl`, future public crates)
- **Claude Code CLI** (`claude -p --model sonnet|haiku`) for Trawl's extractor and tokeniser stages

## Quick Commands

| Task | Command |
|------|---------|
| Build | `cargo build` |
| Check | `cargo check` |
| Test | `cargo test -p trawl` |
| Trawl stats | `cargo run -- --stats ~/.claude/projects/` |
| Trawl dry-run | `cargo run -- --dry-run <session.jsonl>` |
| Trawl extract | `cargo run -- <session-path> -o <entries-dir>` |

## Project Structure

```
crates/
  trawl/              # Session miner (lib + binary)
# site/               # (future) public static site (HTML/CSS/JS)
# backend/            # (future) API
# frontend/           # (future) web UI
docs/
  brainstorms/        # Requirements docs
  plans/              # Implementation plans
  solutions/          # Compound learnings (engineering subset)
todos/                # Outstanding work
```

## Key Principles

### Zero Framework Cognition
Don't build regex classifiers or decision trees around LLM calls. Send the data to Haiku and let it decide. The model IS the framework. See `docs/solutions/design-decisions/zero-framework-cognition-20260320.md`.

### LLM for Cognition, Code for Transformation
The inverse boundary of ZFC: don't use the LLM for deterministic transformations. Path slugification, regex-able patterns, byte-level ops, value normalization — write code, not prompts. If you find yourself writing "format this as a slug" or "extract the hostname from this URL" in a prompt, delete the instruction and write a function. When an LLM-generated field is wrong, the first question is not *"how do I tweak the prompt?"* but *"should this field have been LLM-generated at all?"* See `docs/solutions/design-decisions/llm-cognition-code-transformation-20260409.md`.

### Verbatim Quotes, Not Summaries
Entry bodies can have editorial context, but the appendix must contain the actual anonymized exchange verbatim. Trawl's extractor and tokeniser prompts are explicit about this — verbatim fidelity is a pipeline invariant, not a content-side choice. See `docs/solutions/content-pipeline/verbatim-quotes-not-summaries-20260320.md`.

### Simplicity First
Ship the miner, then tune it. Don't architecture a CLI tool like a production microservice. See `docs/solutions/process-patterns/simplicity-over-architecture-20260320.md`.

## Conventional Commits

All commits follow [Conventional Commits](https://www.conventionalcommits.org/):
- `feat(trawl):` — new Trawl features
- `fix:` — bug fixes
- `docs:` — documentation and compound learnings

## Named References

- **The Trawl** — session miner, named after Alastair Reynolds' memory extraction device in Revelation Space
- **The Daily Claude** — the meme publication this tooling serves
