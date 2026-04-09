# LLM for cognition, code for deterministic transformation

**Date:** 2026-04-09
**Context:** ZFC trawl extractor bug — `project: Users-alice` in entry frontmatter
**Category:** design-decisions

## The rule

**Trust the LLM for cognition. Do not use it for deterministic transformations that code solves correctly every time.**

- **Cognition tasks → LLM.** Classification, judgment, natural-language anonymization, *"is this funny?"*, *"is this a credential?"*, *"is this a real first name in dialogue?"* — things that require understanding.
- **Transformation tasks → code.** Path slugification, filename derivation, regex-pattern scrubbing, value normalization, byte-level operations — anything that has a single correct answer given the same input.

## The bug that made this rule visible

The ZFC trawl extractor was asking the LLM to fill in the `project:` frontmatter field from the session jsonl file it was analyzing. With no canonical source value, the LLM defaulted to slugifying the session file path, producing:

```yaml
project: Users-alice
```

This leaked the user's home directory into entry frontmatter across ~80+ entries in the #211–#310 range, fixed retroactively by the corpus-wide sweep in commit `2b79256`.

The root cause wasn't a bad prompt. It was the **wrong architectural layer**. Slugifying a path is pure code work. Asking an LLM to do it:

1. Costs tokens for zero cognitive benefit.
2. Produces wrong answers (the leak above).
3. Accepts non-determinism in exchange for nothing.
4. Forces the LLM to see the user's path — which is exactly what we're trying to anonymize.

The right fix is not *"add better prompt instructions."* The right fix is:

1. **Remove `project:` from the LLM response schema.** The prompt no longer asks for it.
2. **Compute `project:` in Rust** from the session file path basename (which Trawl already has as input — the file path *is* the input).
3. **Inject it post-call** into the resulting frontmatter.

The LLM continues to read the session content normally via `--add-dir` — that's cognition (extracting meaning from dialogue). Only the one deterministic field moves to code. Tracked in `todos/034-extractor-deterministic-path-handling.md`.

## Relation to ZFC

ZFC ("Zero Framework Cognition") in this repo means **trust the model for the cognition layer** — don't build regex classifiers around LLM calls, don't wrap the model in decision trees. See `docs/solutions/design-decisions/zero-framework-cognition-20260320.md`.

This rule is the *inverse* boundary: **don't use the model for the transformation layer.**

Together they define the LLM's scope:

- **ZFC:** "Don't wrap the model in decision trees. Send the data and let it classify."
- **This rule:** "Don't hand the model problems that aren't classification. Do the pre-processing in code."

The two rules meet at the prompt boundary. Before calling the LLM, pre-compute everything code can compute. Send only the cognitive problem.

## Decision heuristic

Before adding a field to any LLM response schema, ask:

1. **Is the answer the same every time given the same input?** → code.
2. **Does the answer require understanding, judgment, or natural-language parsing?** → LLM.
3. **Can the answer be derived from data Trawl already has in Rust?** → code.
4. **Is this something a human would need to read context to answer?** → LLM.

If you find yourself writing prompt instructions like *"format this as a slug"* or *"convert this path to a relative form"* or *"extract the hostname from this URL"* — **stop**. That's a transformation task. Delete the instruction, write a function.

## Corollary: when an LLM-generated field is wrong

The first question is **not** *"how do I tweak the prompt?"*

It is *"should this field have been LLM-generated at all?"*

In the `project: Users-alice` case, the answer was no. The field never needed to be LLM-generated. The prompt-tweaking path would have produced an endless game of *"add another anti-hallucination instruction"* for a problem that has a one-line code solution.

## Apply to the existing trawl codebase

While implementing todo #034, audit the rest of the response schema in `crates/trawl/src/extractor.rs` and `crates/trawl/src/tokeniser.rs` for any other field where the LLM is being asked to do mechanical work. Suspects to check:

- Source file path / project path frontmatter
- Date/timestamp fields
- Source URLs copied from the input
- Anything described as *"copy this value verbatim from the input"*

Entry ID generation is already handled in code (probe-and-retry on `atomic_write_exclusive` from PR #4). That's the right model. Replicate it.

## References

- `todos/034-extractor-deterministic-path-handling.md`
- Commit `2b79256` (2026-04-09 cleanup sweep — retroactive fix for the bug instances)
- `docs/solutions/design-decisions/zero-framework-cognition-20260320.md` — the original ZFC principle
- Feedback memory: `feedback_no_llm_for_deterministic_work.md` (session-level reinforcement; stored outside the repo)
