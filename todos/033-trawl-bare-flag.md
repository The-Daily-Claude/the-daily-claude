---
title: "Use --bare flag for claude -p calls in Trawl"
priority: high
status: pending
---

# Wire `claude -p --bare` into the Trawl extractor + tokeniser stages

The Claude SDK supports a `--bare` flag that speeds up SDK startup by **up to 10x** by skipping the agent harness initialization. Trawl spawns one `claude -p --model sonnet` per session (extractor) and one `claude -p --model haiku` per draft (tokeniser). At concurrency 3 across the full corpus, SDK startup overhead dominates wall time — a 10x reduction would materially shrink the first full-corpus run.

## Why this matters

- The PR #3 smoke test took ~16 min wall time for 3 sessions / 12 entries. The full corpus is ~250+ sessions. Linear extrapolation: hours, dominated by per-call startup, not model inference.
- `--bare` is a startup optimization, not a behavior change. Risk is limited to "did it strip a flag we depend on."

## Verify before wiring (smoke test it first)

- [ ] Confirm `--bare` is compatible with `--add-dir` (the extractor relies on this to read session jsonl files outside the cwd)
- [ ] Confirm `--bare` is compatible with `--allowedTools Read` (the extractor uses this to read jsonl via the Read tool)
- [ ] Confirm `--bare` doesn't strip `--model sonnet` / `--model haiku` selection
- [ ] Confirm `--bare` doesn't change structured-output behavior the tokeniser depends on (the tokeniser parses JSON from stdout)
- [ ] Confirm `--bare` supports stdin pipe for the prompt — Trawl uses `claude -p ... < prompt.txt` because variadic flags ate the positional prompt (see `docs/solutions/best-practices/variadic-cli-flag-eats-positional-stdin-fix-20260408.md`)
- [ ] Run `claude -p --bare --model haiku --help` and `claude -p --bare --help` to read the actual flag interactions before integrating

## Wire it in

- [ ] `crates/trawl/src/extractor.rs` — add `--bare` to the `claude -p --model sonnet` invocation
- [ ] `crates/trawl/src/tokeniser.rs` — add `--bare` to the `claude -p --model haiku` invocation
- [ ] Re-run smoke test against the three sessions from PR #3 (`10c05bb2`, `aab2e849`, `3c76563c`) and compare wall time vs the previous ~16 min baseline
- [ ] If `--bare` becomes part of the per-call command line that contributes to the prompt-hash cache key, the next state-file invalidation will trigger a full re-trawl on the corpus. Decide whether to bump the cache version intentionally or to keep `--bare` out of the hash so existing state stays valid.

## Risk assessment

**Low.** `--bare` is documented as a startup optimization. The smoke test against three known-good sessions will catch any behavior regression before the full-corpus run. Worst case: `--bare` is incompatible with one of the flags we need; we don't ship it and lose nothing.

## Out of scope

- Switching extractor/tokeniser to a non-Claude backend (tracked in todo #032).
- Persistent process pool / SDK reuse across calls — `--bare` is the lowest-friction win; pooling is a much larger refactor.
