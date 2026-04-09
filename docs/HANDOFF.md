# Session Handoff

Read this at the start of every session. Update it before context compaction or session end.

**Last updated:** 2026-04-09 EOD (initial commit `339728b`, plus `5366bec` adding `bikeshedding` category to the extractor prompt)

## Recent

- **`5366bec` — `bikeshedding` category added to `crates/trawl/prompts/extractor.md`.** New top-level category in the enum alongside `rage | comedy | existential | spectacular-failure | wholesome | role-reversal | dark-comedy | meta | other`, plus a "What counts as a moment" paragraph describing the shape: *long exchange about the 'right way' to do something trivial or unsanctioned; the joke is the gap between effort invested and value of what's being discussed.* Seeded by a real beat — a bot-review loop bikeshedding `--force` atomicity on a flag the product owner didn't know existed. Prompt-hash change will invalidate the trawl state cache on the next run, which is intentional — previously-processed sessions get another pass with the new category available.
- **`AGENTS.md`** — committed as a contributor guide for agents that look for `AGENTS.md` instead of `CLAUDE.md` (Codex, OpenCode, etc.). Mirror of `CLAUDE.md` with the header adjusted.

## Repo context

`The-Daily-Claude/the-daily-claude` is the public monorepo. Home of `crates/trawl` today, with room for a future static `site/`, backend, and frontend. AGPL-3.0-or-later. Copyright holder: La Bande à Bonnot OÜ.

Fresh start, no git history preserved. Everything below is the trawl/engineering slice carried forward into this repo.

## What's Built and Working (trawl)

- **Trawl (ZFC)** — `crates/trawl/src/` is ~1400 lines including tests. Two stages: `extractor.rs` (one `claude -p --model sonnet` per session, reads the jsonl via `--add-dir`), `tokeniser.rs` (one `claude -p --model haiku` per draft, returns tokenised title/category/tags/body + sidecar entity graph). Deterministic backstop: `registry.rs` stores SHA-256 hashes + byte-lengths of every flagged literal (plaintext never on disk), length-aware `find_leaks` probes only the window sizes actually present. Incremental state: `state.rs` hashes file content + both prompts + crate version; any mismatch re-trawls.
- **Atomic write primitives in `state.rs`** — `sidecar_tmp_path` shared helper; `atomic_write` (rename-based, overwriting) and `atomic_write_exclusive` (link-based, create-new with `Ok(bool)` return for race-safe publish); `TmpFileGuard` RAII cleanup on every exit path; `MAX_SIDECAR_NAME_BYTES=200` with UTF-8-boundary truncation against POSIX NAME_MAX. Both helpers are the entire surface for crash-safe file writes across State, Registry, and the entry writer.
- **Probe-and-retry entry publisher** — `run_trawl` in `main.rs` uses `next_number` as an advisory lower bound, loops on `atomic_write_exclusive` up to 1024 retries. Safe against concurrent Trawl runs at the syscall level.

## Smoke Test Results (during an earlier PR round)

Ran ZFC Trawl against three real sessions whose ground-truth entries already existed in an offline corpus used only for regression scoring:

| Session | Size | Entries extracted | Ground-truth hits |
|---|---|---|---|
| 10c05bb2 | 221K | 3 | #212 exact + 2 new beats |
| aab2e849 | 1.0M | 3 | 3 new beats (old corpus missed them) |
| 3c76563c | 831K | 6 | #225 exact + #301 same-beat + 4 new |

Total 12 entries, 0 failures, ~16 min wall time at concurrency 3. State file skipping and prompt-hash invalidation both validated end-to-end. `.pii-registry.json` grew to 13+ hashes after one run, plaintext-free on disk.

## Test counts as of the fresh-start commit

- `cargo test -p trawl`: **58/58 green** (51 lib + 7 main)
- Notable race/boundary tests: `atomic_write_exclusive_publishes_when_target_missing`, `atomic_write_exclusive_refuses_to_overwrite`, `atomic_write_exclusive_parallel_writers_elect_single_winner` (32-thread race), `sidecar_tmp_path_truncates_long_source_names`, `sidecar_tmp_path_truncates_on_utf8_char_boundary`, `atomic_write_handles_near_namemax_filenames`.

## What's Next (trawl priority order)

1. **Todo #034 — extractor deterministic path handling.** Remove `project:` from the LLM response schema, compute it in Rust from the session file path basename, inject post-call. Minimal scope — just the one field. The LLM continues to read the session file normally via `--add-dir`. This unblocks a clean full-corpus trawl run (otherwise the `project: Users-alice` bug reappears on any new entries). See the `llm-cognition-code-transformation-20260409.md` compound learning for the principle.
2. **Todo #033 — `claude -p --bare` flag** for extractor + tokeniser. Up-to-10x SDK startup speedup. Verify `--bare` compatibility with `--add-dir`, `--allowedTools Read`, `--model`, and the stdin-piped prompt pattern BEFORE wiring it in. Re-run the smoke test sessions to compare wall time.
3. **Full-corpus ZFC Trawl run** — `./target/release/trawl ~/.claude/projects/ --concurrency 3`. First real production run. Gated on #034 (so new entries don't reintroduce `project: Users-alice`) and ideally #033 (so wall time is tolerable). Watch `.pii-registry.json` grow; run `trawl validate <entries-dir>` after to catch any leaks against the cumulative registry.
4. **Todo #032 — Codex/Gemini alt backends** if Claude quota becomes a constraint during the full-corpus run. `LlmBackend` enum sketch, per-stage CLI flags, cache-key change for `SessionRecord` (prompt+backend hash, not just prompt hash), per-backend smoke tests gated behind a feature flag. Design-only follow-up; complementary to #033.

## Compound Learnings Index (trawl/engineering subset)

**Design — `docs/solutions/design-decisions/`:**
- `zero-framework-cognition-20260320.md` — the foundational principle: model-as-framework, not framework-around-model.
- `llm-cognition-code-transformation-20260409.md` — **LLM for cognition, code for deterministic transformation.** The inverse boundary of ZFC. The `project: Users-[user]` extractor bug case study. If you're writing prompt instructions like "format this as a slug" — delete them and write a function.
- `entry-id-uuid7-vs-filename-integer-20260409.md` — the entry `id:` frontmatter field is a UUID7, not the filename integer. Schema nuance that could cause silent data corruption during collision fixes.
- `zfc-anonymization-20260406.md` — ZFC direction for anonymization — trust the model for judgment, keep a deterministic backstop.
- `two-stage-zfc-pipeline-in-practice-20260408.md` — the prompt-tuning journey (1→0→3 entries per session), prompts-as-code.
- `tokeniser-pii-boundary-whole-draft-20260408.md` — tokeniser must see every user-visible field, placeholder coreference is load-bearing.
- `prompt-hash-as-cache-invalidation-20260408.md` — `include_str!` + SHA-256 is the version, no manual bump.
- `temperature-variance-acceptance-20260408.md` — accept LLM diversity, design the downstream consumer to handle it.

**Best practices — `docs/solutions/best-practices/`:**
- `atomic-write-exclusive-link-based-20260408.md` — `link(2)` for POSIX-atomic create-new publish; `Ok(bool)` return for race-safe exactly-one-writer; `TmpFileGuard` RAII cleanup; `sidecar_tmp_path` shared helper; `MAX_SIDECAR_NAME_BYTES=200` with UTF-8-boundary truncation.
- `atomic-write-monotonic-tempfile-suffix-20260408.md` — `AtomicU64` nonce in tempfile name for thread-safe concurrent writes (extended by the link-based doc above).
- `streaming-sha256-zero-alloc-hot-loop-20260408.md` — pure-Rust SHA-256 with stack-only padding, chunk-exact block processing.
- `length-aware-substring-registry-20260408.md` — `BTreeSet<usize>` of literal lengths closes correctness hole + ~15× perf win.
- `balanced-bracket-json-candidate-scan-20260408.md` — depth-counting string-aware scanner that walks every candidate, beats `find`/`rfind` heuristics.
- `variadic-cli-flag-eats-positional-stdin-fix-20260408.md` — `claude -p --allowedTools Read <prompt>` fails, stdin pipe is the fix.
- `error-chains-must-not-leak-pii-20260408.md` — never format raw model output into anyhow contexts; log byte ranges only.
- `rust-2024-raw-string-hash-collision-20260408.md` — `r#"...#FOO#..."#` fails, use `r##"..."##`.
- `tolerant-per-unit-errors-multi-stage-pipeline-20260408.md` — `match`-not-`?` on per-draft failures in orchestrated LLM calls.
- `historical-output-as-regression-dataset-20260408.md` — your own high-quality entries are the cheapest ground truth.
- `trawl-quality-multi-layer-filtering-20260401.md` — older trawl quality architecture, superseded by the ZFC refactor but kept for context.

**Process — `docs/solutions/process-patterns/`:**
- `pii-sweeps-span-whole-repo-20260409.md` — **PII sweeps span the whole repo, not just one directory.** A sweep isn't done until a whole-repo grep for the leak patterns comes back empty.
- `review-loop-discipline-20260408.md` — rejecting vague findings, self-scrutinising your own fixes, rebase-stale comments, scoped `git add`.
- `bot-review-loop-gh-api-mechanics-20260408.md` — executable `gh api --paginate` + jq + `/replies` batch-reply snippets.
- `pr-split-for-bot-review-caps-20260408.md` — split data PRs from code PRs when bot reviewers (CodeRabbit free tier: 150 files) can't review bundled work.
- `unpushed-main-phantom-diff-20260408.md` — unpushed local main commits become phantom PR diffs because GitHub computes diffs against `origin/main`, not local.
- `do-less-ritual-more-work-20260408.md` — the SLFG/ralph-loop misfire recognition pattern.
- `bot-reviewers-hallucinate-cli-flags-20260406.md` — bots confidently cite flags that don't exist.
- `bot-pr-review-cadence-and-synthesis-20260406.md` — bot-review cadence, superseded in part by the loop mechanics docs above.
- `simplicity-over-architecture-20260320.md` — ship the simple version first.
