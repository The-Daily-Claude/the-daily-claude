---
title: "Split data PRs from code PRs when bot reviewers have file-count caps"
category: process
date: 2026-04-08
tags: [pr-workflow, coderabbit, bot-reviews, pr-hygiene, split-pr]
related_commits:
  - 46c487b  # PR #4 (code)
  - f0a6916  # PR #5 (data)
  - 10a240a  # PR #6 (docs)
---

# Split data PRs from code PRs when bot reviewers have file-count caps

## Problem

PR #4 opened on 2026-04-08 to close six follow-up todos from PR #3,
bundling three kinds of change into a single commit:

- **Code + tests** (~11 files): `state.rs`, `main.rs`,
  `prompts/tokeniser.md`, `scripts/backfill-entities.py`, 7 todo
  files
- **Data backfill** (210 files): `content/entries/*.md`, each with
  a single-line `entities: {}` insertion in frontmatter

Total: **221 files**. CodeRabbit immediately posted:

```
> [!IMPORTANT]
> ## Review skipped
> Too many files!
> This PR contains 240 files, which is 90 over the limit of 150.
> Please upgrade to a paid plan to get higher limits.
```

(The 240 count includes 19 prior-merge docs files that Github's
PR diff engine carried in due to a stale merge-base — see
`unpushed-main-phantom-diff-20260408.md`.)

No review = no P2 bot findings = no hardening loop = no signal
that the code was actually reviewed.

## What we learned

### Data and code belong in separate PRs

The CodeRabbit free tier caps at 150 files. Any corpus-wide
backfill (content, translations, schema migrations, lint
fix-alls) blows through that on its own. Pattern that worked:

1. **Code PR**: the script + unit tests + docstring. Small,
   reviewable, every line worth a line-by-line look.
2. **Data PR**: the output of the script. Mechanical, one-line
   diffs, no semantic review needed. Bot reviewers may skip it —
   that's fine, the review effort belongs on the script that
   produced it.

Splitting PR #4 into two PRs gave us:

- **PR #4**: 11 files, CodeRabbit reviewed it, Gemini did 3
  iterative rounds of review → 2 rounds of P2 findings → clean
- **PR #5**: 210 files (still >150 — no review), merged as a
  trusted data migration

### The split is cheap after the fact

No history rewriting needed. From a branch that contains both
kinds of change, the split is:

```bash
# Reset soft to keep everything staged
git reset --soft HEAD~1

# Unstage the data files
git restore --staged content/entries/

# Recommit the code
git commit -m "fix(trawl): <scope>"

# Branch from main, pick up the data from working tree
git checkout -b chore/backfill-entities-field main
git add -u content/entries/   # only tracked files — safe
git commit -m "chore(content): backfill <field>"

# Force-push the now-smaller code PR
SSH_AUTH_SOCK=~/.ssh/agent.sock git push --force-with-lease origin <code-branch>
# Push the new data branch
SSH_AUTH_SOCK=~/.ssh/agent.sock git push -u origin chore/backfill-entities-field
```

`git add -u content/entries/` is important: it stages **only
tracked** modified files, not random untracked entries from
earlier sessions that happen to be in the same directory. See
CLAUDE.md "Never `git add .` / `git add -A`".

### Cross-reference each PR in the other's description

The two PRs are independent (no merge order required), but the
reader of one should find the other immediately:

- Code PR body: "The actual 210-entry data migration ships in a
  separate PR (\`chore/backfill-entities-field\`) to keep this PR
  under CodeRabbit's free-tier file cap."
- Data PR body: "Split out from PR #X to keep each PR under
  CodeRabbit's free-tier 150-file cap so automated review can run
  on the engineering work."

### Review expectation: script, not output

On the data PR, the review bar is zero (or near-zero). The
reviewer's job is to confirm:

1. The diff matches what the script in the sibling PR is
   documented to do
2. No files changed that the script had no business touching

On the code PR, the review bar is normal: line-by-line,
tests, edge cases.

## Adjacent lesson: docs PRs split too

Same session produced a third PR (#6) for two doc nits Gemini
flagged on files that shouldn't have been in PR #5's diff at all
(stale merge-base artifact). The pattern generalises: if a
finding belongs on main's current state but is being posted on
your in-flight PR, open a tiny separate PR for it rather than
fighting the merge-base.

## When NOT to split

- **Small bundled changes** (< 150 files total). Not worth the
  coordination overhead.
- **Changes where code and data must land together** to avoid a
  broken intermediate state. E.g. adding a new enum variant and
  the data file that references it.
- **Single-commit bug fixes where the data touched is the fix.**
  E.g. fixing a CSV parser bug by updating the fixture CSV — keep
  them together.

## The shape of a healthy split session

From the 2026-04-08 session:

| PR | Scope | Files | Bot review outcome |
|----|-------|-------|-------------------|
| #4 | Code + tests + script + todos | 11 | 3 rounds Gemini → clean, CodeRabbit reviewed |
| #5 | 210-file data migration | 210 | CodeRabbit skipped (still >150), Copilot reviewed but flagged stale-diff noise |
| #6 | 2-file doc nits | 2 | 2 rounds Gemini → clean, Copilot clean |

All three merged cleanly within 20 minutes of opening PR #4.
Total review iterations: 5+ across the three PRs, all in
parallel.

## Related

- `bot-review-loop-gh-api-mechanics-20260408.md` — `gh api +
  jq` for tracking which comments you've replied to
- `bot-pr-review-cadence-and-synthesis-20260406.md` — the older
  narrative guide for review pacing
- `unpushed-main-phantom-diff-20260408.md` — the stale
  merge-base trap that made PR #5's initial diff look 229 files
- `review-loop-discipline-20260408.md` — rejecting vs accepting
  individual findings
