# The Daily Claude

A public monorepo for The Daily Claude: a session miner CLI (`trawl`), with room for a future static site, backend, and web frontend.

## What's here today

### `crates/trawl`

A CLI for mining Claude Code session logs — the JSONL transcripts in `~/.claude/projects/` — and extracting the moments worth remembering (failures, self-owns, small-scale catastrophes, weird-but-correct behaviour, accidental insights). Two-stage pipeline:

1. **Extractor** — one `claude -p --model sonnet` call per session. Reads the JSONL, identifies candidate moments, returns structured drafts.
2. **Tokeniser** — one `claude -p --model haiku` call per draft. Anonymises, scores, tags, and tokenises placeholder entities.

Backed by a deterministic safety net: `registry.rs` stores length-aware SHA-256 hashes of every flagged literal (plaintext never on disk), and a streaming SHA-256 implementation keeps the hot loop zero-alloc. Incremental state means re-running on the same sessions is cheap — only new or prompt-changed inputs are reprocessed.

```bash
# Build
cargo build --release -p trawl

# Dry-run against a single session
./target/release/trawl --dry-run ~/.claude/projects/<slug>/<uuid>.jsonl

# Extract from a whole project tree
./target/release/trawl ~/.claude/projects/ --concurrency 3 -o content/entries/
```

See `crates/trawl/README.md` for the full CLI reference and `docs/solutions/design-decisions/` for the architecture rationale.

## License

AGPL-3.0-or-later. Copyright © 2026 La Bande à Bonnot OÜ.

See `LICENSE` for the full text.
