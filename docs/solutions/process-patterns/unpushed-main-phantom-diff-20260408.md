---
title: "Push local main before branching: unpushed commits become phantom PR diffs"
category: process
date: 2026-04-08
tags: [git, pr-workflow, merge-base, github, branching-hygiene]
related_commits:
  - b7d52bc  # the unpushed local main commit
  - 10a240a  # PR #6 squash (the PR that ended up cleaning up the mess)
---

# Push local main before branching, or inherit phantom PR diffs

## The incident

Session started 2026-04-08 with local `main` at `b7d52bc` — a
"docs: compound learnings from PR #3" commit made locally but
never pushed. Remote `origin/main` was one commit behind at
`f57348f` (the PR #3 squash merge).

Neither state was inherently wrong. Local and remote diverged by
one docs-only commit that `git log --oneline` showed at the top
of main:

```
b7d52bc (local HEAD) docs: compound learnings from PR #3 ...
f57348f (origin/main) fix(daily-claude): port scrub_profanity_text ...
c557654 docs: compound learnings from PR #2 ...
```

Mid-session I branched `fix/trawl-pr3-followup-todos` off
**local** main (`b7d52bc`), committed ~11 files of code + 210
data files, pushed to origin. Opened PR #4 → GitHub reported
**240 files changed**. Opened PR #5 (210-entry backfill) →
**229 files changed**. CodeRabbit immediately bounced PR #4:

> Too many files! This PR contains 240 files, which is 90 over
> the limit of 150.

240 = 221 (my real diff) + 19 (b7d52bc's docs files). The
"phantom" 19 were the compound-learnings docs commit that was
sitting on local main, carried into every new branch I cut from
local main, and published to GitHub as part of the branch push.

## Why this happens

1. GitHub computes a PR's diff as `merge-base(base, head)..head`.
2. "Base" is `origin/<base_branch>`, not `local/<base_branch>`.
3. If local main has commits that are not yet on origin main,
   **every branch you cut from local main carries those commits**,
   and a PR opened against origin main will show them.
4. Bot reviewers and humans both see the extended diff as "the
   PR's content", because from origin's perspective it is.

The phantom commits are not data corruption — they're real commits
in the branch's history. They're "phantom" only in the sense that
the author didn't mean them to be part of this PR's review scope.

## Symptoms to watch for

- `gh pr view <n> --json files | jq '.files | length'` returns a
  number significantly higher than `git log --oneline origin/main..HEAD`
  would imply
- PR diff contains files you didn't touch in any of your commits
- Bot reviewers file findings on files that are "not really yours"
- `gh pr diff <n> --name-only | grep -v <expected-scope>` shows
  unexpected paths
- `baseRefOid` in `gh pr view --json baseRefOid` matches an
  older-than-expected commit on origin main

## The fix

**Push local main first**, then branch:

```bash
SSH_AUTH_SOCK=~/.ssh/agent.sock git push origin main
git checkout -b feature/x main
```

This is a **fast-forward** publish of an existing local commit —
not a force-push, not a rewrite, not destructive. If origin main
is protected and direct pushes aren't allowed, the fast-forward
push will fail with a clear message and you know to open a PR
for the local commit first.

### If you've already opened PRs with phantom diffs

Pushing main mid-loop works *after the fact* but GitHub's PR diff
cache can lag. In the 2026-04-08 incident, pushing `b7d52bc` to
origin main advanced the merge-base for `fix/trawl-pr3-followup-todos`
locally to `b7d52bc` (confirmed via `git merge-base
origin/main <branch>`), but `gh pr diff <n>` kept showing the
stale 240-file set for a while. The bot reviewers fetched the
branch directly and saw the correct merge-base, so their
subsequent reviews (after the push) were scoped correctly.

### Detection pre-flight

A three-line check to run before `git checkout -b`:

```bash
local_main=$(git rev-parse main)
SSH_AUTH_SOCK=~/.ssh/agent.sock git fetch origin main
remote_main=$(git rev-parse origin/main)
if [ "$local_main" != "$remote_main" ]; then
    echo "WARNING: local main ($local_main) differs from origin/main ($remote_main)"
    git log --oneline "$remote_main..$local_main"
fi
```

If the output is non-empty, decide: push those commits first, or
rebase them away, or branch from `origin/main` instead of
`main`.

## Adjacent failure mode: committed-but-untracked

A cousin of this bug: a commit that was made on a feature branch
(correctly), squash-merged to remote main (correctly), but the
local main pointer was never updated, and then a new feature
branch was cut from the stale local main. The new branch's
merge-base is the *squash-merge preview* state, which doesn't
exist on remote main — every file in the original feature
branch's history gets recounted as a diff against the current
origin main.

Same fix: `git pull --ff-only origin main` after any squash
merge, before cutting the next branch.

## Cost of this incident

- 1 reset + re-stage on fix/trawl-pr3-followup-todos to split
  the PR (we'd have done this anyway because of the CodeRabbit
  file cap)
- 1 small doc PR (#6) to address nits bots left on the phantom
  files, because those files actually *were* on main and the
  finding was valid even if mis-attributed
- ~15 minutes of "why are there 19 extra files in my diff?"
  investigation

Cheap-ish, but every minute spent on it is not spent reviewing
the actual code changes.

## Related

- `pr-split-for-bot-review-caps-20260408.md` — the split that
  was driven partly by this
- `bot-review-loop-gh-api-mechanics-20260408.md` — `gh pr diff
  --name-only` + `gh pr view --json baseRefOid` for the
  detection commands above
