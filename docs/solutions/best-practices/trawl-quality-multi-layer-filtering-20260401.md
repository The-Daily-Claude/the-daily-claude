---
title: "Trawl Quality Architecture — Multi-Layer Filtering for Session Mining"
date: 2026-04-01
category: best-practices
module: trawl
problem_type: best_practice
component: tooling
severity: high
applies_when:
  - "Trawl keep rate drops below 30%"
  - "Haiku token cost is high relative to extraction yield"
  - "Near-duplicate entries appear from overlapping sliding windows"
  - "Editorial review is mostly discarding noise"
tags: [trawl, quality, filtering, scoring, dedup, sliding-window, pipeline, haiku, token-cost]
---

# Trawl Quality Architecture — Multi-Layer Filtering for Session Mining

## Context

The Trawl session miner uses a sliding window approach to extract "remarkable moments" from Claude Code session JSONL files. Each window is sent to Haiku for 8-dimension scoring. The original implementation scored every window indiscriminately, resulting in a 14% keep rate (18/128 entries worth keeping) — 86% of editorial review work was discarding noise.

## Guidance

Implement multi-layer quality filtering as a pipeline where each layer is cheaper than the next:

**Layer 1 — Pre-scoring filter** (`window_is_worth_scoring` in `main.rs`): Reject windows before spending any LLM tokens. Deterministic, zero-cost, catches the bulk of noise:
- Requires both human and assistant turns
- Minimum 2 substantive turns with >20 chars
- Minimum 100 chars total conversation text
- Rejects windows >80% tool calls
- Rejects windows >50% operational chatter patterns ("Let me wait", "CI passed", "Still building", etc.)

**Layer 2 — Scoring prompt calibration** (`score.rs`): "BE HARSH" instruction, calibration examples showing 0.9 vs 0.5 vs near-zero, explicit warning that "relatable alone is NOT enough." Threshold raised from 0.6 to 0.85.

**Layer 3 — Overlap dedup** (`dedup_overlapping_windows` in `main.rs`): After scoring all windows in a session, cluster by >60% message range overlap (overlap length / shorter window length). Greedy-pick highest-scoring from each cluster.

**Layer 4 — Dynamic per-session cap**: `ceil(turns / 1000)` clamped to [3, 10]. Applied AFTER scoring and dedup so all candidates compete fairly.

## Why This Matters

- **Token cost**: Pre-filtering cuts Haiku calls by 60-80% per session.
- **Editorial burden**: A 14% keep rate means the reviewer spends most time saying "no."
- **Content quality**: Near-duplicate entries from overlapping windows dilute the corpus.
- **Composability**: Each layer addresses a different failure mode. Removing any layer degrades a different quality dimension.

**Relationship to Zero Framework Cognition**: Layer 1 is a deliberate exception to ZFC — operational chatter patterns and tool-call ratios are structural signals, not cognitive judgments (same category as credential regex). Layer 2 IS pure ZFC — the model scores, we just give it better calibration.

## When to Apply

- Any LLM-scored extraction pipeline where >60% of candidates can be rejected with cheap deterministic checks
- Sliding window approaches specifically — overlap dedup is mandatory, not optional
- When keep rate drops below 30%

## Examples

**Pre-filter rejecting tool-call window**: 7 of 8 turns are `[tool_use: Read]` / `[tool_result: ...]` → rejected by >80% tool calls check.

**Overlap dedup**: Windows [turns 12-19] and [turns 14-21] have overlap [14-19] = 6 turns, ratio = 6/8 = 0.75 > 0.6 threshold → only the higher-scoring one survives.

**Per-session cap**: A 3000-turn session → `ceil(3000/1000) = 3` entries max. A 500-turn session → `ceil(500/1000) = 1`, clamped to minimum 3.

## Related

- `docs/solutions/design-decisions/zero-framework-cognition-20260320.md` — Layer 1 is a justified exception
- `docs/solutions/process-patterns/simplicity-over-architecture-20260320.md` — validates the approach: each layer is simple (pre-filter 30 lines, dedup 20 lines, cap 2 lines)
- `todos/010-trawl-quality-improvements.md` — original audit that identified the 14% keep rate
