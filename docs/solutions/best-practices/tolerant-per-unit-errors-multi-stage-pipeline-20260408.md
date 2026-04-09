---
title: Tolerant Per-Unit Errors In Multi-Stage LLM Pipelines — `match`, Don't `?`
category: implementation
date: 2026-04-08
tags: [error-handling, rust, llm-orchestration, trawl, resilience]
related_commits: [f57348f]
---

## Problem

The first draft of Trawl's `process_session` function did the obvious
Rust thing for a two-stage pipeline. Sonnet extracted some drafts; the
code then iterated over them and tokenised each with Haiku.
Every call propagated failures with `?`:

```rust
// Bug: one Haiku hiccup sinks every other draft in this session.
fn process_session(...) -> Result<Vec<TokenisedDraft>> {
    let drafts = extractor::extract_session(session_path, extractor_model)?;
    let tokenised: Vec<_> = drafts
        .into_iter()
        .map(|d| tokeniser::tokenise_entry(&d, tokeniser_model))
        .collect::<Result<Vec<_>>>()?; // <-- single failure poisons all
    Ok(tokenised)
}
```

The behaviour is easy to reproduce: Sonnet returns 5 drafts for a
session. The second draft tokenises fine. The third draft's prompt
lands on a Haiku rate limit, or the JSON parse fails, or the model
hallucinates an extra newline in the output. `?` propagates the error
out of `collect`, the function returns `Err`, and the **four
successfully extracted and tokenisable drafts are thrown away** —
including any that were already successfully tokenised in-memory
before the failing one. A Sonnet call that cost real money and real
time has no payoff because one downstream unit had a bad day.

This is a specific instance of a general pattern: **in a multi-stage
LLM pipeline, a unit failure is not a batch failure**, and conflating
the two is a quiet money-waster and a silent-data-loss bug.

## What we learned

Use an explicit `match`, log the failure structurally, and keep the
loop alive. The shipped version:

```rust
fn process_session(
    session_path: &Path,
    _file_sha: &str,
    extractor_model: &str,
    tokeniser_model: &str,
) -> Result<Vec<TokenisedDraft>> {
    let drafts = extractor::extract_session(session_path, extractor_model)
        .with_context(|| format!("extract {}", session_path.display()))?;

    // Tolerant per-draft tokenisation: a single Haiku failure must not
    // sink every other moment Sonnet found in this session. We log the
    // failure and keep going.
    let mut out = Vec::with_capacity(drafts.len());
    for draft in drafts {
        match tokeniser::tokenise_entry(&draft, tokeniser_model) {
            Ok(tokenised) => {
                let mut flagged = diff_literals(&draft.quote, &tokenised.body);
                // ... collect other flagged literals, push into `out`
                out.push(TokenisedDraft { tokenised, flagged_literals: flagged });
            }
            Err(e) => {
                // Intentionally do NOT include draft.title here — the
                // extractor allows real PII in metadata until the
                // tokeniser runs, and this failure path fires exactly
                // when the tokeniser didn't produce usable output. Log
                // only structural info so stderr/CI logs stay PII-free.
                eprintln!(
                    "  tokenise draft failed: {e:#} (skipping this draft, keeping session alive)"
                );
            }
        }
    }
    Ok(out)
}
```

Three properties of this shape matter:

1. **Failure is per-unit.** One draft failing is one draft dropped.
   The other drafts from the same extractor call continue through the
   pipeline. The containing session stays "successful" — the state
   file records it as fresh and will not re-trawl on the next run.

2. **The logged error is PII-safe by construction.** The failing unit
   is a draft whose title/tags/category have not yet been tokenised,
   so it may contain real names, paths, or credentials. The `eprintln`
   intentionally omits `draft.title` and friends; it prints only the
   error chain (which is itself PII-safe — see
   `error-chains-must-not-leak-pii-20260408.md`) and a message saying
   we are keeping the session alive. stderr stays free of plaintext
   PII even in the failure path.

3. **The outer layer still distinguishes "session failed" from "draft
   failed".** `process_session` returns `Ok(Vec::new())` if every
   draft failed, and the caller in `main.rs` will then record the
   session as processed with zero entries and move on. An error
   return from `process_session` is reserved for the cases where the
   **session itself** is unprocessable — the extractor call failed, or
   the session file couldn't be read. Unit failures do not climb to
   batch failures.

**The general rule**: when orchestrating multiple LLM calls in
sequence, every `?` on a per-unit call is a question: *"should failing
this one unit sink every other unit in this containing batch?"*. If
the answer is "no" (and it almost always is, because the outer batch
has already paid for work), replace `?` with an explicit `match` and
decide the failure policy deliberately. `?` is a great default for
linear pipelines where every step depends on the previous; it is a
bad default for fan-out stages where units are independent.

## How to apply

1. **Walk the call graph for `?` on per-unit LLM calls.** Any
   `collect::<Result<Vec<_>>>()` over LLM output is almost certainly
   wrong in a pipeline that has already paid for the containing
   batch. Replace with a `match` loop.

2. **Log structure, not content, in the failure arm.** When the
   failing unit is an untokenised draft (or anything pre-scrub),
   logging its fields is a PII leak. Log the error chain and a
   bounded structural hint (byte range, length, session path) — never
   the raw unit.

3. **Keep the return type `Result<Vec<T>, E>`, not `Vec<Result<T, E>>`.**
   The caller should not have to re-implement the drop-and-log
   decision at every layer. Decide it once, inside the function that
   owns the batch.

4. **Distinguish unit failure from batch failure in the caller.**
   `Ok(Vec::new())` and `Err(..)` must mean different things. The
   session that produced zero tokenised drafts should still be
   recorded as "processed" so the state file's freshness cache
   advances; a session whose extractor call itself died should not.

## Code pointer

- `crates/trawl/src/main.rs` — `process_session` at the bottom of the
  file shows the `match` loop and the PII-safe failure log
- Early PR #3 diff — the `?`-based version that the Copilot review
  did not catch but that a smoke run did; we noticed it when a single
  Haiku JSON hiccup dropped an entire session's worth of work

## Related

- `docs/solutions/best-practices/error-chains-must-not-leak-pii-20260408.md`
  — the companion rule for what the error message itself can contain
- `docs/solutions/best-practices/balanced-bracket-json-candidate-scan-20260408.md`
  — one of the failure modes that used to sink batches (robust
  JSON parse reduces the frequency but doesn't eliminate it)
- `docs/solutions/design-decisions/two-stage-zfc-pipeline-in-practice-20260408.md`
  — the surrounding pipeline whose economics make this tolerance
  load-bearing
- `docs/solutions/design-decisions/temperature-variance-acceptance-20260408.md`
  — the sibling lesson that per-unit variance is a feature; per-unit
  failure is therefore expected and cannot be allowed to cascade
