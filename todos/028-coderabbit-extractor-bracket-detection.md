---
title: "Extractor JSON start-detection is fragile with bracketed prose"
priority: low
status: resolved
source: coderabbit
depends_on: 022
resolved_at: 2026-04-08
resolved_by: balanced-bracket-json-candidate-scan (PR #3)
---

## Resolution (2026-04-08)

Resolved by the balanced-bracket candidate scan that landed in PR #3
(`crates/trawl/src/extractor.rs:116-155`). `parse_draft_array` now walks
every `[` in the model output, calls `find_matching_bracket` to get the
matching `]`, tries to deserialise the slice as `Vec<DraftEntry>`, and
keeps scanning if parsing fails. A preamble like `"Here is the [requested] data:"`
locks onto `[requested]`, serde rejects it (wrong shape), and the loop
continues until it hits the real JSON array. Error context logs byte
positions and candidate length only, never the raw slice.

See `docs/solutions/best-practices/balanced-bracket-json-candidate-scan-20260408.md`
for the full writeup.

# Extractor JSON start-detection is fragile with bracketed prose

## Finding

`extractor.rs` locates the beginning of the JSON array returned by
Sonnet by calling `trimmed.find('[')`. If the model emits any prose
preamble containing square brackets (e.g. "Here is the [requested]
data:"), the scanner locks onto the wrong position and the later
`serde_json` parse fails. The retry logic with a "no prose" follow-up
prompt mitigates this, so the bug is latent rather than blocking.

## Location

`crates/trawl/src/extractor.rs:113-118`

## Proposed fix

Prefer more structural openings before falling back to a plain `[`:

```rust
let start = trimmed
    .find("[\n")
    .or_else(|| trimmed.find("[{"))
    .or_else(|| trimmed.find("[ "))
    .or_else(|| trimmed.find('['))
    .ok_or_else(|| anyhow!("no opening bracket in extractor output"))?;
```

Apply the symmetric treatment to `rfind(']')` (prefer `\n]`, `}]`).
Add a regression test that feeds the extractor a preamble containing
`[foo]` and asserts it still finds the real JSON array.

## Severity

P3 — edge case that the existing retry/no-prose fallback already
covers in practice. Worth fixing because the extractor is the hottest
path and any unexpected Sonnet phrasing can burn a retry.
