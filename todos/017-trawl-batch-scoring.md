---
title: "Trawl two-stage batch scoring (HEAT)"
priority: superseded
status: superseded
superseded_by: 022
---

> **Superseded 2026-04-07 by `todos/022-trawl-zfc-redesign.md`.** The
> Sonnet audit empirically proved that ZFC outperforms framework-cognition
> scoring. There is no batch step in the new design — Sonnet handles a
> whole session per call, and Haiku handles anonymization per entry.

# Trawl Two-Stage Batch Scoring

## Problem

Trawl spawns one `claude -p --model haiku` subprocess **per scoring window**.
Each invocation pays the full Claude Code CLI cold-start before any model work
happens. On a full corpus sweep across `~/.claude/projects/` the cumulative
cold-start tax dominates everything else — confirmed empirically 2026-04-06,
where the process sat at 0.0% CPU most of the time, blocked on subprocess
startup, and surfaced only ~15 entries (215–229) before being parked.

We can't switch to direct Anthropic API calls — that'd cost real money on top
of the existing Claude subscriptions, which is a hard no.

## Fix: Two-stage shaped-charge scoring (HEAT round)

A small precursor charge clears the way, then the main jet does the real work
in parallel. Apply per **batch**, not per window.

The Trawl chunks the corpus into batches of N windows. For each batch, in
serial:

1. **Trawl writes a batch directory** containing the N window files
2. **Trawl spawns one `claude -p --model sonnet`** pointed at that directory
   — this is the precursor cold start, paid **once per batch**, not once per
   window
3. **The orchestrator fans out N Haiku subagents in parallel via Task** —
   each subagent reads one file, scores it against the rubric, returns JSON.
   Task subagents launched from inside an active Claude Code session do not
   pay subprocess cold-start — they're in-process model calls on the
   already-running runtime, and they parallelise
4. **The orchestrator collects** subagent results into a JSON array, prints
   to stdout
5. **Trawl parses**, reattaches scores to windows, writes any extractions,
   and moves to the next batch

Cold starts paid = number of batches, not number of windows. Inside each
batch, scoring runs in parallel rather than serially.

## Layout per batch

```
/tmp/trawl-batch-<uuid>/
  windows/
    window-001.txt
    window-002.txt
    …
    window-N.txt
  rubric.md
  README.md   ← outer prompt
```

Outer prompt (lives in `crates/trawl/prompts/orchestrator.md`):

> Read `rubric.md`. List every file in `windows/`. For each file, spawn a
> Haiku subagent via the Task tool with the rubric and the file's contents.
> Run all subagents in parallel in a single dispatch. When every subagent
> returns, print a JSON array `[{"file": "window-NNN.txt", "scores": {…}},
> …]` to stdout and exit.

## Implementation

- [ ] New scoring path: `score_batch(windows: &[Window]) -> Vec<Score>` —
      processes one batch via one `claude -p` call
- [ ] Tunable batch size, configurable via `--batch-size` — controls both
      directory size and orchestrator-side parallelism
- [ ] Write each window to its own file in the batch dir; filenames carry an
      index so Trawl can reattach scores
- [ ] Outer prompt template embeds the per-window rubric verbatim
- [ ] **Single** `claude -p --model sonnet` invocation per batch (Sonnet for
      the orchestrator, Haiku via Task subagents for the actual scoring)
- [ ] Robust JSON parsing — if the batch response is malformed, fall back to
      per-window scoring for that batch only (don't lose work)
- [ ] Clean up the batch tmpdir on success; preserve on failure for debug
- [ ] Bench against the per-window baseline before merging

## Edge cases

- **Subagent count limits.** Find the per-session ceiling on parallel Task
  subagents and pick a batch size that stays under it.
- **Token budget per subagent.** Each Haiku subagent gets one window — well
  under any context limit. The orchestrator only collects scores.
- **Partial JSON.** If the orchestrator returns scores for some but not all
  files, retry the missing ones individually rather than re-running the
  whole batch.
- **Order independence.** Each batch is an isolated unit of work — no batch
  depends on a previous batch's results. Batches run **serially** so the
  host isn't trying to run multiple Claude Code sessions at once.
- **Failure isolation.** If a single subagent errors, the orchestrator emits
  `{"file": "...", "error": "..."}` for that entry and continues. Trawl
  retries failed windows individually after the batch.
- **Cold-start budget.** Cold starts paid = number of batches, not number of
  windows. If the implementation ever drifts back toward per-window
  invocations, the design has failed.

## Why this works on subscriptions

- Every model call still goes through `claude -p`, which uses the local
  Claude Code session auth (subscription, not API key)
- Task subagents spawned inside that session run on the same auth
- Zero new dependencies, no API key, no metered billing

## Acceptance

- [ ] Exactly **one** `claude -p` subprocess per batch (verify with `pgrep`
      during bench) — never per window
- [ ] Same or better entry quality vs single-window scoring
- [ ] No new dependencies, no API key, no subscription change
- [ ] Falls back gracefully to per-window scoring on a single-batch failure
