---
title: Review-loop discipline — reject bad suggestions, scrutinise your own fixes, keep commits scoped
category: process
date: 2026-04-08
tags: [review-loop, bot-reviews, commit-hygiene, self-scrutiny]
related_commits:
  - 93fd69e  # streaming SHA-256 + balanced-bracket scan (the bad fix)
  - 88fed46  # length-aware registry scan (the bad fix's fix)
  - 1198e42  # rebase-during-review upstream commit (stale-comment source)
  - 382cc35  # tempfile uniqueify — fix landed before Gemini's comment arrived
  - eade73e  # round-5 polish
  - f57348f  # PR #3 squash merge
---

## The situation

Five review rounds on a ~1000 LOC PII-critical refactor. Three bots on
different cadences. An upstream rebase mid-review. A `git status` showing
70+ files of unrelated pending work. Plenty of opportunities to do the
wrong thing.

This doc captures four discipline patterns the PR #3 loop taught us that
are not about mechanics — they are about judgment. Companion doc for the
mechanics is `bot-review-loop-gh-api-mechanics-20260408.md`.

## What we learned

### 1. Rejection is a valid reply

Round 5 had a vague Gemini comment along the lines of "consider improving
error handling here" on code that already used `anyhow::Context` with
specific messages. Applying every suggestion reflexively is a tax on the
codebase: you end up with churn and no signal on what mattered.

**The rule**: if you can articulate *why* the suggestion is wrong or
out-of-scope in two sentences, reject it in the reply and keep moving.
Rejection worth practising:

- "Already handled — `run_claude` wraps stderr via `.context()` on line N."
- "Out of scope — the validator only detects, it does not rewrite. Tracked
  as todo-NNN if we later decide to auto-fix."
- "Rejected — smoke-tested end-to-end on three real sessions, the stdin
  path works. Passing positionally re-introduces the clap variadic-eats-arg
  bug documented in commit 77d8562."
- "Rejected — the suggestion conflicts with the shared `atomic_write`
  helper design (same tempfile directory so rename stays a single syscall).
  The bot can't see the helper's callsites."

**Track the ratio**. On PR #3, about 70% of findings were applied and 30%
rejected with rationale. If your rejection ratio drops to zero you are
reflexively applying; if it climbs past half you are probably ignoring
real findings.

### 2. Your fix is new code — try to break it before pushing

Round 2 flagged our JSON-array extractor as fragile. The first-pass fix
added a `[\n` / `[{` heuristic to find the array opener. It was **worse**
than the original: nested JSON objects inside the array (`[{"foo": {"bar":
[1,2]}}]`) would now get mis-detected because the first `[{` was the outer
opener but the second `[{` wasn't. Round 3 of the same bot caught this
regression.

**The rule**: when you address a finding, the fix is new code and deserves
the same scrutiny as the original. Specifically:

- Write one adversarial test case before committing (nested structures,
  empty arrays, prose preamble, fenced markdown).
- Ask yourself "what input makes my shortcut wrong" and try that input.
- Don't ship the first idea that compiles.

The eventual correct fix (commit `93fd69e`) was a balanced-bracket scanner
that tracked depth across quoted strings. It took one more iteration to
ship, but round 3 didn't find a regression on it.

### 3. Rebase-during-review creates phantom findings

Mid-loop, an upstream fix for a different concern landed on main
(`1198e42` — port `scrub_profanity_text` inline after anonymize.rs
removal). Rebasing the feature branch onto it caused Gemini's round-4
comment on the `atomic_write` tempfile race to arrive **after** the fix
(`382cc35`) was already in the remote HEAD. The bot was looking at a
commit that no longer existed.

**The rule**: when rebasing during review, your view and the bot's view
are briefly desynchronised. Reply to stale comments with the timing
explanation, not as if they were new findings:

> "Fixed in 382cc35 (before this comment was posted — the bot was likely
> reviewing the pre-rebase commit). The tempfile name now includes pid +
> nanosecond counter, and a stress test covers the concurrent-write case."

Do not silently ignore a stale comment. Do not reimplement a fix because
you panicked. Just explain the timing.

### 4. Scope every commit to the files you touched

The working tree across this session had 70+ uncommitted files unrelated
to the refactor: slide PNGs from another branch's render run, new content
entries, todo files, stale lock files. Running `git add -A` at any point
would have polluted the PR with unrelated noise and broken the squash-
merge narrative.

**The rule**: never `git add .` or `git add -A` on a feature branch when
your working tree has unrelated pending work. Enumerate the files your
commit touches:

```bash
# Good — scoped, reviewable
git add crates/trawl/src/registry.rs crates/trawl/src/main.rs

# Bad — pulls in every slide PNG and entry file you forgot about
git add -A
```

If the enumeration gets tedious, that's a signal your commit is doing
too much. Split it.

A sibling practice: before every commit, run `git status` and read the
staged list out loud. If any file makes you go "wait, why is that
staged?" — unstage it and investigate.

## The pattern to use next time

- Every bot finding gets one of three outcomes: **fix + commit hash**,
  **reject + rationale**, **defer + todo file**. No silent drops.
- Before pushing a fix, write one adversarial input that should have
  broken the original. If your fix handles it, ship. If not, iterate.
- After a rebase, assume the next bot round will have at least one stale
  comment. Reply with the timing, not with another fix.
- Stage files by name on feature branches. Treat `git add -A` as a
  code smell unless you just ran `git clean -fd` first.

## Concrete commands / references

- **Session**: `/Users/[user]/.claude/projects/-Users-[user]-Projects-[org-1]-the-daily-claude/e6a3c2cf-41fd-4982-8082-e6ee62be387a.jsonl`
- **PR**: https://github.com/[org-1]/the-daily-claude/pull/3 (merged as `f57348f`)
- **Bracket regression commits**: `93fd69e` (correct fix), with the
  flawed intermediate first-pass caught between rounds 2 and 3.
- **Rebase-stale example**: commit `382cc35` landed before Gemini posted
  its round-4 comment on the same code; see reply on comment `3051944012`.
- **Mechanics companion**: `docs/solutions/process-patterns/bot-review-loop-gh-api-mechanics-20260408.md`
- **Prior cadence doc**: `docs/solutions/process-patterns/bot-pr-review-cadence-and-synthesis-20260406.md`
- **Bot hallucination lesson**: `docs/solutions/process-patterns/bot-reviewers-hallucinate-cli-flags-20260406.md`
