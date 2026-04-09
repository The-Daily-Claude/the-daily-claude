---
title: "Trawl incremental state + Realm-style migrations"
priority: high
status: pending
---

# Trawl Incremental State

## Problem

Today every Trawl run re-scores every window of every session from scratch.
That's fine on a one-shot run, but the corpus only grows: most sessions are
unchanged between runs, a few have new tail turns, and only the rubric or the
Trawl itself changes occasionally. We're paying for a full sweep every time
when an incremental sweep would do.

## Fix: Per-session manifest + Realm-style migrations

Trawl maintains a state file at `content/.trawl-state.json` (or sqlite if we
ever need range queries) keyed by absolute session path. On every run, Trawl
consults the manifest before touching a session and decides whether to skip,
incrementally scan, or fully re-scrape.

### Per-session record

```json
{
  "/Users/[user]/.claude/projects/.../abc.jsonl": {
    "file_sha256": "…",
    "size_bytes": 184320,
    "mtime": "2026-04-06T19:25:12Z",
    "last_scored_offset": 184320,
    "trawl_version": "0.3.1",
    "rubric_sha256": "…",
    "extracted_window_hashes": ["sha256:…", "sha256:…"]
  }
}
```

### Decision table

| Condition | Action |
|---|---|
| file hash unchanged AND trawl version ≥ last AND rubric hash unchanged | **skip** |
| size grew, mtime newer, prefix bytes match prior content | **incremental** — score only windows containing new turns past `last_scored_offset` |
| file hash changed in a non-append way (truncated, edited mid-file) | **full re-scrape** of that session |
| trawl version bumped through a migration with `rescore: true` | **full re-scrape** of all sessions touched by the migration |
| rubric hash changed | **full re-scrape** of every session |

### Realm-style migrations

Trawl ships a `MIGRATIONS` table mapping version → metadata about what that
version changed:

```rust
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: "0.4.0",
        rescore: true,                    // re-score everything
        reason: "added quotability dim",
    },
    Migration {
        version: "0.4.1",
        rescore: false,                   // bug fix, no re-score needed
        reason: "fixed unicode in titles",
    },
    Migration {
        version: "0.5.0",
        rescore: true,
        reason: "calibrated scoring rubric",
    },
];
```

On startup, Trawl computes the highest unapplied migration per session and
acts accordingly. After a successful run it stamps the current version into
the manifest.

### Window-level dedup survives across runs

`extracted_window_hashes` is the canonical "this window already produced an
entry" list. Even if a window scores high again on re-scrape (e.g. after a
rubric change), Trawl checks the hash set before writing a new entry — same
window never extracts twice. Dedup that today only works within a single run
becomes durable.

## Implementation

- [ ] `state.rs` — load/save `content/.trawl-state.json`, atomic writes
- [ ] On session entry: read manifest, fast pre-check (size + mtime), then
      hash if needed
- [ ] Append-detection: if size grew and the prefix bytes match, we know it's
      a pure append and can resume from `last_scored_offset`
- [ ] `MIGRATIONS` table in `migrations.rs` with version + rescore flag +
      reason
- [ ] Migration runner: walks manifest, marks any session whose stamped
      version is below an `rescore: true` migration as needing full re-scrape
- [ ] Rubric hash: sha256 the prompt template at startup, compare to the
      manifest's stored hash, full re-scrape on mismatch
- [ ] Stamp current trawl version + rubric hash into manifest after successful
      run
- [ ] `--force-rescore` flag for the "I know what I'm doing" case
- [ ] `--dry-run` should report what would be scored vs skipped

## Edge cases

- **Deleted sessions.** Manifest entries with no corresponding file get
  garbage-collected on the next run.
- **Renamed sessions.** Treated as new sessions because the key is the path.
  Acceptable — Claude Code session paths are stable in practice.
- **Manifest corruption.** On parse error, back up the broken file and start
  fresh — this means a one-time full re-scrape, not data loss.
- **Concurrent runs.** Trawl takes a file lock on the manifest at startup
  and refuses to start if another instance holds it.
- **Partial runs.** Trawl flushes manifest entries per session, not per
  batch, so a crash mid-run still preserves everything that completed.

## Acceptance

- [ ] Second consecutive Trawl run with no source changes performs **zero**
      `claude -p` invocations
- [ ] Adding a new session and rerunning scores only the new session
- [ ] Appending turns to an existing session and rerunning scores only the
      new windows
- [ ] Bumping a migration with `rescore: true` triggers a full re-scrape on
      next run
- [ ] Changing the rubric template triggers a full re-scrape
- [ ] Already-extracted windows never produce duplicate entries even if
      re-scored
