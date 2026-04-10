---
title: Behavioral contract changes need full-surface sync
category: implementation
date: 2026-04-10
tags: [contract-sync, prompts, documentation, trawl, implementation]
---

## Problem

In `trawl`, the real behavior is not defined by code alone. It lives
across:

- prompt files
- parser/type comments
- README claims
- handoff notes

That makes behavioral drift easy. A prompt can change while the tokeniser
still assumes the old shape. README examples can keep describing a
contract that no longer exists. Handoffs can tell the next session the
wrong story.

This session hit exactly that class of problem again when the tool-result
quote policy changed.

## What changed

The policy started as a single question: is `[TOOL_RESULT]` always a
violation?

The answer turned out to be a contract change:

- tool results are not categorically banned
- they are allowed only as brief supporting context
- they must not become the main speaker of the moment

That answer was only valid once every affected surface agreed:

1. `prompts/extractor.md`
2. `prompts/tokeniser.md`
3. `src/extractor.rs` comments on `DraftEntry.quote`
4. `crates/trawl/README.md`
5. root `README.md`
6. `docs/HANDOFF.md`

Changing only the extractor prompt would have created a silent mismatch:
the model could emit a new shape while the rest of the system still
documented or reasoned about the old one.

## What we learned

**Treat a behavioral contract change as a multi-surface change, not a
prompt tweak.**

If any of these are true:

- the allowed quote labels changed
- a field's meaning changed
- a backend option changed what counts as valid output
- a stage boundary changed

then the work item is bigger than the prompt file. The prompt is only
one copy of the contract.

## Why this matters

`trawl` is a model-shaped system. The prompt is executable behavior, but
it is not the only place humans and downstream code learn that behavior.

If the prompt and the surrounding surfaces drift apart:

- reviewers misdiagnose correct behavior as a bug
- users trust stale README guidance
- later sessions waste time rediscovering what already changed
- downstream stages preserve or reject the wrong things because their
  assumptions are stale

Earlier in the same audit cycle, the README had already drifted away from
the shipped CLI and output schema. The tool-result policy change made the
same lesson visible again from a different angle.

## Apply this as a checklist

When a behavioral contract changes, check these surfaces in order:

1. **Primary prompt(s)** — the model-facing behavior
2. **Downstream prompt(s)** — anything that consumes the changed output
3. **Rust comments / type docs** — what future maintainers will trust
4. **Crate README** — the user-facing contract
5. **Repo README** — the headline public positioning
6. **Handoff docs** — session continuity for the next operator

If the behavior is cross-cutting enough, compound the learning in
`docs/solutions/` too. Otherwise the next contract change will repeat the
same drift pattern.

## A practical heuristic

If your first instinct is "I'll just patch the prompt," ask:

**What other file would now be lying if I stopped here?**

Start with the prompt, finish with the last lie removed.

## Related

- `docs/HANDOFF.md` — this session's contract-sync record
- `crates/trawl/README.md`
- `docs/solutions/design-decisions/tool-results-support-context-not-speech-20260410.md`
- `docs/solutions/design-decisions/prompt-hash-as-cache-invalidation-20260408.md`
