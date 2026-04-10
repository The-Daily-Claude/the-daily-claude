# The Daily Claude

A public monorepo for The Daily Claude: a session miner CLI (`trawl`), with room for a future static site, backend, and web frontend.

## What's here today

### `crates/trawl`

A CLI for mining Claude Code session logs — the JSONL transcripts in `~/.claude/projects/` — and extracting the moments worth remembering (failures, self-owns, small-scale catastrophes, weird-but-correct behaviour, accidental insights). Two-stage pipeline:

1. **Extractor** — one configured CLI call per session. Reads the JSONL, identifies candidate moments, returns structured drafts.
2. **Tokeniser** — one configured CLI call per draft. Tokenises placeholder entities and returns placeholderised output plus review flags.

The stage model flags accept provider-qualified strings such as `claude-code/claude-opus-4-6`, `codex/gpt-5.4-codex`, `gemini/gemini-3.1-pro-preview`, `opencode/kimi-for-coding/k2p5`, or `pi/zai-coding-plan/glm-5.1`. Backed by a deterministic safety net: `registry.rs` stores length-aware SHA-256 hashes of every flagged literal (plaintext never on disk), and a streaming SHA-256 implementation keeps the hot loop zero-alloc. Incremental state means re-running on the same sessions is cheap — only new, prompt-changed, or backend-changed inputs are reprocessed.

Current quality snapshot from small live subset runs: Gemini is the better archive miner today. Codex can surface a few extra beats, but it is noisier, more prone to theory-sprawl, and more likely to overuse tool-result context. See `crates/trawl/README.md` for the fuller backend note.

```bash
# Build
cargo build --release -p trawl

# Dry-run against a single session
./target/release/trawl --dry-run ~/.claude/projects/<slug>/<uuid>.jsonl

# Mixed backend example
./target/release/trawl \
  --extractor-model codex/gpt-5.4-codex \
  --extractor-effort high \
  --tokeniser-model gemini/gemini-2.5-flash \
  --dry-run ~/.claude/projects/<slug>/<uuid>.jsonl

# Extract from a whole project tree
./target/release/trawl ~/.claude/projects/ --concurrency 3 -o content/entries/
```

See `crates/trawl/README.md` for the full CLI reference and `docs/solutions/design-decisions/` for the architecture rationale.

## License

AGPL-3.0-or-later. Copyright © 2026 La Bande à Bonnot OÜ.

See `LICENSE` for the full text.
