# PII sweeps span the whole repo, not just `content/`

**Date:** 2026-04-09
**Context:** 2026-04-09 trawl cleanup aftermath
**Category:** process-patterns

## The rule

When doing a PII / anonymization sweep, scope it across **every tracked file in the repo**, not just `content/`. Home paths, org names, identifying strings, and user references live in `CLAUDE.md`, `HANDOFF.md`, scripts, crate sources, plans, solution docs, and todos — not just content entries.

Use `git grep` (with path negation where needed) to scope the search. Do NOT rely on `grep -r content/entries/` and call the sweep done.

## How this came up

Commit `2b79256` (2026-04-09) swept `content/entries/*.md` for `/Users/alice`, `-Users-alice-`, `Banade-a-Bonnot`, `lightless-labs`, and related patterns. 149 entries cleaned. The sweep agents reported *"all target patterns at 0"*. Sweep declared complete.

Immediately after, while adding two new rules to `CLAUDE.md` itself in commit `be3605b`, the edit surfaced this line in the "Named References" section:

```
- **The Compilation** — the original 128-entry anthology at
  `/Users/alice/workspaces/compilation/the-compilation.md`
```

The project's own root-level instructions file had the exact leak class we'd just spent hours scrubbing from the content corpus. Caught by accident in the next commit, not by the sweep.

A follow-up `git grep` across the whole tree revealed **16 more tracked files** outside `content/entries/` with similar patterns:

- `Cargo.toml`
- `crates/trawl/src/main.rs`
- `docs/HANDOFF.md`
- `docs/materials/claude-session-logs.zip` (binary, filename match)
- `docs/plans/2026-03-19-001-feat-the-daily-claude-v1-pipeline-plan.md`
- `docs/plans/2026-04-04-001-fix-pr1-review-findings-plan.md`
- `docs/solutions/process-patterns/bot-pr-review-cadence-and-synthesis-20260406.md`
- `docs/solutions/process-patterns/bot-review-loop-gh-api-mechanics-20260408.md`
- `docs/solutions/process-patterns/do-less-ritual-more-work-20260408.md`
- `docs/solutions/process-patterns/review-loop-discipline-20260408.md`
- `etl/migrate_compilation.py`
- `scripts/mini-server.sh`
- `todos/001-trawl-refinements.md`
- `todos/018-trawl-incremental-state.md`
- `todos/019-anonymization-hardening.md`
- `todos/022-trawl-zfc-redesign.md`

## All of it ships

The entire repo is open-source — `content/`, `docs/`, `todos/`, `scripts/`, `etl/`, `crates/`, `CLAUDE.md`, `HANDOFF.md`, and the workspace-root `Cargo.toml`. There is no *"dev-machine-only"* surface where leaks are acceptable. Every tracked file ships publicly and needs the same PII hygiene as content entries.

An earlier draft of this doc made a wrong distinction (*"only `crates/trawl/` is public, everything else is private"*) based on the mental model that only the publishable Rust crate ships. That was wrong: the whole repo ships. Documentation, todos, plans, solution docs, shell scripts, and the migration ETL are all publicly visible artifacts of the project.

## The follow-up cleanup

After this doc was written, a second sweep pass cleaned the remaining files:

**`crates/trawl/src/main.rs`** — two leaks in doc comments and tests:

1. A comment in `derive_project_name` showing an example path: `.../projects/-Users-alice-Projects-org-repo/<uuid>.jsonl` → `.../projects/-Users-<user>-Projects-<org>-<repo>/<uuid>.jsonl`.
2. A test input in `derive_project_name_takes_last_two_segments`: the old path embedded the real username and `Banade-a-Bonnot`; scrubbed to `-Users-alice-Projects-example-the-daily-claude`. The test assertion still passes because `derive_project_name` keys off the last 2 dash-segments only (`daily-claude`).

**`etl/migrate_compilation.py`** — a hardcoded `/Users/alice/` in a scrub regex. Generalized to `/Users/[^/]+/` so the script is reusable and no longer leaks.

**`scripts/mini-server.sh`** — `PROJECT_DIR="/Users/alice/Projects/..."` replaced with a portable `$(cd "$(dirname "$0")/.." && pwd)` computation. Script now works regardless of where the repo is cloned.

**`docs/HANDOFF.md`** — pattern descriptions in the 2026-04-09 session notes that used literal `` `/Users/alice` `` inside backticks as documentation of the sweep targets. Replaced with placeholder pattern descriptions so a whole-repo grep comes back clean.

**`docs/plans/*.md`, `docs/solutions/process-patterns/*.md`, `todos/*.md`** — scrubbed by a background subagent sweep across the same leak-pattern set (home paths, org names, GitHub handles, committer emails).

**Cargo.toml** — `repository = "https://github.com/Bande-a-Bonnot/the-daily-claude"` intentionally preserved. That's the actual public GitHub URL and the `repository` field must point at it for the crate to link back to its source.

## Noteworthy side-finding

`derive_project_name` **already exists** in `crates/trawl/src/main.rs` at line 648. The code-side function that todo `#034-extractor-deterministic-path-handling.md` needs is mostly already built — it just needs to be wired into the extractor flow so the injected frontmatter value comes from this function instead of from the LLM response. Significantly shrinks the scope of #034.

## Why the original sweep was incomplete

Two root causes:

1. **Directory-scoped mental model.** The triage report that drove the sweep was titled `entries-200-triage` and the subagent prompt said *"search across `content/entries/*.md`."* Scope was set by the target directory, not by the leak class. Leaks outside that directory were invisible to the sweep.
2. **Confirmation bias from "all target patterns at 0."** When the sweep reported *"zero residual matches for each pattern,"* that was true **for the files it searched.** The success signal masked the incomplete scope.

Both root causes can be diagnosed by a single question: *"What would a whole-repo `git grep` for this pattern return?"* If nobody asked, the sweep isn't done.

## How to fix it next time

1. **Scope by leak class, not by directory.** If the class is *"home paths anywhere in the tree,"* the search is `git grep -l '/Users/' -- ':!*.zip' ':!*.png'` — all tracked text files.
2. **Always run a final verification grep across the whole repo** after the targeted sweep, with the leak patterns applied globally. If it finds anything, your scope was wrong and the sweep isn't done.
3. **Distinguish published PII from operational paths.** `content/entries/*.md` becomes public (memes on social media). `scripts/mini-server.sh` and `Cargo.toml` are internal developer files — an absolute path there is sloppy but not a public PII leak. Different threat models, different urgency, but both worth cleaning.
4. **The sweep isn't done until the whole-repo grep comes back empty** (or every remaining match is deliberately justified in-place).

## Practical checklist

For any anonymization sweep going forward:

- [ ] Define the leak class as a set of regex patterns (not file globs)
- [ ] Run the scoped sweep in its target directory
- [ ] **Run a whole-repo verification grep** via `git grep` with path negation if needed
- [ ] For any hits outside the target: triage each (scrub, justify with a comment, or schedule as a separate todo)
- [ ] Only then declare the sweep done

## References

- Commit `2b79256` — the incomplete sweep (content/entries/ only)
- Commit `be3605b` — the accidental CLAUDE.md catch + the rule added to Working Style
- `docs/solutions/design-decisions/llm-cognition-code-transformation-20260409.md` — companion rule from the same cleanup session
