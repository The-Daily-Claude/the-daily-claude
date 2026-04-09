---
title: Accept Temperature Variance In ZFC Pipelines — Design The Editor, Not The Extractor
category: design-decision
date: 2026-04-08
tags: [zfc, non-determinism, llm-orchestration, trawl, editorial-pipeline]
related_commits: [f57348f]
---

## Context

During the PR #3 smoke runs, the same Sonnet extractor prompt against
the same session jsonl file returned *different* candidate moments
across invocations. The first run on one session turned up the "At
least Descartes had a grand vision" exchange and missed a couple of
others we could see by scrolling the file. A later run on the same
file turned up a different strong moment and missed Descartes. A third
run found both plus a third. No file changed. No prompt changed. Only
temperature-driven sampling variance.

The instinct from years of writing deterministic software is to *fix*
this: pass `temperature: 0`, pin the seed, retry until consistent,
cache the first result, anything. That instinct is wrong for ZFC
pipelines, and the Trawl redesign makes the opposite bet deliberately.

## The pattern

**Accept run-to-run variance as a feature of ZFC. Make the downstream
editor tolerant of diverse output, not the upstream extractor anchored
to determinism.**

Three design choices follow from this:

1. **No temperature pinning, no seeding, no "retry until stable".** The
   extractor call is a single `claude -p --model sonnet` with the
   default sampling behaviour. A parse failure gets *one* structural
   retry (prompt reminder: "return ONLY the JSON array") and that's
   it. The retry is for **malformed output**, not for
   **different content**. We don't re-extract looking for the
   "correct" answer because there is no correct answer — there is a
   distribution of valid answers.

2. **Quality gate per item, not per run.** Each individual draft the
   extractor returns is independently evaluated by the prompt's own
   quality bar: *"would a developer scrolling a feed stop at this
   quote"*. If a draft passes that bar, we keep it. If it doesn't,
   the extractor was supposed to drop it before returning. We do not
   compare two runs and pick "the better one" because the comparison
   is meaningless — both runs are sampling from a universe of valid
   extractions, and any single sample is a valid publication.

3. **The downstream editor is designed for diversity.** After
   extraction, entries go through the registry leak scan, the
   `needs_manual_review` flag, and eventually a human editor who picks
   which entries become posts. That pipeline was **built assuming the
   extractor is a firehose, not a query**. If tomorrow's run of Trawl
   over the same sessions yields a completely different set of 200
   entries than today's run, the editorial funnel still works — the
   human picks the ones that land. Determinism was never a
   requirement of the downstream consumer.

The extractor prompt even leans into this explicitly. The "Find them
all" section says *"when in doubt, split. The downstream editor can
dedupe; the extractor cannot un-merge"*. That sentence only makes
sense if you accept that the extractor will find different things on
different runs — otherwise "find them all" is a demand for
determinism the model can't deliver, and the prompt self-contradicts.

## Why it matters

There is a failure mode specific to people porting classical pipelines
to LLM calls: they wrap the LLM in a determinism harness — temperature
pins, retries, "pick the best of N" — because that's how they make
flaky units stable. In a ZFC pipeline this harness is actively
harmful. Three reasons:

- **It inverts the cost curve.** Retry-until-stable multiplies token
  spend by the retry count. ZFC pipelines only survive economically
  when each decision is a single call; stack retries for determinism
  and you've re-created the expensive pipeline you were trying to
  escape.
- **It discards signal.** Temperature variance is not noise — it is the
  model exploring the solution space. On the Descartes session, the
  "bad" first run still found **a** strong moment; the "bad" second
  run found **a different** strong moment. Accumulating runs over time
  grows the corpus in a way no deterministic extractor could, because
  the determinism would pin you to whichever moment the pinned
  temperature happened to favour first.
- **It moves the wrong complexity to the wrong place.** The difficulty
  you are trying to paper over — "the model sometimes misses things"
  — is better solved inside the prompt (the "did I miss anything?"
  self-check) than outside it (retry loops in Rust). Fixing it inside
  the prompt is a one-paragraph edit that ships with the binary.
  Fixing it outside is orchestration code, state, and race conditions.

This is the practical meaning of *"the model IS the framework"*. The
framework's contract with its callers is not "I return the same answer
every time". It is "every answer I return is individually publishable,
and the universe of possible answers is approximately what you want".
Design the editor around that contract instead of fighting it.

**The one place variance is *not* tolerated** is anonymization. The
tokeniser is expected to get PII right every time. We enforce that
through a **different** mechanism — the deterministic PII registry
backstop — not by pinning the tokeniser's temperature. Correctness
around PII lives in the registry and the review flag; quality around
extraction lives in the distribution. Two different kinds of
"correctness", two different enforcement mechanisms. Don't confuse
them.

## Code pointer

- `crates/trawl/src/extractor.rs` — `extract_session` does **one**
  structural retry and never compares runs
- `crates/trawl/prompts/extractor.md` — `## Find them all` (split when
  in doubt; editor can dedupe) and the "did I miss anything?"
  self-check
- `crates/trawl/src/registry.rs` — the deterministic safety net that
  carries the *correctness* burden the extractor doesn't
- `crates/trawl/src/main.rs` — the editorial flow: extract → tokenise →
  grow registry → validate → `needs_manual_review` if in doubt

## Related

- `docs/solutions/design-decisions/zero-framework-cognition-20260320.md`
  — the principle that the model is the framework
- `docs/solutions/design-decisions/two-stage-zfc-pipeline-in-practice-20260408.md`
  — the pipeline this variance-tolerance sits inside
- `docs/solutions/design-decisions/zfc-anonymization-20260406.md` — why
  variance is tolerated for extraction but not for anonymization
- `docs/solutions/best-practices/length-aware-substring-registry-20260408.md`
  — the deterministic backstop that carries the correctness burden
