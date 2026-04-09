---
title: "Extractor: compute `project:` in Rust, don't let the LLM slugify the path"
priority: critical
status: pending
---

# Stop asking the LLM to slugify a path into the `project:` field

## The bug

The ZFC trawl extractor asks the LLM to fill in the `project:` frontmatter field. With no clean source value, the LLM defaults to slugifying the session jsonl file path, producing `project: Users-alice` — leaking the user's home directory into entry frontmatter.

Found across ~80+ entries in the #211–#310 range from the ZFC batch run. Fixed retroactively by the 2026-04-08 sweep; this todo prevents the next trawl run from reintroducing it.

## Why this is wrong

Slugifying a path is deterministic code work. The LLM has no business doing it. The cost of asking it anyway: non-determinism, wasted tokens, and a hallucination risk that turned into a real PII leak.

## The fix

Minimal. Two steps in `crates/trawl/src/extractor.rs` (plus wherever the response is parsed):

1. **Remove `project:` from the LLM's response schema.** The prompt should no longer ask for it. The structured-output contract should no longer include it.
2. **Compute `project:` in Rust.** Trawl already knows the session file path — it's the input. Derive the project name from the parent-dir slug in the Claude session tree (e.g. `~/.claude/projects/-Users-<user>-Projects-<org>-<project>/<uuid>.jsonl` → `<project>`, applying whatever canonical decoding rule we pick). Inject the computed value into the frontmatter after the LLM call returns.

If the canonical decoding rule is non-obvious for a given path, `project: unknown` is a fine fallback. What matters is that the value is code-generated, not LLM-generated.

### Good news: `derive_project_name` already exists

The function is already written at `crates/trawl/src/main.rs:648` with a passing unit test (`derive_project_name_takes_last_two_segments`). It takes the session path, extracts the parent directory's last 2 dash-segments, and joins them with a hyphen — exactly the canonical decoding rule we need. The current comment says *"This is a heuristic, not security-sensitive — the tokeniser will scrub anything that needs scrubbing"* — that framing is **wrong** under the new architecture. This function is the SOURCE OF TRUTH for the `project:` field, not a fallback heuristic. Update the comment when wiring it in.

Remaining work:

- Remove `project:` from the LLM response schema / prompt in `extractor.rs`
- After the LLM call returns, inject `derive_project_name(session_path)` into the frontmatter the extractor writes
- Update the `derive_project_name` comment to reflect that it is now the authoritative source, not a heuristic fallback
- Verify via smoke test (re-run trawl on `10c05bb2`, `aab2e849`, `3c76563c`) that no new entry has `project: Users-*` or a home-path slug

## Smoke test

Re-run trawl on `10c05bb2`, `aab2e849`, `3c76563c` and confirm no entry frontmatter contains `project: Users-*` or any home-path slug. The field should be either a clean project name or `unknown`.

## Out of scope

- Any other trawl architecture change. Claude still reads the session file normally via the Read tool with `--add-dir`. The tokeniser still handles natural-language PII. The only change is that the single `project:` field moves from LLM output to Rust-computed injection.
