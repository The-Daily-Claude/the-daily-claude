---
title: "Trawl overlap dedup is too loose — overlapping windows ship as separate entries"
priority: superseded
status: superseded
superseded_by: 022
---

> **Superseded 2026-04-07 by `todos/022-trawl-zfc-redesign.md`.** No more
> sliding windows means no more overlap. Sonnet identifies one beat as
> one entry. Dedup becomes a property of the model's judgment, not a
> Rust heuristic.

# Trawl Overlap Dedup Too Loose

## Bug

Trawl's post-extraction dedup pass is supposed to collapse overlapping windows
from the same session into a single entry. It doesn't do it tightly enough.

Live example from the 2026-04-06 run:

| Entry | Session | Range | Overlap |
|---|---|---|---|
| #231 *All Clean Now* | `c6f8bd76…82` | messages 525–532 | — |
| #232 *Still Running* | `c6f8bd76…82` | messages 529–536 | **4 messages of overlap (50%)** |

Same session, same scene, same joke. #231 captures the build-up (Claude
declaring victory three times in a row while task notifications keep
contradicting it). #232 captures the kicker (`See? You still had agents
running :)`). They're two halves of one story, extracted as two entries.

The right outcome is **one** entry spanning 525–536, scored higher than
either half alone, with the build-up *and* the kicker.

## Root cause

The current dedup logic (per `todos/010-trawl-quality-improvements.md`
priority 1) is described as "group extractions by session + overlapping
message ranges, pick the highest-scoring window from each cluster, eliminate
entries with >60% message range overlap."

Empirically that's either:
- Not implemented yet, or
- Implemented but the threshold is too high, or
- Implemented but the cluster-merge step picks one window instead of merging
  the ranges

Either way, 50% overlap on a 7–8 message window is well above any reasonable
"these are the same scene" threshold and should collapse.

## Fix

Two changes, both required:

### 1. Lower the overlap threshold and make it count-based, not percentage-based

Percentage thresholds break on small windows. With an 8-message window, a 50%
overlap is 4 messages — and 4 messages of an 8-message conversation is
*definitely* the same scene. Use **absolute message count** instead:

> Two windows from the same session are duplicates if their message ranges
> overlap by **3 or more messages**, regardless of window size.

3 messages is roughly one full exchange (human + assistant + tool result),
which is the smallest unit where "same scene" is unambiguous.

### 2. Merge ranges instead of picking one

When the dedup pass detects an overlap, the right move is **range union**, not
"keep the higher-scoring one and drop the other":

```
#231: 525..=532 (score X)
#232: 529..=536 (score Y)
                ↓
merged: 525..=536, rescored on the wider window
```

Scoring the merged range gives Haiku the full arc to evaluate and produces
a single entry that contains both the build-up and the kicker. Picking one
window discards content the other window had.

If rescoring the merged range is too expensive (it's another Haiku call), the
fallback is to keep the higher-scoring original range *with the body of the
union* — i.e. score = max(X, Y), body = union(messages). Cheap, lossy on the
score but lossless on the content.

## Implementation

- [ ] Audit `crates/trawl/src/` for the current dedup pass — confirm whether
      it exists, where it lives, and what threshold it uses
- [ ] Switch to count-based overlap detection (≥3 message overlap = duplicate)
- [ ] Implement range union for overlapping clusters
- [ ] Decide: rescore merged ranges (clean but +1 Haiku call per merge) or
      max-score with union body (cheap but slightly lossy on the score)
- [ ] Test fixture: a session with three windows whose ranges are
      [10..=17], [14..=21], [18..=25] — assert the dedup pass produces one
      entry spanning [10..=25], not three separate entries
- [ ] Backfill across existing 232 entries: detect overlap clusters, log them
      for manual review, don't auto-merge bodies on existing entries (the
      author may have edited them)

## Why this matters

Beyond the obvious quality win: overlap dedup affects the **batch cold-start
budget** in `todos/017`. If 10% of windows are near-duplicates of their
neighbours and we're not collapsing them at the *window* level (before
scoring), we're paying for redundant Haiku calls in every batch. Tightening
dedup is also a perf optimization.

## Acceptance

- [ ] Test fixture above passes
- [ ] Re-running Trawl on the session that produced #231 + #232 yields
      exactly **one** entry spanning the union of their ranges
- [ ] Backfill report identifies #231/#232 as a known duplicate pair (and
      surfaces any others lurking in the existing 232 entries)
- [ ] No regression: non-overlapping windows from the same session still
      extract independently
