# trawl

A CLI that mines Claude Code session logs and extracts the moments worth remembering — failures, self-owns, small-scale catastrophes, weird-but-correct behaviour, accidental insights — into anonymised markdown entries.

## What it does

Claude Code writes a JSONL transcript per session under `~/.claude/projects/<slug>/<uuid>.jsonl`. Each line is a message: user prompt, assistant response, tool call, tool result. Over months, a heavy user accumulates thousands of these sessions, most of them forgettable and a few of them memorable. `trawl` reads the forgettable and the memorable all together, asks Claude which moments are worth keeping, and writes the keepers to a directory of markdown files.

Two stages, one configured CLI call each. The default backend is Claude Code, but both stages now accept provider-qualified model strings:

1. **Extractor** — reads a session JSONL, identifies candidate moments (rage, comedy, catastrophe, insight, meta), returns a structured list of drafts with verbatim quotes. Tool results may appear only as short supporting context, never as the dominant voice of the moment.
2. **Tokeniser** — reads one draft at a time and returns placeholderised `title`, `category`, `tags`, and `body`, plus an `entities` graph and review flags. Sensitive spans are replaced with `#TYPE_NNN#` tokens such as `#USER_001#`, `#ORG_001#`, or `#CRED_001#`. The sidecar graph preserves only placeholder relationships, never raw values.

Supported provider prefixes today:

- `claude-code/...` (implemented via the `claude` CLI)
- `codex/...`
- `gemini/...`
- `opencode/...`
- `pi/...`

Everything after the first slash is treated as opaque backend-specific model text. So `opencode/minimax-coding-plan/MiniMax-M2.7` and `pi/zai-coding-plan/glm-5.1` are both valid model strings.

Both stages are still pure CLI invocations — no Rust-side classifiers wrapping the model, no regex decision trees. See `docs/solutions/design-decisions/zero-framework-cognition-20260320.md` for why.

## Deterministic safety net

LLM anonymisation is best-effort. A PII registry (`src/registry.rs`) tracks every literal string the tokeniser has ever flagged as sensitive, stored as SHA-256 digests and byte lengths — plaintext is never written to disk. After each new draft the tokeniser returns, the body is scanned against the accumulated registry using length-aware windowing: if any previously-flagged literal appears verbatim in the new draft, the entry is marked `needs_manual_review` and a human catches the leak before publish.

The streaming SHA-256 implementation is zero-alloc in the hot loop (see `docs/solutions/best-practices/streaming-sha256-zero-alloc-hot-loop-20260408.md`) and the length-aware substring scan probes only byte-window sizes the registry has actually seen (see `docs/solutions/best-practices/length-aware-substring-registry-20260408.md`).

The intended boundary is that no PII or confidential information should leak into the extracted data, model interpretations, or generated reports. That boundary matters, but it is not "solved" forever — leakage resistance is still an area to improve and harden.

## Crash-safe writes

Entries and state files are published via an RAII `TmpFileGuard` + `link(2)`-based `atomic_write_exclusive` primitive in `src/state.rs`. Concurrent trawl runs electing a single winner for each filename, no partial writes on crash, no race between file-existence probes and create-new publish. See `docs/solutions/best-practices/atomic-write-exclusive-link-based-20260408.md`.

## Usage

```bash
# Build
cargo build --release -p trawl

# Dry-run a single session (no writes, no model calls; prints which sessions would be trawled)
./target/release/trawl --dry-run ~/.claude/projects/<slug>/<uuid>.jsonl

# Use a provider-qualified extractor model and a fast Gemini tokeniser
./target/release/trawl \
  --extractor-model codex/gpt-5.4-codex \
  --extractor-effort high \
  --tokeniser-model gemini/gemini-2.5-flash \
  --dry-run ~/.claude/projects/<slug>/<uuid>.jsonl

# Use OpenCode with provider-specific reasoning effort mapping
./target/release/trawl \
  --extractor-model opencode/kimi-for-coding/k2p5 \
  --extractor-effort medium \
  --tokeniser-model opencode/minimax-coding-plan/MiniMax-M2.7 \
  --tokeniser-effort min \
  ~/.claude/projects/<slug>/<uuid>.jsonl -o content/entries/

# Extract from a whole project tree, parallel across sessions
./target/release/trawl ~/.claude/projects/ --concurrency 3 -o content/entries/

# Stats for a project tree (currently total file count and total bytes)
./target/release/trawl stats ~/.claude/projects/

# Validate existing entries against the accumulated PII registry
./target/release/trawl validate content/entries/
```

See `src/main.rs` for the full CLI surface.

## Model selection

`--extractor-model` and `--tokeniser-model` now take `provider/model` strings.

Examples:

- `claude-code/claude-opus-4-6`
- `claude-code/claude-sonnet-4-6`
- `codex/gpt-5.4-codex`
- `gemini/gemini-3.1-pro-preview`
- `gemini/gemini-2.5-flash`
- `opencode/kimi-for-coding/k2p5`
- `opencode/minimax-coding-plan/MiniMax-M2.7`
- `pi/zai-coding-plan/glm-5.1`

Backward compatibility: bare names like `sonnet` and `haiku` are still accepted and treated as `claude-code/sonnet` and `claude-code/haiku`.

`--extractor-effort` / `--tokeniser-effort` accept `min`, `medium`, or `high`.

Current backend mapping:

- `claude-code`: maps to `claude --effort low|medium|high`
- `codex`: maps to `codex exec -c 'model_reasoning_effort="minimal|medium|high"'`
- `opencode`: maps to `opencode run --variant minimal|medium|high`
- `gemini`: no effort flag today; specifying effort fails early
- `pi`: no effort mapping wired today

Before a run starts, `trawl` uses `which` to confirm the required backend CLI exists on `PATH` and fails fast if it does not.

## Backend quality snapshot

The multi-backend flags are real, but the backends are not equivalent in
editorial quality yet.

From small live subset runs on known-interesting sessions:

- **Gemini** (`gemini-2.5-pro` extractor + `gemini-2.5-flash` tokeniser)
  had the best keep-rate. It under-extracted versus the historical
  baseline, but the strongest hits were genuinely good and required the
  least triage.
- **Codex** (`gpt-5.4` extractor + `gpt-5.3-codex-spark` tokeniser)
  had higher recall on some sessions, but it was noisier overall. It
  over-theorised more often, produced more filler, and was more likely to
  let tool-result context dominate a moment.
- Raising Codex extractor effort from `medium` to `high` improved one
  cleaner session, but did not fix the noisier debugging-session failure
  mode.

So the current practical ranking is:

1. Claude Code remains the intended default.
2. Gemini is the strongest non-Claude archive miner today.
3. Codex is promising, but still needs tighter prompt discipline before
   it is a better default than Gemini for `trawl`.

## What lands on disk

Each extracted entry is a markdown file with YAML frontmatter plus a tokenised body.

Current entry fields on disk include:

- `id`
- `title`
- `project`
- `category`
- `source_type`
- `session_id`
- `extracted_at`
- `needs_manual_review`
- `review_reason`
- `tags`
- `entities`
- body text using `#TYPE_NNN#` placeholders

Quoted body text may include `[TOOL_RESULT:<name>]` blocks when they are
load-bearing context for the joke, but they are expected to stay brief
and subordinate to the surrounding human / assistant / thinking beat.

What does **not** currently land on disk:

- no score fields
- no friendly fake-name substitution layer inside `trawl`
- no persisted raw PII values in the entry or the registry

## Configuration

- Prompts live at `prompts/extractor.md` and `prompts/tokeniser.md`. They are `include_str!`'d into the binary at compile time, and their SHA-256 hashes are part of the per-session cache key.
- The per-session cache key also includes the extractor backend/model/effort signature and the tokeniser backend/model/effort signature. Changing either stage's model config invalidates freshness for previously-trawled sessions.

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

67 tests at the time of writing. Includes a 32-thread concurrent-writer race on `atomic_write_exclusive` to catch the kind of file-publish bug that made entry-number collisions possible in v1.

## License

AGPL-3.0-or-later. See the repository root `LICENSE` for the full text.
