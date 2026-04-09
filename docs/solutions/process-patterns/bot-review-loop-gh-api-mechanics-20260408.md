---
title: Bot review loop — the gh api mechanics that actually work
category: process
date: 2026-04-08
tags: [gh-cli, bot-reviews, pr-workflow, jq, review-loop]
related_commits:
  - 964ef47  # atomic entry writes (reply thread)
  - c943bcd  # tokeniser PII scrub (reply thread)
  - 382cc35  # tempfile uniqueify (reply thread)
  - 88fed46  # length-aware registry scan
  - 93fd69e  # streaming SHA-256 + balanced-bracket scan
  - eade73e  # round-5 style polish
  - f57348f  # PR #3 squash merge
supersedes_narrative_in:
  - docs/solutions/process-patterns/bot-pr-review-cadence-and-synthesis-20260406.md
---

## Superseded by

The earlier doc (`bot-pr-review-cadence-and-synthesis-20260406.md`) gave the
narrative case for waiting, cross-checking, and threading replies. This doc
supersedes its **mechanics** section (guidance points 4–6) with the actual
executable commands we ran on PR #3 — five review rounds, multiple bots,
dozens of inline threads. Keep reading the old doc for the "why"; use this
one for the "how".

## The situation

PR #3 (`refactor/trawl-zfc-scaffold`, merged as squash `f57348f`) rewrote
~1000 lines of PII-adjacent code across five active review rounds plus one
implicit clean round. Gemini, Copilot, and CodeRabbit each posted inline
findings. The default `gh api repos/.../pulls/N/comments` pager returns 30
comments, and the surface for figuring out *which* threads you've already
answered is jq-shaped, not gh-cli-shaped.

Without a disciplined loop, the agent would either:

- miss unreplied bot threads (default pager truncation)
- post top-level comments that orphan themselves from the line being discussed
- repeatedly re-reply to the same threads because there's no "is this answered" signal
- lose track of which commit addresses which finding

## What we learned

### 1. `--paginate` is not optional

```bash
gh api repos/[org-1]/the-daily-claude/pulls/3/comments
# returns 30 by default — silently truncates anything beyond
```

Every enumeration query needs `--paginate`. Budget for it: a PR with 5
review rounds will have 40–80 inline comments across authors.

### 2. The "unreplied tops" jq one-liner

This is the single most valuable query during a multi-round review loop.
It joins top-level comments against `in_reply_to_id` children to surface
exactly the threads you still owe an answer to:

```bash
gh api --paginate repos/[org-1]/the-daily-claude/pulls/3/comments \
  --jq '.[] | {id: .id, user: .user.login, reply_to: .in_reply_to_id}' \
  | jq -s '
  [.[] | select(.user != "<your-handle>" and .reply_to == null)] as $tops |
  [.[] | select(.user == "<your-handle>" and .reply_to != null) | .reply_to] as $replied_ids |
  {
    unreplied_tops: ($tops | map(select(.id as $id | ($replied_ids | index($id)) | not))),
    total_tops: ($tops | length),
    replied_count: ($tops | map(select(.id as $id | $replied_ids | index($id))) | length)
  }
'
```

Replace `<your-handle>` with the account posting replies. The shape:

- `$tops` = every top-level (non-reply) comment NOT authored by us
- `$replied_ids` = the `in_reply_to_id` values of our own replies
- Difference = what still needs an answer

Run this after every push to figure out which threads to address. Run it
again before merging to confirm zero `unreplied_tops`.

### 3. Replies go to a different endpoint than the thread

A top-level PR comment orphans itself from the line being discussed.
Gemini's round-2 finding on `atomic_write` sat on `main.rs:250` — if you
reply via `gh pr comment`, the reply is not attached to that line and other
bots (and humans) scrolling the file won't see it. Use the threaded-replies
endpoint instead:

```bash
gh api -X POST \
  repos/[org-1]/the-daily-claude/pulls/3/comments/<PARENT_ID>/replies \
  -f body="Fixed in 964ef47. Entry writes now go through atomic_write() — tempfile + rename, clean up the tempfile on failure. Three tests cover happy path, nested mkdir, and overwrite."
```

The `<PARENT_ID>` is the top-level comment's `id` from the enumeration
query above. Not the `pull_request_review_id`. Not the `node_id`. The
integer `id`.

### 4. Batch replies via heredoc

For a review round with 6–10 findings, inlining shell + heredoc keeps the
batch readable and atomic:

```bash
gh api -X POST \
  repos/[org-1]/the-daily-claude/pulls/3/comments/3051944007/replies \
  -f body="$(cat <<'EOF'
Fixed in 382cc35. Tempfile names now include the pid + a process-wide
`static AtomicU64` counter, so two concurrent atomic_write calls on the
same target never collide on the intermediate file. Added a stress test
that spawns 8 threads writing to the same path.
EOF
)" 2>&1 | jq '{id, html_url}'
```

Piping through `jq '{id, html_url}'` at the end gives you a confirmation
URL per reply without flooding context with the full response body.

### 5. Name the commit in the reply, every time

Every reply should cite the commit hash that ships the fix. The reader
(human or bot) has to be able to check the work without scrolling the PR.
Gemini's round-3 "thanks, this looks solid" summary literally listed the
hashes it had verified — that feedback loop only worked because our replies
named commits explicitly.

## The pattern to use next time

1. **Push** → wait the Gemini window (6–12 min).
2. **Enumerate unreplied** with the jq one-liner above.
3. For each unreplied thread, decide:
   - fix + reply with commit hash, OR
   - reject + reply with rationale (see `review-loop-discipline-20260408.md`)
4. **Push fixes** as a single focused commit; do not bundle unrelated work.
5. **Re-enumerate** to verify `unreplied_tops == []`.
6. **Re-request** the bots explicitly: `@gemini-code-assist review`,
   `@codex review`. Copilot does not respond to mentions — only to the
   initial PR open — so don't wait for it past round 1.
7. **Merge** only when the enumeration shows zero unreplied tops AND the
   latest bot summary comment acknowledges the latest commit hashes.

## Concrete commands / references

- **Session**: `/Users/[user]/.claude/projects/-Users-[user]-Projects-[org-1]-the-daily-claude/e6a3c2cf-41fd-4982-8082-e6ee62be387a.jsonl`
- **PR**: https://github.com/[org-1]/the-daily-claude/pull/3 (merged as `f57348f`)
- **Prior narrative doc**: `docs/solutions/process-patterns/bot-pr-review-cadence-and-synthesis-20260406.md`
- **Sibling doc on rejection/hygiene**: `docs/solutions/process-patterns/review-loop-discipline-20260408.md`
- **Bot hallucination lesson**: `docs/solutions/process-patterns/bot-reviewers-hallucinate-cli-flags-20260406.md`
