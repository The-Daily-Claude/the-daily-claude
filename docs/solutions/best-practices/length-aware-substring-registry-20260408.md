---
title: Length-Aware Substring Registry — The Data Tells You How to Scan It
category: technical
date: 2026-04-08
tags: [rust, registry, performance, correctness, sliding-window]
related_commits: [f57348f]
---

# Length-Aware Substring Registry — The Data Tells You How to Scan It

## Problem

Trawl's PII registry stores SHA-256 digests of literal credentials and
names that the tokeniser has flagged across every run. To validate a
new draft, it needs to find any registry literal that appears verbatim
inside the body. The naive contract is "scan every possible substring
of `body` and check membership."

The first version hardcoded a window range — something like
`for window in 8..=96 { ... }`. This had two compounding problems:

1. **Correctness hole.** A 120-byte credential added to the registry
   could not be found by a scan that capped at 96. The hash set
   contained it, but the scan never asked the right question.
2. **Wasted work.** Even when the correct length lived inside the
   range, the loop still hashed 89 other window sizes that the
   registry knew nothing about. For a 4 KB body that's ~350K wasted
   SHA-256 calls per validation.

## What we learned

The registry already knows everything the scan needs. **Store a
`BTreeSet<usize>` of every literal length you've ever ingested
alongside the hash set.** Then `find_leaks` iterates only those exact
window sizes — no more, no fewer.

The result fixes both problems at once:

- **Correctness.** A 120-byte literal is matchable because the
  registry recorded the length 120 when it ingested it. There is no
  ceiling.
- **Performance.** A registry with three distinct literal lengths
  scans three windows, not 89. For a real-world Trawl run with ~13
  literals across 4-6 distinct lengths, the validation pass goes from
  O(body_len × 89) to O(body_len × 6) — about 15x fewer hash calls,
  and the speedup grows with body size.

The slogan: **the data tells you how to scan it.** When you find
yourself writing a hardcoded range over your data, ask whether the
data could just enumerate the values you actually need.

A legacy fallback handles the rollover gracefully: if you load an
older registry file whose `lengths` set is empty, fall back to a wide
static range so existing data still detects leaks. New growth
populates `lengths`, and the fallback evaporates on the next save.

## How to apply

- Anywhere you have a hash set of "things to look for" inside a larger
  blob, also record the **shape** of those things (length, prefix,
  suffix, character class) and use it to constrain the scan.
- Don't pick a window range based on what you think the data could
  contain. Pick it based on what the data **does** contain. Track
  evidence as you grow the structure.
- Provide a legacy/empty fallback so old serialised state stays valid
  through the upgrade — but design so the fallback decays naturally as
  new data flows in.

## Code pointer

- `crates/trawl/src/registry.rs:35-43` — `MIN_LITERAL_LEN`,
  `LEGACY_MIN_LEN`, `LEGACY_MAX_LEN` constants
- `crates/trawl/src/registry.rs:104-118` — `Registry::grow` records
  both hash and length
- `crates/trawl/src/registry.rs:140-188` — `find_leaks` iterates only
  the tracked lengths, with the legacy-range fallback for empty sets
- `crates/trawl/src/registry.rs:264-` — `find_leaks_only_iterates_tracked_lengths`
  test asserting the perf bound

## Related

- `docs/solutions/design-decisions/zfc-anonymization-20260406.md` — the
  registry is the deterministic safety net beneath the ZFC tokeniser
