---
title: Two-Stage ZFC Pipeline In Practice — Prompts Are The Code
category: design-decision
date: 2026-04-08
tags: [zfc, trawl, prompt-engineering, iterative-tuning, sonnet, haiku]
related_commits: [f57348f]
---

## Context

The ZFC principle was written down on 2026-03-20 (see
`docs/solutions/design-decisions/zero-framework-cognition-20260320.md`):
when you catch yourself building a regex classifier or a decision tree
around an LLM call, stop and make it one LLM call with good examples
instead. That document was aspirational — a rule we agreed to follow
next time we built something.

PR #3 was the first time we lived inside it. Trawl went from ~1000 lines
of sliding windows, role enums, operational-pattern blacklists, an
8-dimension scoring rubric, a regex anonymizer, and overlap dedup to two
sibling ZFC stages — Sonnet extractor + Haiku tokeniser — plus a
deterministic PII safety net. The commit (`f57348f`) calls the deleted
machinery by name in its body. The replacement is about 400 lines of
Rust orchestration around two `claude -p` calls.

The part the 2026-03-20 doc did not prepare us for: when the framework
is gone, the **prompt becomes the program**, and the prompt has to be
debugged like a program. This document captures what that debugging
loop actually looked like.

## The pattern

**Two concentric control loops, both ZFC:**

1. *Outer* (per session): Sonnet reads the JSONL directly and returns a
   JSON array of draft moments. No Rust picks windows. No Rust
   disambiguates roles. No Rust scores.
2. *Inner* (per draft): Haiku takes the full draft (title + category +
   tags + quote) as a single JSON object and returns every field
   tokenised with coreference-consistent placeholder ids. No Rust
   recognises names. No Rust scrubs credentials.

The Rust code around those two calls does only the work the model
*cannot* do from inside a prompt: hashing files for the cache, atomic
writes, growing the PII registry, spawning subprocesses, and managing
concurrency. Every *decision* lives in the prompt.

**The debugging loop has the shape you expect from code, not from
prose.** Each prompt edit is a commit. Each smoke run is a test. Each
regression on the ground-truth set is a failing test. The P0 inside PR
#3 had nothing to do with Rust:

1. First draft of the extractor prompt said "be harsh". It over-anchored
   on strength and produced **1 entry per session** on the smoke set.
2. Second draft tried to relax the bar. The model over-corrected the
   other way and produced **0 entries** on the same sessions — it read
   the rewording as "only extract if you are sure", which was stricter
   than "be harsh".
3. Third draft reframed the anchor. Added *"session length is a poor
   proxy for moment density"* and an explicit self-check step *"after
   your candidate list, re-read the session and ask 'did I miss
   anything?'. The first pass usually does."* That version produced **3
   entries** on the same session. The full smoke run that followed
   extracted 12 entries from 3 sessions with 0 tokeniser failures in
   ~16 min wall time at concurrency 2.

The productive change was not a rule, a threshold, or a tag. It was a
**reframing of the task's cognitive anchor** — telling the model to
measure the session by content density rather than size, and forcing a
second pass. Neither is expressible as Rust. Both are one paragraph in
`prompts/extractor.md`.

## Why it matters

ZFC is not just "use an LLM instead of a classifier". That is the
*decision*. The *practice* is that your source of bugs moves: you stop
debugging `if` branches and start debugging how the model anchors on a
task. A prompt edit is a code edit. A prompt diff is a code diff. And
the iteration loop is: ship → smoke run → look at the output →
re-anchor → ship again.

Two implications fall out of this:

- **Prompts live next to code and ship with it.** In Trawl they are
  `include_str!`'d from `crates/trawl/prompts/*.md` so the binary
  embeds them. There is no "prompt store", no "prompt management
  service". They are source files.
- **Prompt edits are first-class cache invalidators.** Every state
  record in `content/.trawl-state.json` carries
  `extractor_prompt_sha256` *and* `tokeniser_prompt_sha256`. Editing
  either prompt automatically invalidates every session it ever
  produced an entry for, on next run. There is no migration script.
  The hash IS the version. See the companion note
  `prompt-hash-as-cache-invalidation-20260408.md`.

The 2026-03-20 doc said "the model IS the framework". The 2026-04-08
lesson is: *and the prompt IS the code, and iterative tuning IS the
implementation loop*. If you are not willing to iterate on the prompt
the way you iterate on a function, you are not doing ZFC — you are
just calling an LLM and hoping.

## Code pointer

- `crates/trawl/src/main.rs` — the 400-line orchestration layer, all
  that is left after the framework was deleted
- `crates/trawl/src/extractor.rs` — Stage 1 ZFC call site
- `crates/trawl/src/tokeniser.rs` — Stage 2 ZFC call site
- `crates/trawl/prompts/extractor.md` — Sonnet extractor prompt; see
  the `## Find them all` section for the "session length is a poor
  proxy" + "did I miss anything" reframing
- `crates/trawl/prompts/tokeniser.md` — Haiku tokeniser prompt
- Squash commit `f57348f` — the diff where ~1000 lines of framework
  cognition became two prompts and a cache

## Related

- `docs/solutions/design-decisions/zero-framework-cognition-20260320.md`
  — the principle this is the first real application of
- `docs/solutions/design-decisions/zfc-anonymization-20260406.md` — the
  Haiku-as-scrubber pattern this pipeline operationalises
- `docs/solutions/design-decisions/tokeniser-pii-boundary-whole-draft-20260408.md`
  — how the tokeniser's input structure enforces the PII boundary
- `docs/solutions/design-decisions/prompt-hash-as-cache-invalidation-20260408.md`
  — treating prompts as content-addressed code
- `docs/solutions/design-decisions/temperature-variance-acceptance-20260408.md`
  — accepting non-determinism across runs
