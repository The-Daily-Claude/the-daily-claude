---
title: Tool Results Can Support The Joke, But Cannot Be The Speaker
category: design-decision
date: 2026-04-10
tags: [trawl, extractor, tool-results, editorial-policy, prompt-design, zfc]
---

# Context

During live backend subset runs, one Gemini extraction shipped a
`[TOOL_RESULT]` block directly into the candidate entry. The first read
of the then-current extractor contract said this was a hard violation:
tool results were categorically excluded from quoted output.

The user clarified the intended editorial policy. Tool results are not
forbidden because they are "tool results." They are forbidden when they
become the *main event*. A short tool result can be the load-bearing
setup or punchline support for a stronger human / assistant / thinking
beat around it. What it must not become is the dominant speaker of the
moment.

That distinction is small in wording and large in behavior. It required
changing the extractor prompt, tokeniser prompt, Rust-side docs, and the
README together.

# The rule

**Tool results may appear in an extracted quote only as brief supporting
context around a stronger human / assistant / thinking beat.**

Corollaries:

1. Use an explicit `[TOOL_RESULT:<name>]` label when the output is kept.
2. Quote the minimum span needed for the joke to land.
3. If the moment is funny only because of the raw tool output, drop it.
4. Build logs, diffs, stack traces, and long dumps are still bad output
   even when they are technically "verbatim."

# Why this matters

Treating tool output as categorically banned is too rigid for real
debugging transcripts. Some beats genuinely depend on a one-line result:
the failed command, the "Hi!" that disproves a theory, the log line that
collapses an elaborate explanation. Removing that line can make the
moment unintelligible.

Treating tool output as just another speaker is the opposite failure.
Once the extractor starts centering the raw output itself, the archive
fills with logs, command results, and compiler noise that are technically
real but editorially dead. The human or assistant reaction is what makes
the beat postable.

This is the same design boundary as the tokeniser PII rule: user-visible
output matters, but the pipeline must still know which parts are primary
content and which are supporting structure.

# Consequences for the contract

- The extractor prompt must explicitly allow `[TOOL_RESULT:<name>]`
  blocks while saying they cannot dominate the moment.
- The tokeniser prompt must preserve `[TOOL_RESULT:*]` labels exactly,
  because once the extractor is allowed to emit them they become part of
  the on-disk contract.
- Rust-side comments and README examples must match the prompt. A prompt-
  only change is not enough when the type docs and user docs still teach
  the old rule.

# Examples

## Good

A short result line that disproves the assistant's theory, followed by
the assistant admitting it was wrong:

```text
[HUMAN]: Why the f on Earth would the prompt affect auth?

[ASSISTANT]: You're right, it wouldn't. Let me stop guessing and test it:

[TOOL_RESULT:Bash]: Hi!

[ASSISTANT]: You're right, it wouldn't. I'm making things up at this point.
```

The joke is not "Hi!" The joke is the collapse of the theory.

## Bad

A long tool result block with no real surrounding beat:

```text
[TOOL_RESULT:Read]: # Boucle - autonomous agent loop
...
[TOOL_INPUT:Edit]: env > "$SANDBOX_DIR/logs/last-env.log"
```

This is operational trace, not a postable moment.

# Related

- `tokeniser-pii-boundary-whole-draft-20260408.md` - once a label is
  allowed into the quote contract, downstream stages must preserve it
- `two-stage-zfc-pipeline-in-practice-20260408.md` - prompts are the
  code, but only when the rest of the system agrees on the same contract
- `docs/HANDOFF.md` - records the Gemini and Codex subset runs that
  forced this clarification
