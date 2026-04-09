---
title: Handle PR #3 review feedback loop
type: fix
status: active
date: 2026-04-08
---

# Handle PR #3 Review Feedback Loop

## Overview

React to automated reviews (CodeRabbit, Gemini, Codex) on PR #3
iteratively until all P1/P2 concerns are addressed, every comment has
a reply, CI + deployments are green, and two consecutive review
rounds come back clean.

## Problem Frame

PR #3 is the ZFC refactor — 8 commits ripping ~1000 lines of framework
cognition and shipping the Sonnet+Haiku+state+registry pipeline. Bots
will review it, find real issues in a security-sensitive area, and we
need to respond quickly and correctly without letting the queue balloon.

## Requirements Trace

- R1. Every P1 finding gets fixed inline.
- R2. Every P2 finding gets fixed inline or filed as a todo with clear rationale.
- R3. Every P3/nitpick finding gets filed as a todo.
- R4. Every review comment gets a reply explaining the action taken or the rationale for deferral.
- R5. CI (CodeRabbit check, any deploy workflows) is green before the loop advances.
- R6. Two clean review rounds (new @codex, @gemini-code-assist requests) before declaring done.

## Scope Boundaries

- Out of scope: new features, scope creep beyond review feedback, merging to main.
- Deferred feedback belongs in `todos/` (pending status), not in the code.
- Content entries, slide PNGs, HANDOFF.md are untouched.

## Workflow

1. Fetch all open review threads via `gh api repos/.../pulls/3/comments` + `gh pr view`.
2. Triage each finding into P1/P2/P3/nitpick using the same rubric as the CodeRabbit round.
3. Fix P1/P2 inline in one commit per logical unit. Cargo check + test after each.
4. File P3/nitpicks as todos under `todos/NNN-*.md`.
5. Reply to every thread with `gh api ... pulls/comments/{id}/replies` or `gh pr comment` — include rationale and commit SHA when a fix was applied.
6. Push; wait for CI to come back green.
7. Re-request reviews from @codex and @gemini-code-assist.
8. Loop until two consecutive rounds are clean.

## Implementation Units

- [x] **Unit 1: First triage round on current bot feedback**
  - Current feedback: 3 Gemini medium-priority inline comments (race, non-atomic write, rehashing)
  - CodeRabbit walkthrough body, no nitpicks inlined yet
  - Fix: atomic write via tempfile+rename; defer race (existing todo 027); file rehashing as todo
  - Reply to all 3 Gemini threads + CodeRabbit walkthrough

- [ ] **Unit 2: Push + wait for CI**

- [ ] **Unit 3: Re-request reviews**

- [ ] **Unit 4: Second round triage (if findings remain)**

## Verification

- All review threads have a reply from the author
- CI green
- No P1/P2 findings without a fix or a tracked todo
- Two consecutive clean review rounds
