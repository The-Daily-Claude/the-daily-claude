---
title: Balanced-Bracket JSON Candidate Scan — Trust the Deserialiser, Not Heuristics
category: technical
date: 2026-04-08
tags: [rust, json, parsing, llm-output, robustness]
related_commits: [f57348f]
---

# Balanced-Bracket JSON Candidate Scan — Trust the Deserialiser, Not Heuristics

## Problem

Sonnet and Haiku return JSON with a side of prose: a markdown fence,
a "Here you go:" preamble, an editorial summary, sometimes an
inline `[3]` count in the narration. The pipeline needs to fish the
real JSON value out of that envelope, whatever Claude wrapped it in.

The first attempt was the obvious one:

```rust
let start = raw.find('[').context("no opening bracket")?;
let end   = raw.rfind(']').context("no closing bracket")?;
serde_json::from_str(&raw[start..=end])
```

This breaks immediately on `"I found [3] moments:\n[ ... ]"` because
`find('[')` lands on the prose `[`, and the rest of the slice is
nonsense JSON. The "fix" you reach for next is worse than the bug:

> Match `[\n` or `[{` instead of bare `[`.

This **breaks on the success case.** The real array often has a
nested object like `{"quote": "[OK] starting", ...}` whose `[OK]`
matches the heuristic perfectly, and the slice you cut is now
truncated mid-string and unparseable. Every "smarter than `find('[')`"
heuristic has the same shape: it works on the corpus you tested and
fails on the next character variation Sonnet tries.

## What we learned

Stop trying to identify the right opener. **Iterate every candidate
opener and let `serde_json::from_str` be the validator.**

The algorithm:

1. Scan forward from `search_from = 0` for the next `[` (or `{` for
   objects).
2. Walk a balanced-bracket scanner from that position that tracks
   `depth: i32`, `in_string: bool`, and `escape: bool`. Increment
   depth on the open byte, decrement on the close byte, but **only
   when not inside a string literal** — and the string scanner has to
   honour `\"` escapes. Return the byte index when depth hits zero.
3. Slice `start..=end`. Try `serde_json::from_str::<TargetType>` on
   it.
4. If it parses, return. If it doesn't, set `search_from = start + 1`
   and loop. The naive `[3]` slice gets rejected by serde because
   `Vec<DraftEntry>` can't contain a bare integer; the scan keeps
   walking until it hits the real array.

The serialiser is the ground truth of "is this the JSON I want." Any
heuristic you write is a worse version of the parser you already
linked against.

This pattern is symmetric for objects: the tokeniser uses the same
`find_matching_bracket(s, start, b'{', b'}')` helper to scan candidate
`{` openers. Both stages share one balanced-bracket walker so they
speak the same dialect of "where does this candidate end."

## How to apply

1. **Walk every candidate, parse to validate.** Never single-shot
   `find` + `rfind` over LLM output that may carry prose.
2. **Track string state in the bracket walker.** A scanner that
   counts `[` and `]` without honouring `"..."` and `\"` is a footgun
   waiting to fire on the next nested string with brackets in it.
3. **Keep the last parse error structurally** (byte range + length,
   no slice) so failure messages help you debug without leaking
   model output. See the companion lesson on PII-safe error chains.
4. **Reuse one balanced-bracket helper across every parser** so
   bugs in string handling get fixed in one place.

## Code pointer

- `crates/trawl/src/extractor.rs:116-160` — `parse_draft_array` walks
  every candidate `[` and lets serde reject prose hits
- `crates/trawl/src/extractor.rs:163-212` — `find_matching_bracket`
  with string-literal awareness, shared across stages
- `crates/trawl/src/tokeniser.rs:137-186` — `parse_tokenised` reuses
  the same helper for `{` / `}` candidates
- `crates/trawl/src/extractor.rs:220-` —
  `parser_skips_bracket_in_prose_preamble` test pinning the regression
