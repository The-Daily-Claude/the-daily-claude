---
title: "Extractor: compute `project:` in Rust, don't let the LLM slugify the path"
priority: critical
status: complete
---

# Compute `project:` in Rust, not via extractor output

## What landed

This fix is already in the codebase.

The extractor prompt no longer asks the model for a `project:` field. Its structured output is now only:

- `title`
- `category`
- `tags`
- `quote`
- `why`

When `trawl` writes an entry, it derives `project` from the session path in Rust via `derive_project_name(session_path)` and injects that value into the entry frontmatter at write time.

## Why this mattered

The old extractor flow let the model invent the `project:` field from the session file path. In practice that produced values like `Users-alice`, which leaked home-path context into entry frontmatter.

That was the wrong boundary. Slugifying a path is deterministic code work, not LLM cognition.

## What changed

1. `project:` was removed from the extractor contract.
2. `derive_project_name(session_path)` became the write-time source of truth for the frontmatter field.
3. The entry writer now sets `project` from code, not model output.

## Current evidence

Grounding for the completion claim:

- `crates/trawl/prompts/extractor.md` no longer includes `project` in the output schema.
- `crates/trawl/src/main.rs` sets `let project = derive_project_name(session_path);` before constructing the `Entry`.
- The written `Entry` uses that Rust-derived value for `project`.

## Residual caveat

One wording mismatch remains in code comments: `derive_project_name` is still described as a heuristic and "not security-sensitive". That comment is stale relative to the architecture now that the function is the source of truth for the `project` field.

## Verification status

Implementation: complete.

Fresh smoke rerun: not re-executed during the 2026-04-09 capability audit session. We did not send private session content back through the model just to re-prove this fix. The completion here is based on source inspection of the shipped implementation, not a new extractor run over `10c05bb2`, `aab2e849`, and `3c76563c`.

## Out of scope

- Any broader extractor redesign
- Any tokeniser change
- Any fresh end-to-end smoke run over private sessions
