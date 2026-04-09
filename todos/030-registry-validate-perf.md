---
title: "Registry::find_leaks rehashes every substring window per byte offset"
priority: low
status: resolved
source: gemini-code-assist
depends_on: 022
resolved_at: 2026-04-08
resolved_by: length-aware-substring-registry + streaming-sha256 (PR #3)
---

## Resolution (2026-04-08)

Resolved by the length-aware registry redesign in PR #3. `Registry`
now carries a `BTreeSet<usize> lengths` field (`crates/trawl/src/registry.rs:58`)
populated at grow time. `find_leaks` iterates only those lengths instead
of `min_len..=max_len`, so a corpus with three distinct literal lengths
runs three passes, not 89. Combined with the zero-alloc streaming SHA-256
from `state::sha256_bytes`, per-body cost dropped ~15× in practice.

Tests added:
- `find_leaks_only_iterates_tracked_lengths` — proves scan length
  restriction
- `legacy_registry_fallback` — covers the legacy `.pii-registry.json`
  path that predates length tracking

See compound learnings:
- `docs/solutions/best-practices/length-aware-substring-registry-20260408.md`
- `docs/solutions/best-practices/streaming-sha256-zero-alloc-hot-loop-20260408.md`

# Registry validation rehashes substrings redundantly

## Finding

`crates/trawl/src/registry.rs:~120` (`Registry::find_leaks`) does a
nested loop: for each window length `w` in `min_len..=max_len`, walk
every byte offset `i` in `0..=n-w`, SHA-256 the slice, check
membership. That means each byte position gets hashed `max_len -
min_len + 1` times across different window lengths, and each hash
call re-reads up to 96 bytes.

For a single draft body of ~5 KB with the current defaults
(`min_len=8`, `max_len=96`), that is roughly `5000 * 89 ≈ 445,000`
SHA-256 calls per body. Per-body wall time is fine (a few hundred
ms at most), but for `trawl validate` walking all 256 existing
entries the total cost is minutes-scale.

Gemini flagged this as a medium-priority perf concern on PR #3
(inline comment id 3051944016).

## Why this is P3 (not P2)

- Current bodies are 500 B - 8 KB; pathological long entries do not
  exist in the corpus.
- `trawl validate` is a rare operation (once before publish, maybe
  weekly). A multi-minute run is tolerable.
- The hash set lookup is O(1); there is no algorithmic cliff as the
  registry grows, only linear growth in per-entry cost.
- Correctness is not affected.

## Proposed fix (when it becomes a real bottleneck)

**Do NOT** swap to Rabin-Karp or a rolling hash. That adds cryptographic
complexity (salt choice, collision handling) without a meaningful
speedup at this scale.

Instead, the cheap wins in order:

1. **Tighten `max_len`.** Most flagged literals are < 64 chars. Drop
   the default `max_len` from 96 to 64. That is a ~30% cost cut for
   zero algorithmic work.
2. **Single-pass hash reuse.** Record each literal's length at grow
   time in a `BTreeSet<usize>`. At validate time, only scan window
   sizes that actually exist in the registry. If the registry only
   ever saw lengths {16, 24, 40}, we run 3 passes, not 89.
3. **Exact-length lookup.** Store literals keyed by length:
   `BTreeMap<usize, HashSet<HashDigest>>`. Validate by walking the
   body once per known length. This is still a full rescan but
   avoids the outer `w in min..=max` loop entirely.

Any of these is O(n) per body with a dramatically smaller constant
than today. None changes the deterministic-safety-net contract.

## Scope

- Only `crates/trawl/src/registry.rs`
- The `trawl validate` subcommand automatically benefits once the
  core helper is faster — no CLI changes needed.

## Acceptance

- `cargo test -p trawl` green before and after.
- `trawl validate content/entries/` on the full corpus runs in < 10 s
  with a registry of a few hundred literals.
- The synthetic Doppler-leak test in `registry::tests` still passes.
