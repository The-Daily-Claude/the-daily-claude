---
title: Bot PR review cadence — wait, cross-check, synthesize, push back
date: 2026-04-06
category: process-patterns
module: development_workflow
problem_type: workflow_issue
component: development_workflow
severity: medium
applies_when:
  - Working a PR with bot reviewers (CodeRabbit, Gemini Code Assist, Copilot)
  - Polling for review feedback after pushing commits
  - Deciding whether to accept or push back on a bot finding
  - Threading replies to inline review comments via the gh CLI
tags: [pr-review, coderabbit, gemini, copilot, gh-cli, workflow]
---

# Bot PR review cadence — wait, cross-check, synthesize, push back

## Superseded by

The **mechanics** in this doc (particularly guidance points 4–6 on
enumerating, cross-checking, and threading replies) are superseded by
`bot-review-loop-gh-api-mechanics-20260408.md`, which captures the
executable `gh api` + `jq` snippets that actually worked across PR #3's
five review rounds. The **narrative** here (why to wait, why to cross-
check, why to push back) is still the authoritative rationale — read it
first, then use the newer doc for the commands.

Related companion docs from the PR #3 loop:
- `do-less-ritual-more-work-20260408.md` — when **not** to run
  autonomous loops in the first place.
- `review-loop-discipline-20260408.md` — rejecting bad suggestions,
  scrutinising your own fixes, rebase-stale replies, commit scoping.

## Context

PR #2 (`fix/consistent-slide-rendering`) was reviewed by three different bots
in parallel: CodeRabbit (Free plan), Gemini Code Assist, and Copilot PR
reviewer. Each behaved differently, each on its own clock. The session
revealed several recurring failure modes in how an agent waits for and
interprets bot feedback:

1. Polling reviews 30 seconds after pushing a commit returns nothing, which
   reads as a false "all clear" signal. The user had to explicitly correct
   this: *"They take time. Just like you."*
2. CodeRabbit's Free plan only posts walkthrough/summary comments, no
   line-level findings. If CodeRabbit is the only reviewer queried, the PR
   appears clean even when other bots have raised real issues.
3. The three bots gave three different signals on the same diff. Treating
   any single bot's verdict as authoritative would have either missed a real
   bug (Copilot's bare-`>` finding) or accepted a wrong one (Gemini's
   `--settings` claim).
4. Gemini flagged `--settings` as an unsupported flag with a `high-priority`
   label. It was wrong — `carbon-now --help` lists the flag explicitly and
   the previous render had already produced the expected output. The
   high-priority label is not a verification.
5. Top-level PR comments lose the connection to the line being discussed.
   Threaded inline replies require a different gh API endpoint than
   top-level comments.

## Guidance

### 1. Budget elapsed time for bot reviews

After pushing a commit that should trigger a bot review:

- Wait at least **2–5 minutes** before the first poll. Bots queue, fetch
  the diff, run their model, and post — none of that is instantaneous.
- Re-poll after **every** new commit, not just the first push. Each commit
  re-triggers the bots independently and on their own schedules.
- If a poll returns nothing, the correct interpretation is "not yet,"
  not "clean."

### 2. Cross-check across both review surfaces

GitHub exposes review feedback through two distinct surfaces. Querying
only one will miss findings from bots that post to the other.

```bash
# Inline (line-level) comments — Copilot, Gemini, CodeRabbit Pro post here
gh api repos/:owner/:repo/pulls/<N>/comments

# Review summaries and top-level review bodies — Gemini and Copilot
# post their overview here; CodeRabbit Free posts its walkthrough as an
# issue comment, NOT a review
gh pr view <N> --json reviews

# CodeRabbit Free's walkthrough lives in the issue comments stream
gh pr view <N> --json comments
```

Run all three. Treat any single one returning empty as "this surface is
empty," not "the PR is clean."

### 3. CodeRabbit Free has no inline findings — plan accordingly

CodeRabbit on the Free plan generates only the walkthrough/summary
comment. There will never be line-level findings from it, regardless of
what's in the diff. The comment itself even says so:

> Your organization is on the Free plan. CodeRabbit will generate a
> high-level summary and a walkthrough for each pull request. For a
> comprehensive line-by-line review, please upgrade…

If CodeRabbit is the only reviewer the agent thinks is configured, it
will report the PR as clean every time. Always check whether other bots
(Gemini Code Assist, Copilot PR reviewer) are wired into the repo and
poll their endpoints too.

### 4. Synthesize across reviewers, verify each claim independently

On PR #2:

| Reviewer    | Signal                             | Reality |
|-------------|------------------------------------|---------|
| CodeRabbit  | Walkthrough only, no findings      | False clean (plan limitation) |
| Gemini      | 1 high-priority finding            | Wrong on the main claim, right on a secondary point |
| Copilot     | 3 inline comments on one bug       | Real bug, correct fix |

No single bot was reliable on its own. The correct posture:

- Don't trust any single "all clear" signal.
- Don't trust "high priority" labels as verification — they're a model's
  confidence, not ground truth.
- Verify each claim with the cheapest possible test (often `--help`,
  a one-line script, or a re-run of the actual command). Gemini's
  `--settings` claim was disproven by `carbon-now --help` in two seconds.

### 5. Pushing back is healthy when you have evidence

When a bot is wrong, the right move is neither capitulation nor silence.
Reply with the verification, accept any legitimate sub-points, hold the
line on the rest. Example reply pattern from PR #2:

> **On `--settings`:** This is actually a documented flag in
> carbon-now-cli — `carbon-now --help` lists it explicitly: `--settings
> Override specific settings for this run`. Empirically it works: the
> previous render produced `slide-01.png: 1080x1080 (card 1080x702)` with
> the pinned width applied. So I'm keeping the inline `--settings` over
> a separate config file…
>
> **On `2>/dev/null`:** Fair point — silencing stderr under `set -e` does
> mask failures. Fixed in f768712.

Two parts of one finding, two different outcomes, both grounded in
evidence. This is the model to copy.

### 6. Reply to inline comments via the threaded-replies endpoint

A top-level PR comment loses its connection to the line being discussed.
To thread a reply under an inline review comment:

```bash
gh api \
  --method POST \
  repos/:owner/:repo/pulls/<N>/comments/<COMMENT_ID>/replies \
  -f body="reply text"
```

The `<COMMENT_ID>` comes from the `id` field in
`gh api repos/:owner/:repo/pulls/<N>/comments`. This keeps the
conversation attached to the original line, which matters when other
reviewers (human or bot) read the thread later.

## Why This Matters

Bot reviewers compress hours of human review into minutes, but only if
the agent driving the PR knows how to consume them. The failure modes
above all silently degrade the value of the review:

- Polling too early turns "review pending" into "review clean" and ships
  unverified code.
- Querying only one surface hides entire bots' worth of findings.
- Trusting CodeRabbit Free as the sole signal makes every PR look clean.
- Accepting any single bot's verdict propagates that bot's individual
  errors into the codebase (either by acting on a false positive or by
  ignoring a real bug another bot caught).
- Capitulating to wrong findings degrades the code; ignoring them
  degrades the review relationship. Pushing back with evidence does
  neither.
- Top-level reply comments lose context and make the next reviewer
  re-derive what was being discussed.

The compounding payoff: each PR handled this way teaches the next
session how to interpret the same bots faster, and the threaded replies
leave a durable audit trail.

## When to Apply

- Every PR that has any bot reviewer configured.
- Especially when the repo has multiple bots wired up — the synthesis
  step is where most signal is gained or lost.
- Whenever an agent is about to mark a PR "ready to merge" based on a
  single empty poll.

## Examples

**Wrong cadence (PR #2, first attempt):**

```
push commit → wait 30s → gh pr view --json reviews → empty → "clean!"
```

**Right cadence:**

```
push commit
wait 2-5 min
gh api repos/:owner/:repo/pulls/2/comments        # inline
gh pr view 2 --json reviews                        # review summaries
gh pr view 2 --json comments                       # CodeRabbit walkthrough
synthesize across all three
verify each non-trivial finding with a cheap independent test
reply inline (threaded) with evidence — accept, push back, or fix
push fix → repeat the whole cycle from the top
```

**Threaded reply (PR #2, Gemini's `--settings` finding):**

```bash
gh api --method POST \
  repos/[org-1]/the-daily-claude/pulls/2/comments/3040512642/replies \
  -f body="$(cat <<'EOF'
On --settings: documented flag, carbon-now --help lists it. Empirically works.
Keeping inline --settings over a separate config file.
On 2>/dev/null: fair point under set -e. Fixed in f768712.
EOF
)"
```

## Related

- PR #2: https://github.com/[org-1]/the-daily-claude/pull/2
- `docs/solutions/design-decisions/zero-framework-cognition-20260320.md` —
  same theme: trust the actual signal, not the wrapper around it.
- `docs/solutions/process-patterns/simplicity-over-architecture-20260320.md`
