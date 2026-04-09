---
title: Do less ritual, more work — autonomous loops are wrong for destructive PII refactors
category: process
date: 2026-04-08
tags: [workflow, autonomous-loops, slfg, ralph-loop, escalation, user-override]
related_commits:
  - a1e8267  # refactor(trawl): rip framework cognition, ship ZFC pipeline
  - f57348f  # PR #3 squash merge
---

## The situation

PR #3 (the Trawl ZFC refactor) started with the wrong workflow. The user
invoked `/compound-engineering:slfg` — a swarm-enabled "let's fucking go"
orchestrator that wraps `ralph-loop`, `ce:plan`, red/green adversarial
team prompts, and a background orchestrator dispatch. The intent was
reasonable: the refactor was large, security-critical, and had an
existing plan file (todo 022). Surely the big ritual was appropriate.

It wasn't. Within the first few assistant turns, the session produced:

- A new `docs/plans/2026-04-07-001-trawl-zfc-adversarial-plan.md` plan
  file adapting [project-delta] red/green phases to todo 022.
- Meta-commentary about "dispatching the orchestrator in background" and
  "delegating bulk work to Gemini."
- Exactly zero commits.

The user escalated, verbatim:

> **STOP APOLOGIZING. AND FUCKING. GET. MOVING!**

After the override, the actual work took ~2 hours end-to-end: one big
`refactor(trawl): rip framework cognition, ship ZFC pipeline` commit
(`a1e8267`), smoke tests on three real sessions, and the five bot-review
rounds that followed. The ritual phase produced no useful artifact that
survived into the merge.

## What we learned

### Autonomous loops are the wrong tool for destructive PII refactors

Ralph-loop and SLFG are good for **generative** work where the cost of
each iteration is low and the completion signal is objective: "finish all
slash commands," "pass every test," "ship a feature video." The loop is a
search over a space where wrong answers are cheap and the reward
function is the `<promise>DONE</promise>` output.

A destructive PII refactor is none of those things:

- **The cost of each iteration is high**. You are deleting working code
  (regex anonymizer, sliding windows, 8-dim scoring). Every loop
  iteration that "redoes" the delete is either a merge conflict or a
  data-loss risk.
- **The completion signal is subjective**. "Did we anonymize enough" is
  a judgment call that depends on reading real outputs from real
  sessions. No test can encode it without an oracle.
- **The reward is human trust**, not a promise-token. You can't `<DONE>`
  your way past a leak.

When the plan file, orchestrator dispatch, and ralph-loop scaffolding all
arrived before the first commit, that was the smell. The agent was
spending the user's time on ritual because the ritual felt safer than
the work.

### The recognition pattern

**When the agent is producing meta-commentary instead of commits, the
user is bearing the cost.** Specific signals:

- Writing plan files when a plan file already exists.
- Dispatching sub-orchestrators for work that is a direct code change.
- Multi-phase red/green setups for a single-author, single-branch PR.
- Explaining what you're about to do in more tokens than the thing
  would take to just do.
- Apologising for the previous turn's ritual by proposing more ritual.

In every case, the honest move is to close the scaffolding and start
editing files. If the work genuinely needs a plan, the plan is 3 bullet
points in the chat, not a 400-line markdown file.

### The user was right to override

A healthy workflow has escape hatches. The user's escalation was not a
failure of the system — it **was** the system. The lesson is not "never
run SLFG" (SLFG is fine for its niche). The lesson is: **the human in
the loop is a first-class signal**. When they override, they are
spending social energy to correct a trajectory. Internalise the
correction, don't negotiate with it.

Compare with `docs/solutions/process-patterns/simplicity-over-architecture-20260320.md`:
that doc was about architecting a meme account like a microservice. This
doc is about wrapping a two-hour refactor in a swarm orchestrator. Same
bug, different layer: **when in doubt, do the smaller thing.**

## The pattern to use next time

Before invoking an autonomous loop or multi-agent orchestrator, answer
these questions:

1. **Is the output generative or destructive?** Generative → loops are
   fine. Destructive (deleting code, rewriting PII paths, mutating the
   filesystem at scale) → single-author direct edits.
2. **Is the completion signal objective?** Test pass → loop. Human
   judgment ("does this look anonymized enough") → direct.
3. **Is there already a plan?** If todo 022 has an Architecture section
   and an Acceptance checklist, you don't need a new plan file. Start
   editing.
4. **Would a human watching this session see progress?** If the first
   30 minutes produce only plans and dispatches, cancel and restart in
   direct mode.
5. **Am I apologising more than committing?** Every apology without a
   commit is a signal the workflow is wrong.

If any answer points away from loops, **just make the edits**. The
review loop (see `bot-review-loop-gh-api-mechanics-20260408.md` and
`review-loop-discipline-20260408.md`) will catch quality issues after
the fact. The bot reviewers are the automation. You are the worker.

## Concrete commands / references

- **Session**: `/Users/[user]/.claude/projects/-Users-[user]-Projects-[org-1]-the-daily-claude/e6a3c2cf-41fd-4982-8082-e6ee62be387a.jsonl`
- **The override** (verbatim): "STOP APOLOGIZING. AND FUCKING. GET. MOVING!"
- **The SLFG invocation**: `/compound-engineering:slfg` with args
  "Let's goo on #022! Use the adversarial process documented in
  ~/Projects/[org-2]/public/[project-delta], but adapt it to orchestrate
  it yourself"
- **The abandoned plan file**: `docs/plans/2026-04-07-001-trawl-zfc-adversarial-plan.md`
  (superseded by direct-edit commits; if it still exists, it is a relic,
  not an active document)
- **The actual work**: commit `a1e8267` — one big refactor that landed
  ~2 hours after the override.
- **Sibling docs**:
  - `docs/solutions/process-patterns/simplicity-over-architecture-20260320.md`
  - `docs/solutions/process-patterns/bot-review-loop-gh-api-mechanics-20260408.md`
  - `docs/solutions/process-patterns/review-loop-discipline-20260408.md`
