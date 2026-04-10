# Session Handoff

Read this at the start of every session. Update it before context compaction or session end.

**Last updated:** 2026-04-10 EOD (tool-result support widened; Gemini/Codex subset runs recorded)

## Recent

- **Capability audit completed.** The shipped `trawl` core matches the architecture claims that matter: two-stage Claude pipeline, prompt-hash cache invalidation, hashed PII registry, and race-safe create-new file publishing.
- **`cargo test -p trawl`** — now 67/67 green after the provider-model follow-up.
- **`trawl stats ~/.claude/projects`** — exercised against the real session tree: 7,159 session files, 1,729,530,059 bytes.
- **Todo #034 is complete in code.** Follow-up todo `#035` is now resolved; the public README has been synced to the shipped CLI and output contract.
- **Audit caveat:** no fresh end-to-end extraction rerun was performed in this session, because that would require sending private session content back through the model again.
- **Subset smoke test run afterwards.** Build, test, `stats`, and `--dry-run` all worked on a single small session file. Live extraction failed because the underlying Claude CLI was quota-blocked, and `trawl` only surfaced `claude exited with status Some(1)`. Follow-up todo `#036` now tracks better subprocess error surfacing.
- **Provider-qualified model flags landed.** `--extractor-model` / `--tokeniser-model` now accept `provider/model` strings, optional per-stage effort is wired for supported backends, and the freshness cache now invalidates on backend/model/effort changes instead of only prompt hashes.
- **Tool-result quoting policy widened.** The extractor/tokeniser contract, Rust docs, and README now allow `[TOOL_RESULT:<name>]` blocks only as brief supporting context around a stronger human / assistant / thinking beat. Tool output is no longer categorically banned, but it still must not dominate the moment.
- **Gemini subset run completed on two known-interesting sessions.** `gemini/gemini-2.5-pro` as extractor + `gemini/gemini-2.5-flash` as tokeniser yielded 6 entries total. Quality read: 3 clear keepers, 1 decent but incomplete beat, 1 redundant beat, 1 tool-result-heavy miss. Net: promising taste on the strongest moments, but still under-extracting versus the historical baseline.
- **Codex subset run completed on the same two sessions.** `codex/gpt-5.4` as extractor + `codex/gpt-5.3-codex-spark` as tokeniser yielded 7 entries total. Quality read: better recall than Gemini on the first session, but noisier taste overall, 3 weaker/filler extractions, and multiple entries where tool-result material still dominated too much.
- **Codex high-effort subset run completed too.** Raising the extractor to `--extractor-effort high` improved the first session materially (3 real beats instead of medium's 4 with filler), but the second session sprawled back out to 5 entries and stayed noisy. Net: `high` helps precision on cleaner sessions, but does not fix Codex's tendency to over-theorise and over-quote tool-result context on messy debugging sessions.
- **Compounded learnings added for this session.** New docs cover the tool-result quote policy, live backend ranking by subset run, and the rule that behavioral-contract changes need full-surface sync across prompts, runtime docs, README, and handoff notes.
- **One derived-project edge case remains.** Running directly against root Claude session files showed that Rust-side project derivation can still collapse to an unhelpful root-session slug rather than a meaningful repo/project name. It is no longer LLM-generated, but the fallback still needs hardening.

## Repo context

`The-Daily-Claude/the-daily-claude` is the public monorepo. Home of `crates/trawl` today, with room for a future static `site/`, backend, and frontend. AGPL-3.0-or-later. Copyright holder: La Bande à Bonnot OÜ.

Fresh start, no git history preserved. Everything below is the trawl/engineering slice carried forward into this repo.

## What's Built and Working (trawl)

- **Trawl (ZFC)** — `crates/trawl/src/` now includes provider-qualified stage config. `--extractor-model` / `--tokeniser-model` accept `provider/model` strings (`claude-code/...`, `codex/...`, `gemini/...`, `opencode/...`, `pi/...`), with optional per-stage effort for the backends that expose it. Deterministic backstop: `registry.rs` stores SHA-256 hashes + byte-lengths of every flagged literal (plaintext never on disk), and length-aware `find_leaks` probes only the window sizes actually present. Incremental state: `state.rs` hashes file content + both prompts + backend signatures + crate version; any mismatch re-trawls.
- **`project` is now derived in Rust, not by the extractor.** The extractor prompt no longer asks the model for that field; the entry writer sets it from `derive_project_name(session_path)` at write time.
- **Atomic write primitives in `state.rs`** — `sidecar_tmp_path` shared helper; `atomic_write` (rename-based, overwriting) and `atomic_write_exclusive` (link-based, create-new with `Ok(bool)` return for race-safe publish); `TmpFileGuard` RAII cleanup on every exit path; `MAX_SIDECAR_NAME_BYTES=200` with UTF-8-boundary truncation against POSIX `NAME_MAX`.
- **Probe-and-retry entry publisher** — `run_trawl` in `main.rs` uses `next_number` as an advisory lower bound, loops on `atomic_write_exclusive` up to 1024 retries. Safe against concurrent Trawl runs at the syscall level.
- **Read-only CLI path verified during the audit.** `stats` successfully walked the real `~/.claude/projects` tree without model calls or repo writes.

## Historical smoke-test evidence

Earlier PR-round smoke tests remain the current end-to-end extraction evidence:

| Session | Size | Entries extracted | Ground-truth hits |
|---|---|---|---|
| 10c05bb2 | 221K | 3 | #212 exact + 2 new beats |
| aab2e849 | 1.0M | 3 | 3 new beats (old corpus missed them) |
| 3c76563c | 831K | 6 | #225 exact + #301 same-beat + 4 new |

Total 12 entries, 0 failures, ~16 min wall time at concurrency 3. State file skipping and prompt-hash invalidation both validated end-to-end. `.pii-registry.json` grew to 13+ hashes after one run, plaintext-free on disk.

## Verification snapshot (2026-04-09 audit)

- `cargo test -p trawl`: **67/67 green** (60 lib + 7 main)
- Read-only real-tree check: `trawl stats ~/.claude/projects` reported **7,159** session files and **1,729,530,059** bytes
- No fresh private-session extraction rerun was performed in this audit session
- Later subset smoke test: one 5 KB session file built/tested cleanly, `stats` and `--dry-run` succeeded, live extraction failed due current Claude quota and exposed an error-surfacing gap rather than a CLI-contract break
- Provider-model follow-up: mixed-backend `--dry-run` succeeded with `codex/gpt-5.4-codex` as extractor and `gemini/gemini-2.5-flash` as tokeniser; `cargo test -p trawl` is now **67/67 green**
- Live backend follow-up: end-to-end subset runs now exist for both Gemini (`gemini-2.5-pro` / `gemini-2.5-flash`) and Codex (`gpt-5.4` / `gpt-5.3-codex-spark`) on two known-interesting sessions

## What's Next (trawl priority order)

1. **Todo #036 — surface actionable Claude subprocess failures.** Current quota/auth-style failures can collapse to `claude exited with status Some(1)`, which is not good enough for operators.
2. **Todo #033 — `claude -p --bare` flag** for extractor + tokeniser. Up-to-10x SDK startup speedup if compatible with `--add-dir`, `--allowedTools Read`, `--model`, and stdin-piped prompts.
3. **Backend quality tightening beyond the first subset runs.** Gemini and Codex both now have live end-to-end smoke evidence; next step is prompt and policy tuning to reduce Gemini under-extraction and Codex tool-result overreach / filler.
4. **Full-corpus ZFC Trawl run** — `./target/release/trawl ~/.claude/projects/ --concurrency 3`. No longer blocked on `#034`, but do it after `#036` if you want failures to be diagnosable.

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
- `tool-results-can-support-the-joke-but-cannot-be-the-speaker-20260410.md` — tool results may support a beat, but cannot become the main voice of the moment.

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
- `live-subset-backend-evals-beat-theoretical-rankings-20260410.md` — rank providers by archive-worthy keepers on the same interesting sessions, not by theoretical reputation or raw count.
- `behavioral-contract-changes-need-full-surface-sync-20260410.md` — prompt-level policy changes must be synchronized across downstream prompts, runtime docs, README, and handoff notes.
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
