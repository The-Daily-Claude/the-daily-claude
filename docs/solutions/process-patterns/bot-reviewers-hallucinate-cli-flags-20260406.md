---
title: Bot Reviewers Hallucinate CLI Surface — Verify with --help Before Deferring
date: 2026-04-06
category: docs/solutions/process-patterns
module: code-review
problem_type: workflow_issue
component: development_workflow
severity: medium
applies_when:
  - An LLM-based code reviewer (Gemini Code Assist, CodeRabbit, Copilot review, etc.) flags a CLI flag, API method, or config key as "nonexistent" or "incorrect"
  - You are about to revert a change based purely on the bot's claim
tags: [code-review, llm-reviewers, hallucination, verification, workflow]
---

# Bot Reviewers Hallucinate CLI Surface — Verify with --help Before Deferring

## Context

In PR #2, Gemini Code Assist confidently flagged `carbon-now --settings` as
a nonexistent flag. The flag is in fact documented and visible in
`carbon-now --help`. Running `--help` took two seconds and immediately
disproved the review comment. Had the comment been deferred to without
verification, the working fix would have been reverted and the original
bug reintroduced.

This is not a Gemini-specific failure — every LLM reviewer trained on
mixed-vintage docs will at some point hallucinate API surface that does
not exist, *or* deny API surface that does. The failure mode is symmetric.

## Guidance

When an LLM reviewer makes a claim about whether a specific flag, method,
parameter, or config key exists, **verify against the tool itself before
acting on the comment.** The verification ladder, cheapest first:

1. `<tool> --help` (or `--help <subcommand>`)
2. `<tool> --version` followed by checking that version's docs
3. The installed package's source/manpages on disk
4. Upstream documentation for the *exact installed version*

Only after the claim survives at least step 1 should you consider
reverting working code. If the claim *fails* verification, leave a
short reply on the review comment noting what you ran and the result —
this trains future reviewers (human and bot) and creates a paper trail.

## Why This Matters

LLM reviewers sit in a position of asymmetric authority: they comment on
code at scale, in a tone that reads as confident, against an audience
(the PR author) who is often tired and ready to defer. The default of
"the bot is probably right" is exactly backwards for the failure mode
LLMs have. They are most confident on questions of API existence — and
that's the question they get wrong most often, because their training
data conflates versions, forks, and similarly-named tools.

Two seconds of `--help` is the entire defense.

The corollary applies to your *own* certainty as an LLM agent: when you
are about to claim a flag does not exist, run `--help` first. The same
discipline applies in both directions.

## When to Apply

- Any review comment that says "this flag/method/option does not exist"
- Any review comment that suggests reverting a change because of a
  claimed API mismatch
- Your own first draft of a comment claiming nonexistence — verify
  before posting

## Examples

**Smell** — deferring without verification:

> Bot: "The `--settings` flag is not a valid carbon-now option."
> Author: "You're right, let me revert."

**Fix** — verify, then reply:

```bash
$ carbon-now --help | grep -A1 settings
  --settings <settings>  Use a custom Carbon settings JSON string
```

> Author: "Verified with `carbon-now --help` — `--settings` is documented and accepts an inline JSON string. Keeping the change."

## Related

- `tooling-issues/carbon-consistent-frame-20260406.md` — the PR where this happened
- PR #2 (`fix/consistent-slide-rendering`)
