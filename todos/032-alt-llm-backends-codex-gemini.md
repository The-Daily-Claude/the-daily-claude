---
title: "Alternative LLM backends: Codex CLI and Gemini CLI"
priority: medium
status: pending
depends_on: 022
---

# Alternative LLM Backends for the Trawl Pipeline

## Why

Trawl currently shells out to `claude -p --model sonnet` for the
extractor stage and `claude -p --model haiku` for the tokeniser stage.
Both stages burn Claude quota on the same personal plan the rest of the
editorial workflow uses. A full corpus run against
`~/.claude/projects/` can process hundreds of sessions at concurrency
3+ and eat a meaningful chunk of the month's budget.

Two other full-agent CLIs already live on the box and share the same
general capability envelope:

- **Codex CLI** (OpenAI `gpt-5.4`, configurable `reasoning_effort`
  from `minimal` to `xhigh`). Strong structured output, supports
  `--output-schema` for JSON-shaped responses — which is exactly what
  both the extractor and the tokeniser need.
- **Gemini CLI** (`gemini-3.1-pro-preview`, `gemini-2.5-pro`,
  `gemini-2.5-flash`). Flash is the obvious Haiku analogue for the
  tokeniser; Pro covers the extractor when heavy reasoning is needed.

The Claude Code skill descriptions for both already flag them as
"full coding agents" with file read/write and shell access. We are
only using them as one-shot JSON producers, so none of that matters —
we just need stdin → JSON stdout with a shared prompt.

## What

Let `trawl` select the LLM backend per stage at the CLI level. Current
flags: `--extractor-model` (default `sonnet`), `--tokeniser-model`
(default `haiku`), both consumed by `process_session` and passed
through to the `claude` subprocess.

Desired:
- `--extractor-backend {claude,codex,gemini}` (default `claude`)
- `--tokeniser-backend {claude,codex,gemini}` (default `claude`)
- Existing `--extractor-model` / `--tokeniser-model` keep their
  current string shape; each backend parses them in its own namespace
  (`sonnet` / `haiku` for claude, `gpt-5.4` etc for codex, model id
  for gemini).
- Env vars `TRAWL_EXTRACTOR_BACKEND` and `TRAWL_TOKENISER_BACKEND`
  mirror the flags so CI / scheduled runs don't need to rebuild the
  command line.

## How

The extractor and tokeniser modules currently call into a shared
`claude_subprocess` helper in `extractor.rs` that does:
`claude -p --model <model> --add-dir <dir> < prompt.md`.

Refactor that into a `LlmBackend` trait (enum dispatch is fine, no
need for `dyn`) with three variants:

```rust
enum LlmBackend {
    Claude { model: String },
    Codex { model: String, reasoning_effort: String },
    Gemini { model: String, sandbox: bool },
}

impl LlmBackend {
    fn run(&self, prompt: &str, extra_context_dir: Option<&Path>) -> Result<String> {
        match self {
            Self::Claude { .. }  => /* existing claude subprocess call */,
            Self::Codex { .. }   => /* codex exec --model ... */,
            Self::Gemini { .. }  => /* gemini -m ... */,
        }
    }
}
```

Both `extractor::process_session` and `tokeniser::tokenise_draft`
then take a `&LlmBackend` instead of a `model: &str`. Prompts stay
shared — the existing `include_str!` files in `crates/trawl/prompts/`
work byte-for-byte for all three backends.

### Per-backend quirks to handle

**Codex CLI**
- Command shape: `codex exec --model <model> --reasoning-effort <level> --sandbox none` or similar. Verify against the current `codex-cli` skill docs.
- Supports `--output-schema <json-schema-path>` which would lock the extractor output to a `Vec<DraftEntry>` shape and kill a class of parse errors. Ship that opt-in once the basic backend works.
- Stdin pipe vs positional `<prompt>`: decide once and codify — the Claude subprocess already had the variadic-flag-eats-stdin trap
  (`docs/solutions/best-practices/variadic-cli-flag-eats-positional-stdin-fix-20260408.md`).

**Gemini CLI**
- Command shape: `gemini -m <model> -p <prompt>` with stdin support. `-s false` to disable sandbox when we need to read session jsonl files.
- Flash is the obvious Haiku swap for the tokeniser, ~cheap and fast. Pro is the Sonnet swap for the extractor.
- JSON mode is less mature than Codex's `--output-schema`. Expect to keep the balanced-bracket parser doing the heavy lifting.

**Claude (no change)**
- Existing code path stays as `LlmBackend::Claude`. Zero regression risk.

### Prompt-hash cache invalidation

`state.rs` currently hashes `EXTRACTOR_PROMPT` and `TOKENISER_PROMPT`
bytes to decide whether a session needs a rerun. With multiple
backends the cache key needs to include the backend identity too:
the same prompt fed to Sonnet and to GPT-5 produces different
outputs, and we don't want a `--extractor-backend gemini` run to
skip sessions that were freshly trawled by `claude`.

Add `backend_signature` to `SessionRecord`, hash
`(backend_name, model, prompt)` into a single SHA, compare on
`is_fresh`. Any mismatch re-trawls.

### Per-backend smoke tests

Once each backend compiles and produces a valid `Vec<DraftEntry>`
on the canonical smoke-test session (`crates/trawl/tests/fixtures/…`,
create one if it doesn't exist), it counts as landed. Do **not**
require semantic parity with Claude output — these are diverse
models, they will disagree. The tokeniser + registry pipeline is
designed to handle that variance.

## Scope

- `crates/trawl/src/extractor.rs` — refactor subprocess helper into
  `LlmBackend`.
- `crates/trawl/src/tokeniser.rs` — same.
- `crates/trawl/src/main.rs` — new CLI flags + env var plumbing.
- `crates/trawl/src/state.rs` — `SessionRecord::backend_signature`.
- New module `crates/trawl/src/llm_backend.rs` housing the enum and
  its `run` impls.
- Tests: one per backend, gated behind `#[cfg_attr(not(feature =
  "external-llm-smoke"), ignore)]` so CI doesn't need the CLIs
  installed.

## Out of Scope

- Streaming output. All three CLIs can stream, but trawl needs the
  full JSON blob anyway and streaming complicates error handling.
- Rate limiting or budget tracking. The user already watches their
  own quota.
- Pricing-aware backend auto-selection. Pick a backend, run the job,
  inspect the bill. No meta-reasoning about what to pick.
- OpenCode CLI. Different set of models (GLM, Minimax, Kimi,
  Perplexity Sonar) worth a separate todo if any of them prove
  useful for the editorial workflow — but out of scope here.

## Acceptance

- `trawl --extractor-backend codex --tokeniser-backend claude <path>`
  runs end-to-end on the smoke-test session and produces a valid
  entry file.
- `trawl --extractor-backend gemini --tokeniser-backend gemini <path>`
  same, using Gemini Pro + Flash.
- Claude-only path is unchanged: same CLI, same defaults, same state
  file format (modulo the new `backend_signature` field, which is
  `#[serde(default)]` so old state files still load).
- `cargo test -p trawl` passes without the CLIs installed (external
  smoke tests are `#[ignore]`d by default).
- Compound learning doc: when the first non-Claude run lands, capture
  any per-backend prompt quirks under
  `docs/solutions/best-practices/`.
