---
title: "Concurrent trawl invocations can collide on next entry number"
priority: medium
status: resolved
source: coderabbit
depends_on: 022
resolved_at: 2026-04-08
resolved_by: atomic_write_exclusive + probe-and-retry in run_trawl
---

## Resolution (2026-04-08)

Two layers:

1. **`state::atomic_write_exclusive` (new)** — same sidecar tempfile +
   `link(2)` instead of `rename(2)`. POSIX `link` fails with `EEXIST`
   if the target exists, which is our race-safe "create new" signal.
   Returns `Ok(true)` on publish, `Ok(false)` if the target was
   already there, `Err` on any other I/O failure. Sidecar is removed
   regardless of outcome so no tempfile litter.
2. **Probe-and-retry loop in `run_trawl`** — `next_number` is now just
   an advisory lower bound. For each draft we call
   `atomic_write_exclusive`; on `Ok(false)` we bump the number and
   try again. Bounded at 1024 retries so a pathologically corrupted
   directory can't wedge the binary.

Tests added to `state::tests`:
- `atomic_write_exclusive_publishes_when_target_missing`
- `atomic_write_exclusive_refuses_to_overwrite`
- `atomic_write_exclusive_parallel_writers_elect_single_winner` —
  32 threads race on the same target; exactly one winner, 31 clean
  `Ok(false)` losers, no tempfile litter.

Cross-process race is covered by the same primitive since `link(2)` is
an atomic kernel syscall — two trawl processes computing the same
`next_number` will race at the syscall level, exactly one wins.

Related `todos/023-trawl-state-out-of-repo.md` still stands on its own
merits (state file location), but the race fix here no longer waits
on it — there is no lockfile inside `content/`.

# Concurrent trawl invocations can collide on next entry number

## Finding

`run_extract` computes `next_number` once via
`max_existing_entry_number(&cli.output)` and then increments locally.
If two trawl processes run in parallel against the same
`content/entries/` directory, both can read the same base number and
overwrite each other's output files. The existing code comment says
"all writes go through this binary" but it does not address multiple
concurrent instances of the same binary.

## Location

`crates/trawl/src/main.rs:143-145`

## Proposed fix

Acquire a process-wide advisory lock (e.g. `fs2::FileExt::try_lock_exclusive`
on a lockfile in `cli.output` or a sibling of it) before calling
`max_existing_entry_number`, hold it across the whole extraction loop
(or at minimum across each write), and release it once the entries are
flushed to disk. Alternatively: use atomic `O_EXCL` create per
candidate filename and retry on `EEXIST`, renumbering on conflict.

Either approach needs to be consistent with
`todos/023-trawl-state-out-of-repo.md` (where state lives) so locks
don't end up inside `content/` itself.

## Severity

P2 — only bites when multiple trawl processes race. Current workflow
invokes trawl from a single shell at a time, but cron + manual runs
can realistically overlap. Not P1 because no end-user has hit this yet
and the fix needs a small dep or thoughtful lock placement.
