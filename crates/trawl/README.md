# trawl

A CLI that mines Claude Code session logs and extracts the moments worth remembering — failures, self-owns, small-scale catastrophes, weird-but-correct behaviour, accidental insights — into anonymised markdown entries.

## What it does

Claude Code writes a JSONL transcript per session under `~/.claude/projects/<slug>/<uuid>.jsonl`. Each line is a message: user prompt, assistant response, tool call, tool result. Over months, a heavy user accumulates thousands of these sessions, most of them forgettable and a few of them memorable. `trawl` reads the forgettable and the memorable all together, asks Claude which moments are worth keeping, and writes the keepers to a directory of markdown files.

Two stages, one `claude -p` call each:

1. **Extractor** (`claude -p --model sonnet`) — reads a session JSONL, identifies candidate moments (rage, comedy, catastrophe, insight, meta), returns a structured list of drafts with verbatim quotes.
2. **Tokeniser** (`claude -p --model haiku`) — reads one draft at a time and returns an anonymised + scored + tagged version. First names become `Alice`/`Bob`. Cities and nationalities come from randomised pools. Credentials, hostnames, and org identifiers are tokenised to opaque placeholders (`[org-1]`, `[vendor]`, `[redacted-token]`). Profanity is scrubbed.

Both stages are pure `claude -p` invocations — no Rust-side classifiers wrapping the model, no regex decision trees. See `docs/solutions/design-decisions/zero-framework-cognition-20260320.md` for why.

## Deterministic safety net

LLM anonymisation is best-effort. A PII registry (`src/registry.rs`) tracks every literal string the tokeniser has ever flagged as sensitive, stored as SHA-256 digests and byte lengths — plaintext is never written to disk. After each new draft the tokeniser returns, the body is scanned against the accumulated registry using length-aware windowing: if any previously-flagged literal appears verbatim in the new draft, the entry is marked `needs_manual_review` and a human catches the leak before publish.

The streaming SHA-256 implementation is zero-alloc in the hot loop (see `docs/solutions/best-practices/streaming-sha256-zero-alloc-hot-loop-20260408.md`) and the length-aware substring scan probes only byte-window sizes the registry has actually seen (see `docs/solutions/best-practices/length-aware-substring-registry-20260408.md`).

## Crash-safe writes

Entries and state files are published via an RAII `TmpFileGuard` + `link(2)`-based `atomic_write_exclusive` primitive in `src/state.rs`. Concurrent trawl runs electing a single winner for each filename, no partial writes on crash, no race between file-existence probes and create-new publish. See `docs/solutions/best-practices/atomic-write-exclusive-link-based-20260408.md`.

## Usage

```bash
# Build
cargo build --release -p trawl

# Dry-run a single session (no writes, shows what would be extracted)
./target/release/trawl --dry-run ~/.claude/projects/<slug>/<uuid>.jsonl

# Extract from a whole project tree, parallel across sessions
./target/release/trawl ~/.claude/projects/ --concurrency 3 -o content/entries/

# Stats for a project tree (counts, sizes, most recent session)
./target/release/trawl --stats ~/.claude/projects/

# Validate existing entries against the accumulated PII registry
./target/release/trawl validate content/entries/
```

See `src/main.rs` for the full CLI surface.

## Configuration

- `anonymize.local.toml` — project-local overrides for entity replacement patterns (real project names, internal hostnames, etc.). **Never committed.** See `anonymize.local.toml.example` for the schema.
- Prompts live at `prompts/extractor.md` and `prompts/tokeniser.md`. They are `include_str!`'d into the binary at compile time, and their SHA-256 hashes are part of the per-session cache key — editing a prompt invalidates state for every previously-trawled session so they get re-processed with the new rules.

## State and registry

v1 stores the PII registry and incremental state under `content/.pii-registry.json` and `content/.trawl-state.json`, gitignored. The target layout (tracked in `todos/023-trawl-state-out-of-repo.md`) is:

```
~/.local/share/com.the-daily-claude.trawl/
  the-stash/entries/
  pii-registry.json
  trawl-state.json
```

…on both Linux (via `$XDG_DATA_HOME`) and macOS (as a literal — deliberately not `~/Library/Application Support/`). Until that lands, the state lives inside the caller's content repo as a gitignored pair.

## Tests

```bash
cargo test -p trawl
```

58 tests at the time of writing. Includes a 32-thread concurrent-writer race on `atomic_write_exclusive` to catch the kind of file-publish bug that made entry-number collisions possible in v1.

## License

AGPL-3.0-or-later. See the repository root `LICENSE` for the full text.
