---
title: "Trawl quality improvements — post-audit"
priority: high
status: pending
---

# Trawl Quality Improvements

From the audit of entries 138-265: 128 extracted, only 18 (14%) were KEEP-worthy.
86% was noise, duplicates, or lacking context.

## Priority 1: Post-Extraction Deduplication
- [ ] Group extractions by session + overlapping message ranges
- [ ] Pick the highest-scoring window from each cluster
- [ ] Eliminate entries with >60% message range overlap
- [ ] Same-title detection across entries

## Priority 2: Minimum Content Threshold
- [ ] Reject extractions where body is entirely tool calls (`[tool: Bash]`, `[tool: Edit]`)
- [ ] Require at least 100 chars of actual human/assistant text per entry
- [ ] Skip windows that are >80% tool results

## Priority 3: Recalibrate Scoring
- [ ] Reduce relatability weight — relatable but not remarkable should score low
- [ ] Increase quotability + humor weight — best predictors of "stop scrolling"
- [ ] Add novelty penalty — Nth instance of same pattern scores near 0
  (e.g., 4th "Let me wait longer" = score 0)
- [ ] Examples in the scoring prompt: show what a 0.9 vs 0.5 looks like
- [ ] Raise extraction threshold to 0.85 minimum

## Priority 4: Operational Chatter Filter
- [ ] Detect and penalize: "Let me wait," "Let me check," "Let me verify,"
  "CI passed," "Deploy is running," "Still building"
- [ ] These are operational, not content. Apply heavy score penalty or filter.

## Priority 5: Narrative Arc Detection + Multi-Window Stories
- [ ] After scoring individual windows, look forward N windows for resolution/escalation
- [ ] If a pair or chain of windows tells a better story together, merge them
- [ ] Bridge the gap with a ZFC call:
  - Editorial ellipsis: `[...]`
  - Narrative bridge: `*200 rounds of deletion later...*`
  - SpongoBob-style: `*3 hours later...*`
  - Or a precise count: `*47 PRs, 3 deploys, and one existential crisis later...*`
- [ ] Bridge prompt: "Given these two exchanges from the same session, write a brief
  bridge that connects them. Keep it short — one line."
- [ ] Detect common arc patterns:
  - Setup → Escalation → Punchline
  - Human instruction → Claude response → Ironic outcome
  - Declaration of completion → Immediate failure
  - Confident fix → Same error again
  - "This time it will work" → It didn't
- [ ] The bridge is the ONLY editorial content allowed — everything else is verbatim

## Priority 6: Session Fatigue Penalty
- [ ] In long sessions (500+ messages), apply increasing score thresholds
- [ ] Message 500 needs to be MORE remarkable than message 50
- [ ] Marathon debugging sessions are mostly operational chatter

## Audit Results Reference

| Category | Count | Percentage |
|----------|-------|------------|
| KEEP     | 18    | 14%        |
| MAYBE    | 30    | 23%        |
| DUPLICATE| 22    | 17%        |
| CUT      | 58    | 45%        |

KEEP entries: 138, 139, 176, 178, 186, 192, 198, 199, 219, 237, 240, 245, 247, 250, 253, 256, 257, 262
