---
title: "Move Trawl state + PII registry out of the content repo"
priority: low
status: done
depends_on: 022
---

# Move Trawl State Out of the Content Repo

## Why

`todos/022` (ZFC redesign) introduces two machine-local files that v1
keeps gitignored inside `content/`:

- `content/.trawl-state.json` — per-session manifest (file hashes,
  prompt hashes, trawl version)
- `content/.pii-registry.json` — accumulated real-world PII for the
  validation pass

Keeping them inside the repo is fine for v1 simplicity, but it has
real downsides:

- One slip on `git add content/` and either file lands in history.
  The registry contains live PII; the consequence is non-recoverable.
- Cloning the repo on a second machine starts state from zero, even
  though the corpus and conventions transfer cleanly.
- Wiping `content/` (e.g. for a clean re-trawl) destroys state that
  has nothing to do with the content itself.
- Two unrelated concerns share a directory: the content the repo
  exists to track, and the runtime memory of a tool that produces it.

## Move target

Canonical reverse-DNS namespace is **`com.the-daily-claude.trawl`** — the
project name is *The Daily Claude*, and the CLI subject is *trawl*. This
matches the long-term plan for the entries stash at
`~/.local/share/com.the-daily-claude.trawl/the-stash/entries/`.

Layout:

```
~/.local/share/com.the-daily-claude.trawl/
  the-stash/
    entries/             # trawl output (YYYY-MM-DD-slug.md, long term)
  pii-registry.json
  trawl-state.json
```

**Cross-platform path resolution:**

- **Linux:** `$XDG_DATA_HOME/com.the-daily-claude.trawl/`, with
  `$XDG_DATA_HOME` defaulting to `~/.local/share/` when unset.
- **macOS:** `$HOME/.local/share/com.the-daily-claude.trawl/` as a
  literal — same path shape as Linux for parity. **Deliberately NOT
  `~/Library/Application Support/`** — we want the same path on both
  platforms so muscle-memory, scripts, and docs transfer cleanly.

The directory is created on first run with `0700` perms. Entries,
registry, and state all live under the same app namespace — per-machine,
outside any repo.

The registry is still gitignored as a defence-in-depth measure (in case
the migration is incomplete on some machine), but its canonical home is
outside the repo.

## Implementation

- [x] Add a `data_dir()` helper that returns
      `$XDG_DATA_HOME/com.the-daily-claude.trawl/` with a fallback to
      `~/.local/share/com.the-daily-claude.trawl/`.
- [x] Update Trawl to read/write `pii-registry.json` and `trawl-state.json`
      from `data_dir()` instead of `content/`.
- [x] Update Trawl's default output directory for extracted entries to
      `data_dir()/the-stash/entries/` (override via `-o` flag as today).
- [x] Update `trawl validate` and any other commands that touched the
      old paths.
- [x] Update the doc comment at the top of `crates/trawl/src/registry.rs`
      to reference the new location instead of `content/.pii-registry.json`.
- [x] Document the new location in HANDOFF.md and the project README.

Deliberate simplification from the original sketch: no migration logic and no
manual permission management. Old repo-local files are left in place; the
atomic write helpers create the new parent directories on first write.

## Cross-platform notes

- **Linux:** honour `$XDG_DATA_HOME` when set, default to `~/.local/share/`
  when unset. Either resolves to the canonical
  `~/.local/share/com.the-daily-claude.trawl/`.
- **macOS:** use `$HOME/.local/share/com.the-daily-claude.trawl/` verbatim.
  **Do not** use `~/Library/Application Support/` — cross-platform path
  parity is more valuable than Apple's preferred location for a CLI tool.
  Write a small helper (don't just delegate to `dirs::data_dir()` which
  would give you the Apple path on macOS).
- **Windows:** not a target today. If it becomes one, pick whatever maps
  cleanly from a user's shell under `%LOCALAPPDATA%\com.the-daily-claude\trawl\`
  or similar — cross that bridge if and when.

## Acceptance

- [x] State + registry + stash all live under
      `$XDG_DATA_HOME/com.the-daily-claude.trawl/` by default
- [x] Wiping `content/` no longer destroys runtime state or stash
- [x] Cloning the repo on a fresh machine still discovers existing
      state from the user's home dir
- [x] `crates/trawl/src/registry.rs` doc comment updated
- [x] HANDOFF.md and README mention the new location
