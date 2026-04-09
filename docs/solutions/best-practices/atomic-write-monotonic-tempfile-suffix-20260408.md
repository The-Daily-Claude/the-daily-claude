---
title: Atomic File Writes with a Process-Wide Monotonic Tempfile Suffix
category: technical
date: 2026-04-08
tags: [rust, atomic-write, concurrency, file-system, tempfile]
related_commits: [f57348f]
---

# Atomic File Writes with a Process-Wide Monotonic Tempfile Suffix

## Problem

The Trawl pipeline writes three classes of file: the state cache, the
PII registry, and N entry markdowns per session. Until PR #3 the entry
writer used `fs::write` directly, so a SIGINT (or a panic) mid-write
left a half-truncated entry on disk that the next run would happily
parse and publish.

The standard fix is "tempfile + rename." Easy enough — but the first
draft of the helper named the tempfile `.<final>.tmp.<pid>`. Gemini
correctly flagged the race in round 2 of the PR review: two threads in
the same process writing the **same** target path collide on the same
sidecar name, and whichever thread loses the race observes the other
thread's half-written contents during its own `fs::write`.

The naive escapes — UUID, random nonce, timestamp — are heavier than
necessary and either pull dependencies or make the failure mode harder
to reason about (timestamps collide on fast hardware, UUID is overkill
for in-process uniqueness).

## What we learned

A `static AtomicU64` counter, bumped per call, gives you a
process-wide monotonic suffix with zero dependencies and a one-line
allocation:

```rust
static TMPFILE_COUNTER: AtomicU64 = AtomicU64::new(0);

// `file_name` must be the basename only — use `Path::file_name()` on
// `final_path` to strip any directory components before formatting
// the tempfile name, otherwise the tempfile ends up in a sibling dir.
let file_name = final_path
    .file_name()
    .ok_or_else(|| anyhow!("path has no file name: {}", final_path.display()))?;
let nonce = TMPFILE_COUNTER.fetch_add(1, Ordering::Relaxed);
let tmp_name = format!(
    ".{}.tmp.{}.{nonce}",
    file_name.to_string_lossy(),
    std::process::id()
);
```

Why each piece matters:

- **`pid`** disambiguates across processes (one Trawl run vs. another).
- **The atomic counter** disambiguates across threads inside a single
  process — every call gets a fresh value, so two threads writing the
  same `final_path` produce two distinct sidecars.
- **`Ordering::Relaxed`** is sufficient because we're not synchronising
  any other memory through this counter, just claiming a unique
  integer. The hardware atomic does the rest.
- **Same-directory tempfile** keeps `rename(2)` on a single filesystem
  so the commit step is one syscall, not a copy + delete.
- **Cleanup on rename failure** (`fs::remove_file(&tmp_path)`) keeps
  the directory tidy when the rename actually does fail (cross-device,
  permission, etc.).

The contract this guarantees: a reader either sees the previous
version of the target or one of the new payloads — never a partial
write, never a stray sidecar after a successful run, and never a
collision when multiple threads write the same path.

## How to apply

- Whenever you write a "tempfile + rename" helper, name the tempfile
  with both `pid` **and** a process-wide atomic counter. Either alone
  is insufficient.
- `Ordering::Relaxed` is the right pick for "give me a unique number"
  counters. Don't reach for `SeqCst` out of nervousness.
- Pin the contract with concurrency tests: spawn 16+ threads, write
  to both **different** targets and the **same** target, then assert
  no `.tmp.` sidecars survive and the final file is one of the
  expected payloads. The same-target test is the one that catches the
  collision; the different-target test is the one that catches a
  cleanup bug.
- Share the helper across every code path that writes a file. Three
  call sites with three subtly-different open/write/rename sequences
  is three places to forget the cleanup.

## Code pointer

- `crates/trawl/src/state.rs:23-79` — `TMPFILE_COUNTER` and
  `atomic_write` body
- `crates/trawl/src/state.rs:526-604` —
  `atomic_write_parallel_writes_to_different_targets_do_not_collide`
  and `atomic_write_parallel_writes_to_same_target_do_not_collide`
- `crates/trawl/src/main.rs:288` — entry writer call site (the one
  Gemini flagged as non-atomic in round 1)
