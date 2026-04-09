---
title: "Trawl ZFC Redesign — Thin Orchestrator + Sonnet/Session + Haiku/Tokeniser"
type: refactor
status: active
date: 2026-04-07
supersedes: [todos/017, todos/020, todos/021]
reshapes: [todos/018, todos/019]
---

## Overview

Rewrite `crates/trawl/` as a thin ~250-line orchestrator around two LLM
calls per session: a Sonnet extractor that reads the raw JSONL and returns
draft entries, and a Haiku tokeniser that replaces every PII span with a
`#TYPE_NNN#` placeholder and emits a sidecar entity graph. Delete the
regex anonymizer, sliding windows, 8-dimension scoring rubric, role enum,
operational blacklist, overlap dedup, and per-session caps. Keep
concurrency, state file, and the entry writer.

## Problem Frame

The 2026-04-07 Sonnet audit over 6 sessions / 19 entries showed Trawl at
**63% recall / 63% precision**. Every miss and every false positive traced
back to framework cognition Trawl had hard-coded: sliding windows, role
labels, tool-result skips, `narrative_completeness` thresholds, overlap
heuristics, regex anonymisation, per-session caps. A single Sonnet pass
given the raw JSONL plus a one-paragraph brief found every miss and
rejected every false positive.

This plan translates that finding into code: delete the scaffolding,
let the model be the framework, and add a relational-graph tokeniser so
PII stays off disk while downstream consumers of the extracted entries
retain the relationships needed for coherent per-render substitution.

## Requirements Trace

- ZFC principle — `docs/solutions/design-decisions/zero-framework-cognition-20260320.md`
- ZFC anonymisation precedent — `docs/solutions/design-decisions/zfc-anonymization-20260406.md`
- Incremental state (reshaped) — `todos/018-trawl-incremental-state.md`
- Anonymisation hardening (reshaped) — `todos/019-anonymization-hardening.md`
- Batch scoring (moot) — `todos/017-trawl-batch-scoring.md`
- Tool-result mislabeling (moot) — `todos/020-trawl-tool-result-mislabeling.md`
- Overlap dedup (moot) — `todos/021-trawl-overlap-dedup-too-loose.md`
- Verbatim-quotes invariant — `docs/solutions/content-pipeline/verbatim-quotes-not-summaries-20260320.md`

## Scope Boundaries

**In scope**
- Full replacement of `crates/trawl/src/` extraction + anonymisation layers
- New prompts under `crates/trawl/prompts/`
- State file `content/.trawl-state.json` (hash-based skip only)
- PII registry `content/.pii-registry.json` (grow + validate; gitignored)
- `trawl validate` subcommand
- `.gitignore` update
- Smoke test against the 6 audited sessions

**Out of scope**
- Any downstream consumer of the extracted entries (documented only as an implicit contract on the `entities` frontmatter field)
- Chunking strategy for multi-MB sessions (v2)
- Batch Haiku calls (deferred unless wall-time dominates)
- Append-mode re-trawl (rejected — full re-trawl on change)
- Persistent `placeholder → real_value` map (explicitly rejected)

## Context & Research

### Relevant code (current state, to be removed unless noted)

- `crates/trawl/src/main.rs` — CLI entry, flag parsing, pool. **Keep** pool, rewrite main loop.
- `crates/trawl/src/session.rs` — `parse_session`, `Role` enum, `create_sliding_windows`, `window_is_worth_scoring`, `format_exchange`, `dedup_overlapping_windows`. **Delete whole module.**
- `crates/trawl/src/score.rs` — `score_exchange`, 8-dim rubric, `worth_extracting`, `--dump-scores`. **Delete whole module.**
- `crates/trawl/src/anonymize.rs` — regex machinery, Alice/Bob assignment, `needs_review` heuristic, `anonymize.local.toml` loader. **Delete whole module.**
- `crates/trawl/src/entry.rs` (if split) or inline — `Entry`, `Source`, frontmatter writer. **Keep, simplify** (drop `Scores`).
- Existing entry shape — `content/entries/*.md` YAML frontmatter: `id`, `title`, `project`, `category`, `tags`, `source`, `source_type`, `human_alias`, `needs_manual_review`. Add `entities` map.

### Institutional learnings

- `docs/solutions/design-decisions/zero-framework-cognition-20260320.md` — "The model IS the framework."
- `docs/solutions/design-decisions/zfc-anonymization-20260406.md` — precedent for letting the LLM make the anonymisation call instead of regex.
- `docs/solutions/content-pipeline/verbatim-quotes-not-summaries-20260320.md` — downstream renders need real words; extractor prompt must forbid paraphrasing.
- HEAT orchestrator JSON-fragility lessons — robust bracket extraction + one retry.

### External refs

- Claude Code CLI `claude -p --model <sonnet|haiku> --add-dir <path> --allowedTools "Read"` — subprocess contract.
- Conventional Commits for the migration PR series.

## Key Technical Decisions

| Decision | Rationale |
|---|---|
| **Two stages, no Stage 3** | Per-post substitution is more flexible; the same `#CITY_001#` can become Tokyo on Tuesday and Berlin on Wednesday. No persistent PII map on disk. |
| **Sonnet reads JSONL directly via Read tool** | Deletes `parse_session`, role enum, tool_result skip logic, thinking-block handling. The model handles role semantics implicitly. |
| **One Haiku call per draft entry (not batch)** | Earlier batch attempts showed cross-entry identity mixing. Subprocess cost is acceptable until measured otherwise. |
| **No global PII → value map** | The `{placeholder → real_value}` dict lives only in memory during the Haiku call, then evaporates. Eliminates an entire class of leak. |
| **Entity graph in frontmatter** | Downstream consumers of the extracted entries need hierarchical context (`#CITY_001#` is in `#COUNTRY_002#`) to substitute coherently. Only the *types and links* ship on disk. |
| **PII registry: grow + validate, no resolve** | Registry only answers "did any literal value from any past entry leak into this new body?" It never resolves placeholders. |
| **Session size ceiling: fail loudly on overflow (v1)** | Option (a) from the todo. Measure before adding chunking complexity. |
| **Full re-trawl on any change** | Simpler than append-mode. Trust Sonnet + state-file hash gating. |
| **In-pipeline tokenisation (not a sweep)** | `tokeniser_prompt_sha256` in the state file already triggers re-trawl when the prompt drifts. `trawl retokenise` can be added later. |
| **`--allowedTools "Read"` for Sonnet** | Minimal blast radius; expand only if smoke test fails. |

## Open Questions

### Resolved (in this plan)

1. **Session size ceiling** → fail loudly on overflow for v1; revisit only if it fires frequently in Phase 7.
2. **JSON output reliability** → robust `[`..`]` extraction + one retry with stricter instruction + write raw to debug file and skip on second failure.
3. **Haiku per entry vs batch** → per entry; reconsider only if wall time dominates.
4. **Idempotent retokenisation sweep** → in-pipeline only for v1; `trawl retokenise` deferred.
5. **`--allowedTools` for extractor** → start with `Read` only.

### Deferred

6. **Cost / rate limits on full corpus** → unknown until Phase 7 runs. Mitigation: incremental state permits daily batching.
7. **Downstream contract for new `entities` frontmatter** → documented later as an informal contract for any consumer of the extracted entries. Not blocking Trawl delivery.

## High-Level Technical Design

```
┌──────────────────────────────────────────────────────────────┐
│ Trawl (Rust, ~250 lines)                                     │
│                                                              │
│   1. walk ~/.claude/projects/                                │
│   2. consult state file: which sessions need trawling?      │
│   3. load PII registry (content/.pii-registry.json)         │
│   4. work-stealing pool of N workers                         │
│        each worker:                                          │
│          a. claude -p --model sonnet  ←── Stage 1            │
│             "<EXTRACTOR_PROMPT>"                             │
│             --add-dir <session.jsonl path>                   │
│             returns: [{title, category, tags, quote, why},…] │
│                                                              │
│          b. for each draft entry:                            │
│               claude -p --model haiku  ←── Stage 2           │
│               "<TOKENISER_PROMPT>"                           │
│               draft → tokenised body + entity graph +        │
│                       transient {placeholder → real_value}   │
│                                                              │
│          c. registry.grow(transient_map)   ←── grow phase    │
│          d. registry.validate(tokenised_body) ← validate     │
│             if any literal PII matches → needs_manual_review │
│                                                              │
│          e. write tokenised entry to content/entries         │
│             (placeholders ship as-is; downstream consumers   │
│              pick friendly substitutes at render time)       │
│          f. update state file                                │
└──────────────────────────────────────────────────────────────┘
```

### Prompt sketches (directional — full text lives in `crates/trawl/prompts/`)

**Extractor (Sonnet).** Editorial brief; read the JSONL at the absolute
path; find every postable moment; quote verbatim with `[HUMAN]:` /
`[ASSISTANT]:` / `[TOOL_RESULT]:` blocks; return a JSON array of
`{title, category, tags, quote, why}`; empty array if nothing. BE HARSH.
Output ONLY the JSON array.

**Tokeniser (Haiku).** Replace every PII span with `#TYPE_NNN#`. Diarise
aliases of the same entity to the same placeholder. Capture only
relationships that matter for the joke. Be aggressive on token-shaped
strings (`dp.sa.*`, `sk-*`, `AKIA*`, `eyJ*`, base64 runs). Output
`{body, entities, needs_review, review_reason}`. Flag uncertainty.

These prompts are **directional**; Phase 0 locks their exact wording by
hand-iteration against the 6 audited sessions.

## Implementation Units

### Phase 0 — Prompt lock-in  [x]

- **Goal:** Freeze `extractor.md` and `tokeniser.md` before any Rust work.
- **Requirements:** Match Sonnet audit quality on 3–5 known sessions.
- **Dependencies:** None.
- **Files:** `crates/trawl/prompts/extractor.md`, `crates/trawl/prompts/tokeniser.md`.
- **Approach:** Hand-invoke `claude -p --model sonnet` with the prompt and a session path. Compare to audit ground truth. Iterate prose. Repeat for Haiku tokeniser on 5 hand-picked drafts containing names, cities, credentials, paths.
- **Patterns:** ZFC — push decisions into prose, not code. Verbatim-quotes invariant.
- **Test scenarios:**
  - Happy path: session with 3 known funny moments → extractor returns exactly those 3.
  - Edge case: session with only operational chatter → returns `[]`.
  - Edge case: long monologue where the joke lives in the assistant's thinking block → extracted.
  - Error path: tokeniser given a draft with a Doppler token `dp.sa.xxx` → replaced with `#CRED_001#` and `needs_review: true`.
- **Verification:** Manual diff against audit notes; ≥90% of known moments recovered.

### Phase 1 — Delete framework-cognition modules

- **Goal:** Remove every module the ZFC redesign makes moot.
- **Requirements:** `crates/trawl/src/` shrinks toward ~250 lines.
- **Dependencies:** Phase 0 prompts locked (so we know we can actually replace the behaviour).
- **Files:** delete `crates/trawl/src/session.rs`, `crates/trawl/src/score.rs`, `crates/trawl/src/anonymize.rs`, `anonymize.local.toml`; strip `--window`, `--step`, `--threshold`, `--model`, `--dump-scores` from `crates/trawl/src/main.rs`; drop `Scores` from entry struct.
- **Approach:** Mechanical deletion, keep compilation green with stubs where main.rs still references removed items (will be rewritten in Phase 4).
- **Patterns:** Clean cut; no graceful fallbacks to the old path.
- **Test expectation:** none — pure removal, covered by Phase 6 smoke test.
- **Verification:** `cargo check` passes; `wc -l crates/trawl/src/*.rs` shows the reduction.

### Phase 2 — `extractor.rs` (Sonnet stage)

- **Goal:** Spawn `claude -p --model sonnet --add-dir <session>` with the extractor prompt and parse the JSON result into draft entries.
- **Requirements:** Robust against prose wrapping; one automatic retry; debug-file on second failure.
- **Dependencies:** Phase 0, Phase 1.
- **Files:** `crates/trawl/src/extractor.rs`, `crates/trawl/prompts/extractor.md`.
- **Approach:** Thin wrapper — subprocess, stdout capture, `first '[' .. last ']'` slicing, `serde_json` parse into `DraftEntry { title, category, tags, quote, why }`. Retry once with a stricter suffix. On second failure, dump raw output to `target/trawl-debug/<session>.raw.txt` and skip.
- **Patterns:** HEAT-style JSON robustness; minimal `--allowedTools "Read"`.
- **Test scenarios:**
  - Happy path: Sonnet returns clean JSON array → parses into N drafts.
  - Edge case: Sonnet wraps JSON in ```` ```json ```` fences → extractor strips and parses.
  - Edge case: Sonnet returns empty array → zero drafts, no error.
  - Error path: Sonnet returns prose only → retry fires, still fails → debug file written, session skipped with stderr log.
  - Integration: real audited session → drafts match audit notes.
- **Verification:** Unit tests on the JSON slicer; integration test on one committed fixture session.

### Phase 3 — `tokeniser.rs` (Haiku stage)

- **Goal:** For each draft, spawn `claude -p --model haiku` with the tokeniser prompt; parse `{body, entities, needs_review, review_reason}`.
- **Requirements:** Never persist the transient `placeholder → real_value` map. Entities graph ships in frontmatter.
- **Dependencies:** Phase 0, Phase 2.
- **Files:** `crates/trawl/src/tokeniser.rs`, `crates/trawl/prompts/tokeniser.md`.
- **Approach:** Subprocess, JSON parse, validate shape, return `TokenisedEntry { body, entities, needs_review }`. The transient map is not returned by the prompt (only the caller receives it in memory for registry.grow; it is never written).
- **Patterns:** ZFC anonymisation; fail-closed on parse error → `needs_manual_review: true`.
- **Test scenarios:**
  - Happy path: draft with one name + one city → returns body with `#USER_001#` and `#CITY_001#`, entities graph present.
  - Happy path: diarisation — "Alice" / "alice" / "A." all map to the same `#USER_001#`.
  - Edge case: draft with no PII → body unchanged, entities empty, `needs_review: false`.
  - Edge case: draft with a hierarchical chain (city → region → country) → entities graph includes `in` links.
  - Error path: Haiku returns malformed JSON → entry marked `needs_manual_review: true` with reason.
  - Error path: draft contains a Doppler-shaped token → `#CRED_NNN#` + `needs_review: true`.
  - Integration: runs against a Phase 2 draft end-to-end.
- **Verification:** Unit tests on parse + shape validation; fixture-based integration test; manual spot-check on 3 known drafts.

### Phase 4 — Main loop rewrite

- **Goal:** Walk → state-check → pool → extract → tokenise → registry grow/validate → write entry → update state.
- **Requirements:** Concurrency preserved (per-session now, not per-window).
- **Dependencies:** Phases 2 and 3.
- **Files:** `crates/trawl/src/main.rs`, `crates/trawl/src/entry.rs` (simplified writer), `crates/trawl/src/pool.rs` if extracted.
- **Approach:** `walkdir` over `~/.claude/projects/`; state-file lookup; dispatch to work-stealing pool; per-worker pipeline is `extract → for each draft { tokenise; registry.grow; registry.validate; write entry; }`; atomic state-file update at worker-end.
- **Patterns:** Keep the existing pool abstraction; simplify flags to `--extractor-model`, `--tokeniser-model`, `--concurrency`.
- **Test scenarios:**
  - Happy path: 3 fresh sessions → 3 batches of entries written.
  - Edge case: session already in state with unchanged hashes → skipped.
  - Edge case: session present but `extractor_prompt_sha256` changed → re-trawled.
  - Error path: extractor crashes on one session → other workers continue; state for the failed session is not advanced.
  - Integration: dry-run flag prints intended work without touching disk.
- **Verification:** Smoke run on 2-session fixture directory; entry count and state-file diff match expectation.

### Phase 5 — State file

- **Goal:** Hash-based skip logic per `todos/018`, simplified (no append mode).
- **Requirements:** Decision matrix from the todo (file hash + both prompt hashes + trawl version).
- **Dependencies:** Phase 4.
- **Files:** `crates/trawl/src/state.rs`, `content/.trawl-state.json` (data file).
- **Approach:** `serde_json` map keyed by absolute session path → `{file_sha256, size_bytes, mtime, extractor_prompt_sha256, tokeniser_prompt_sha256, trawl_version, extracted_entry_ids}`. Single atomic rewrite at end of each worker's batch.
- **Patterns:** Conservative — any mismatch re-trawls from scratch.
- **Test scenarios:**
  - Happy path: unchanged session → skipped.
  - Edge case: session file modified → re-trawl.
  - Edge case: extractor prompt edited → all sessions re-trawl.
  - Edge case: tokeniser prompt edited → all sessions re-trawl.
  - Edge case: trawl version bump → re-trawl.
  - Error path: corrupt state file → rebuild from scratch, warn on stderr.
- **Verification:** Unit tests per matrix row.

### Phase 5b — PII registry + `trawl validate`

- **Goal:** `content/.pii-registry.json` grows with every transient PII value seen; `registry.validate(body)` catches literal leaks; `trawl validate` re-runs validation across `content/entries/`.
- **Requirements:** Registry is gitignored. Never resolves placeholders. Only answers "does `body` contain any literal string the registry has seen?"
- **Dependencies:** Phase 3 (produces the transient map), Phase 4 (wiring).
- **Files:** `crates/trawl/src/registry.rs`, `content/.pii-registry.json`, `.gitignore` (append `content/.pii-registry.json`).
- **Approach:** Load on startup; after each Haiku call, `grow(literals)` stores the SHA-256 hex digest of every literal the tokeniser masked in this draft. Plaintext never touches disk — only the hex digests do. Validate by iterating every byte-substring window of length `min_len..=max_len` over the tokenised body, SHA-256 hashing each slice, and checking membership in the on-disk hash set. Any hit forces `needs_manual_review: true`. `trawl validate` walks `content/entries/` and re-runs the same substring-scan over each body. Substring hashing is deliberately boring — no Rabin-Karp, no salt, no tuning knobs beyond the length range. The hash set is the authority; the scan is the safety net.
- **Patterns:** Fail-closed; hashed persistence; plaintext never on disk.
- **Test scenarios:**
  - Happy path: tokenised body contains no literal PII → passes.
  - Edge case: synthetic fixture — paste a known credential into a draft, run Stage 2 + registry.validate → entry gets `needs_manual_review: true`.
  - Edge case: two sessions share the same real name → same placeholder assignment within each entry, but registry sees both and catches any future leak.
  - Error path: `.pii-registry.json` missing → start empty.
  - Integration: `trawl validate` over all existing entries returns clean or flags entries.
- **Verification:** Synthetic-leak test passes; `.gitignore` entry present.

### Phase 6 — Smoke test against the Sonnet audit

- **Goal:** Validate the redesign matches or beats the manual audit's findings.
- **Requirements:** Acceptance criteria from `todos/022`:
  - Extract ≥90% of the moments the Sonnet audit identified manually.
  - Zero entries anchored on `[tool_result:` or `[tool: Bash]` text.
  - No `narrative_completeness: 0.25`-equivalent slop.
  - Zero Doppler-token-style leaks.
  - State file correctly skips unchanged sessions on rerun.
  - Bumping either prompt hash triggers re-trawl.
  - Synthetic-leak fixture caught by registry validation.
  - `.pii-registry.json` in `.gitignore`.
  - `trawl validate` re-runs validation across existing entries.
- **Dependencies:** Phases 1–5b.
- **Files:** `crates/trawl/tests/smoke_audit.rs` (or shell test harness under `scripts/`).
- **Approach:** Run Trawl against the 6 audited sessions; diff extracted entries vs. audit ground truth stored in a fixture file. Rerun to confirm state-file skip. Bump a prompt, rerun, confirm re-trawl.
- **Patterns:** Ground-truth diffing; explicit acceptance checklist.
- **Test scenarios:**
  - Happy path: fresh run on audited sessions → ≥90% recall.
  - Edge case: rerun with no changes → all sessions skipped.
  - Edge case: tweak extractor prompt → all sessions re-run.
  - Edge case: inject credential fixture → caught by registry.
  - Error path: one session fails JSON parse → other 5 complete.
- **Verification:** Every acceptance bullet above ticks green.

### Phase 7 — Full corpus run

- **Goal:** Run Trawl over `~/.claude/projects/` end-to-end.
- **Requirements:** Tolerate rate limits via incremental state (daily batch if needed).
- **Dependencies:** Phase 6 green.
- **Files:** no code changes expected; operational only.
- **Approach:** Start with `--concurrency 2`. Monitor wall time, token spend, failure rate. Review a sample of ~30 generated entries by hand. Run `trawl validate` across the corpus afterward.
- **Patterns:** Conservative rollout; measure before optimising.
- **Test scenarios:**
  - Happy path: corpus completes; entries land in `content/entries/`.
  - Edge case: daily token cap → incremental state lets the next day continue.
  - Edge case: multi-MB session → fails loudly; logged for v2 chunking decision.
  - Error path: Claude CLI auth issue mid-run → workers exit cleanly; state file records progress so far.
- **Verification:** Post-run `trawl validate` clean; manual review of 30 entries approves voice and anonymisation.

## System-Wide Impact

- **Interaction graph.** Trawl ↔ Claude CLI subprocess (new); Trawl → `content/entries/` (unchanged shape plus `entities` field); Trawl → `content/.trawl-state.json` (new); Trawl → `content/.pii-registry.json` (new, gitignored). Any downstream consumer reads `entities` from frontmatter (contract documented later).
- **Error propagation.** Extractor JSON failure → retry → debug file → skip session (not fatal). Tokeniser failure → entry flagged `needs_manual_review`. Registry validation hit → entry flagged `needs_manual_review`. State file corruption → rebuild from scratch.
- **State lifecycle.** Transient PII map: in-memory per Haiku call, dropped immediately. Registry: persistent, hash-only on disk. Entry frontmatter: placeholders + entity graph, never real values.
- **Unchanged invariants.**
  - Verbatim quotes in `quote` block.
  - Entry filenames and Markdown writer shape (minus `Scores`).
  - `content/used.jsonl` append-only log.
  - Conventional Commits discipline.

## Risks & Dependencies

| Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|
| Sonnet wraps JSON in prose / markdown | high | medium | robust bracket extraction + one retry + debug-file fallback |
| Multi-MB session overflows Sonnet context | medium | medium | fail loudly in v1; measure frequency before building chunker |
| Haiku misses a PII span (false negative) | medium | high | registry grow + validate catches literal leaks on future runs; `needs_review` flag; manual review pass |
| Daily token / rate-limit caps during Phase 7 | medium | low | incremental state enables daily batching |
| Prompt drift between local iteration and committed file | low | medium | prompt hash in state file forces re-trawl on edit |
| Downstream consumers break on new `entities` frontmatter | medium | medium | document informal contract; land Phase 4 before any consumer depends on the field |
| Registry plaintext leaks via `.pii-registry.json` | low | high | hashed storage on disk; `.gitignore`; plaintext confined to per-run memory |
| One Haiku call per draft dominates wall time | low | low | deferred optimisation; batch mode if measurements justify |

## Alternative Approaches Considered

- **Keep the regex anonymiser, bolt Haiku on as a second pass.** Rejected — duplicates responsibility, preserves the regex failure modes that leaked Doppler tokens twice.
- **Batch all drafts from one session into a single Haiku call.** Rejected — earlier batching attempts mixed identities across drafts. Reconsidered only if wall time dominates.
- **Persistent Stage 3 registry that resolves `#USER_001#` → "Alice" globally.** Rejected — removes per-post substitution flexibility, creates a stored PII map, and reintroduces cross-entry conflict edge cases.
- **Chunk sessions upfront at time-gap boundaries.** Rejected for v1 — adds complexity before measuring overflow frequency.
- **Append-mode state file (extract only new turns).** Rejected — Sonnet handles the whole session cheaply enough; append-mode reintroduces framework cognition about "which turns are new".
- **Store prompt files outside the repo.** Rejected — prompt text is load-bearing and must be versioned.

## Phased Delivery

1. **Phase 0** — lock prompts by hand on 3–5 sessions.
2. **Phase 1** — delete framework-cognition modules; compile green.
3. **Phase 2** — `extractor.rs` + Sonnet subprocess.
4. **Phase 3** — `tokeniser.rs` + Haiku subprocess.
5. **Phase 4** — main loop rewrite; pool-per-session.
6. **Phase 5** — state file wired in.
7. **Phase 5b** — PII registry, `trawl validate`, `.gitignore` update.
8. **Phase 6** — smoke test vs. Sonnet audit ground truth.
9. **Phase 7** — full corpus run.

Each phase lands as its own Conventional Commit (`refactor(trawl):` /
`feat(trawl):`). Phase 1 must not ship without Phase 2–4 queued, since
it breaks the binary intentionally.

## Sources & References

- `todos/022-trawl-zfc-redesign.md` — source of truth for this plan
- `todos/017-trawl-batch-scoring.md` — superseded
- `todos/018-trawl-incremental-state.md` — reshaped into Phase 5
- `todos/019-anonymization-hardening.md` — reshaped into Phases 3 + 5b
- `todos/020-trawl-tool-result-mislabeling.md` — superseded
- `todos/021-trawl-overlap-dedup-too-loose.md` — superseded
- `docs/solutions/design-decisions/zero-framework-cognition-20260320.md`
- `docs/solutions/design-decisions/zfc-anonymization-20260406.md`
- `docs/solutions/content-pipeline/verbatim-quotes-not-summaries-20260320.md`
- `CLAUDE.md` — project principles (ZFC, verbatim quotes, simplicity, anonymisation)

## Execution Log

- **2026-04-07 — Phase 0 complete** (commit `5efd691`). Locked
  `crates/trawl/prompts/extractor.md` and `crates/trawl/prompts/tokeniser.md`
  on branch `refactor/trawl-zfc-scaffold`. Purely additive; no Rust touched.
