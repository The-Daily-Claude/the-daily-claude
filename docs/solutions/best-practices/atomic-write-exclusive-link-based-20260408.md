---
title: "atomic_write_exclusive via link(2): race-safe create-new publish"
category: technical
date: 2026-04-08
tags: [rust, atomic-write, concurrency, posix, link, file-system, tempfile, raii, name-max]
related_commits:
  - 46c487b  # PR #4 squash
  - 2242cfe  # sidecar_tmp_path refactor + NAME_MAX truncation
  - 7fcddfc  # TmpFileGuard RAII cleanup for atomic_write
  - 4672ece  # original atomic_write_exclusive + probe-and-retry
supersedes_narrative_in:
  - docs/solutions/best-practices/atomic-write-monotonic-tempfile-suffix-20260408.md
---

# Atomic publish-only-if-new via POSIX `link(2)`

## Problem

The original `atomic_write(final_path, contents)` helper (see
`atomic-write-monotonic-tempfile-suffix-20260408.md`) uses
`rename(2)` to publish a sidecar tempfile over the target — but
`rename` silently overwrites any existing target. For Trawl's entry
writer that's the wrong behaviour: when two Trawl processes run
concurrently against the same `content/entries/` directory they can
both compute the same `next_number`, and the second writer
happily clobbers the first writer's entry.

CodeRabbit's PR #4 review filed this as `#027` — the concurrent
entry-number race. The naive fix (advisory lock, probe-and-write)
has a TOCTOU window between "does this file exist?" and "write
it". We needed a primitive that is atomically "create iff missing".

## What we learned

POSIX `link(2)` **fails with `EEXIST`** if the target exists — that
is the atomic create-iff-missing primitive we need, for free,
without advisory locks. Combined with the existing sidecar
tempfile:

```rust
pub fn atomic_write_exclusive(final_path: &Path, contents: &[u8]) -> Result<bool> {
    let tmp_path = sidecar_tmp_path(final_path)?;
    let _guard = TmpFileGuard(&tmp_path);

    fs::write(&tmp_path, contents)
        .with_context(|| format!("write tempfile {}", tmp_path.display()))?;

    match fs::hard_link(&tmp_path, final_path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e).with_context(|| format!("link {} -> {}", tmp_path.display(), final_path.display())),
    }
}
```

- `Ok(true)` = published the new entry at `final_path`
- `Ok(false)` = the target was already there; caller can renumber and retry
- `Err(_)` = any other I/O failure (disk full, permission, cross-device)

The caller wraps it in a probe-and-retry loop:

```rust
let start_number = next_number;
let mut write_attempt = 0u32;
let final_filename = loop {
    if write_attempt >= 1024 {
        eprintln!("  write failed: exhausted 1024 retries (started at {start_number}, reached {next_number})");
        break None;
    }
    let filename = entry.filename(next_number);
    let path = cli.output.join(&filename);
    match atomic_write_exclusive(&path, body_bytes) {
        Ok(true) => break Some(filename),
        Ok(false) => { next_number += 1; write_attempt += 1; }
        Err(e) => { eprintln!("  write failed for {filename}: {e:#}"); break None; }
    }
};
```

`next_number` becomes an *advisory lower bound*, not a claim. The
1024-retry cap is a safety net against a pathological directory
state (e.g. someone manually filling 1024 consecutive slots) —
in practice the loop terminates in one iteration for a solo run
and two or three iterations for concurrent runs.

### Cross-process safety comes free

`link(2)` is a single kernel syscall, not a library primitive. Two
Trawl **processes** racing on the same target will go through the
kernel's inode table and exactly one wins — no userspace
coordination needed, no lockfile inside `content/` (which would
conflict with `todos/023-trawl-state-out-of-repo.md`).

### `TmpFileGuard` cleans up on every failure path

Gemini's PR #4 round-2 review caught a tempfile leak: if
`fs::write(&tmp_path, contents)` fails (disk full, permission) the
`?` returns before any explicit `remove_file` runs, leaving a
zero- or partially-written sidecar on disk. Fixed with a
module-private RAII guard installed *before* the write:

```rust
struct TmpFileGuard<'a>(&'a Path);

impl Drop for TmpFileGuard<'_> {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.0);
    }
}
```

Every exit path — write fail, rename/link fail, `Ok(false)`
EEXIST return, panic unwind, success — goes through `Drop` and
best-effort unlinks the sidecar. On the success paths the inode
is already gone so `remove_file` is a harmless no-op. Applied to
both `atomic_write` and `atomic_write_exclusive` (the original
`atomic_write` had the same latent bug).

### `sidecar_tmp_path` shared helper

Both atomic-write variants computed the same parent +
`create_dir_all` + nonce + tmp_name. Gemini's PR #4 round-3
review called out the duplication. Extracted
`sidecar_tmp_path(final_path) -> Result<PathBuf>` so exactly one
place owns:

- Parent derivation and `create_dir_all`
- `Path::file_name()` extraction (basename-only guarantee)
- NAME_MAX truncation (see below)
- Monotonic nonce from `TMPFILE_COUNTER`
- Final `format!(".{name}.tmp.{pid}.{nonce}")`

Both public functions collapse to four lines of glue each. Any
future addition to sidecar naming (e.g. a checksum suffix, a
different separator) changes exactly one place.

### NAME_MAX defense via UTF-8-boundary truncation

Gemini's PR #4 round-3 review also flagged NAME_MAX: POSIX
`NAME_MAX` is typically 255 bytes and the sidecar adds ~37 bytes
of overhead (`.<source>.tmp.<pid>.<u64_nonce>`), so a source
filename of ~220 bytes would produce an illegal sidecar that
fails `fs::write` with `ENAMETOOLONG`.

Fixed by capping the source-name component at
`MAX_SIDECAR_NAME_BYTES = 200` with a UTF-8-boundary-aware
truncation:

```rust
const MAX_SIDECAR_NAME_BYTES: usize = 200;

let file_name_str = file_name.to_string_lossy();
let truncated: &str = if file_name_str.len() > MAX_SIDECAR_NAME_BYTES {
    let mut end = MAX_SIDECAR_NAME_BYTES;
    while end > 0 && !file_name_str.is_char_boundary(end) {
        end -= 1;
    }
    &file_name_str[..end]
} else {
    &file_name_str
};
```

The `is_char_boundary` walk handles multi-byte codepoints: a
200-byte cut that lands mid-codepoint would produce invalid UTF-8
and `&str[..end]` would panic. Walking backward to the nearest
boundary costs at most 3 bytes (UTF-8's max codepoint width) and
is O(1).

Tests prove all three cases: long pure-ASCII truncation, long
4-byte-codepoint truncation, and an end-to-end
`atomic_write` with a 237-byte target filename (legal on APFS
NAME_MAX=255) that would have exploded without the truncation.

## Test coverage (`state::tests`)

Every piece has a test:

- `atomic_write_exclusive_publishes_when_target_missing`
- `atomic_write_exclusive_refuses_to_overwrite`
- `atomic_write_exclusive_parallel_writers_elect_single_winner`
  — 32 threads race on the same target, exactly 1 winner, 31
  clean `Ok(false)` losers, no sidecar litter
- `sidecar_tmp_path_truncates_long_source_names`
- `sidecar_tmp_path_truncates_on_utf8_char_boundary` — 400 bytes
  of 4-byte `𐐷` codepoints truncate cleanly on a boundary
- `atomic_write_handles_near_namemax_filenames` — 237-byte
  target flows through write → rename without ENAMETOOLONG

## When to reach for this

- **Any write where "exactly one writer wins" is the contract.**
  Entry publishing, lockfile-style beacons, "create a new shard"
  patterns.
- **Cross-process races** — `link(2)` is a single kernel syscall,
  no userspace coordination needed.
- **When you can renumber on loss.** If the caller can't retry
  (e.g. the target name is fixed), this primitive isn't the fix
  — use a true advisory lock instead.

## What not to do

- Don't probe `path.exists()` before writing — it's TOCTOU-racy.
- Don't use `OpenOptions::create_new(true)` on `final_path`
  directly — that gives atomic create-new but you lose
  crash-safety (a partial write leaves a corrupt target).
- Don't advisory-lock a file inside `content/` — conflicts with
  `todos/023-trawl-state-out-of-repo.md` which wants state out of
  the content tree.
- Don't rely on `rename(2)` with overwrite semantics when you
  actually want create-new — silent clobbers are the single
  hardest-to-reproduce class of data-loss bug.

## Related

- `atomic-write-monotonic-tempfile-suffix-20260408.md` — the
  original `rename(2)`-based helper this extends
- `todos/027-coderabbit-trawl-entry-number-race.md` — original
  coderabbit finding that drove this work
- `crates/trawl/src/state.rs` — current production implementation
- `crates/trawl/src/main.rs` — the probe-and-retry caller
