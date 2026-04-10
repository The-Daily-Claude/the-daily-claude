---
title: Live Subset Backend Evals Beat Theoretical Rankings
category: implementation
date: 2026-04-10
tags: [trawl, backend-eval, gemini, codex, smoke-test, quality, archive-mining]
---

# Problem

Provider-qualified model flags made it easy to *configure* `trawl` for
Codex, Gemini, OpenCode, or Claude. That did not answer the question
that matters before a public claim or a full-corpus run:

**which backend actually finds archive-worthy moments rather than merely
returning structured output?**

The tempting shortcut is to rank backends by reputation, model family,
or benchmark lore. That is not enough for an editorial mining pipeline.

# What we learned

**Run the same known-interesting sessions across candidate backends and
judge the archive output, not the raw extraction count.**

This session used the same two historically interesting Claude sessions
for all non-Claude runs:

- Gemini: `gemini-2.5-pro` extractor + `gemini-2.5-flash` tokeniser
- Codex medium: `gpt-5.4` extractor + `gpt-5.3-codex-spark` tokeniser
- Codex high: same tokeniser, extractor effort raised to `high`

The practical findings were:

1. **Gemini had the best keep-rate.** It under-extracted versus the
   historical baseline, but the strongest hits were real and needed the
   least downstream triage.
2. **Codex medium had higher recall on one cleaner session, but worse
   precision overall.** It surfaced more candidates, but several were
   filler or too tool-result-heavy to archive.
3. **Codex high improved the cleaner session, not the messy one.**
   Raising effort dropped some filler on the first session, but the
   second debugging-heavy session sprawled back out and stayed noisy.
   Runtime also increased materially.
4. **Archive outcome is the metric that matters.** In the downstream
   private archive, Gemini yielded three keepers from the subset and
   Codex high yielded only one additional keeper beyond those.

# Why this works

The archive miner is not a generic reasoning benchmark. It is an
editorial selector. A backend that returns more JSON objects is not
better if most of them are weak, redundant, or structurally off-policy.

Using the same known-interesting sessions controls the comparison:

- same source material
- same historical expectation of "there is signal here"
- same downstream keep/discard bar

That gives an answer tied to the actual product: *what survives into the
archive?*

# Practical ranking

As of this session:

1. Claude Code remains the intended default.
2. Gemini is the strongest non-Claude archive miner today.
3. Codex is promising, but still needs tighter prompt discipline before
   it becomes a better default than Gemini for `trawl`.

# How to apply

- Use a **small live subset** before any full-corpus backend switch.
- Reuse **known-interesting sessions**, not arbitrary fixtures.
- Record three outputs for every backend trial:
  - raw extracted count
  - runtime / latency
  - archive-worthy keepers after triage
- Treat **keeper count and keeper quality** as the decision metric.
- If a backend only wins by emitting more candidates, it has not won.

# Related

- `historical-output-as-regression-dataset-20260408.md` - why known
  historical outputs are the right smoke-test source
- `temperature-variance-acceptance-20260408.md` - why exact string
  equality is the wrong success metric for LLM pipelines
- `../design-decisions/tool-results-can-support-the-joke-but-cannot-be-the-speaker-20260410.md`
  - one of the quality rules Codex kept violating in the subset runs
