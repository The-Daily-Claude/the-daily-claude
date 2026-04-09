---
title: "Trawl mislabels tool results as human turns"
priority: superseded
status: superseded
superseded_by: 022
---

> **Superseded 2026-04-07 by `todos/022-trawl-zfc-redesign.md`.** The new
> design has no `Role::User` / `Role::Assistant` enum. Sonnet reads the
> raw jsonl and disambiguates tool results from human turns implicitly.
> The mislabeling bug ceases to exist when the parser ceases to exist.

# Trawl Mislabels Tool Results as Human Turns

## Bug

`crates/trawl/src/session.rs:84` maps the JSONL `type` field directly to a
`Role`:

```rust
let role = match msg.r#type.as_str() {
    "user" => Role::User,
    "assistant" => Role::Assistant,
    _ => continue,
};
```

In Claude Code's JSONL, `type: "user"` covers two completely different things:

1. **Actual human prompts** — what Thomas typed
2. **Tool results** — output of Bash/Read/Edit/etc, returned to the model on
   the user-side of the protocol

Both get tagged `Role::User`. The skip at line 107 catches *single-line* tool
results only:

```rust
if text.starts_with("[tool_result:") && !text.contains('\n') {
    continue;
}
```

Multi-line tool results (which is essentially all of them — file contents,
command output, search results) sail through and end up in entries labeled
`[HUMAN]: [tool_result: ...]`.

## Why this is P1, not cosmetic

1. **Quality.** The window scorer counts tool-result blocks as human
   participation, inflating apparent dialogue density. Entries get extracted
   where the "human" side is entirely tool noise.

2. **Anonymization blast radius.** Tool results are exactly where most secrets
   leak — `cat ~/.config/...`, env vars, file contents, command output. The
   anonymization story has been reasoning about the wrong thing. Entry #222's
   Doppler token came in via a tool result mislabeled as `[HUMAN]:` —
   `todos/019` only fixes the regex layer, this fixes the parser layer.

3. **Slide unusability.** Several recent entries (220, 222, 229, others) lead
   with `[HUMAN]: [tool_result: ...]` quotes that are unquotable. Verbatim-quotes
   pipeline depends on the human turns being actual humans.

## Fix

Inspect the content blocks before assigning a role. A `user`-typed message
whose content is a `tool_result` block isn't a human turn — it's a tool
response. Three roles, not two:

```rust
pub enum Role {
    Human,         // user-typed message with text content
    Assistant,     // assistant message
    ToolResult,    // user-typed message containing tool_result blocks
}
```

The classification logic:

```rust
let role = match msg.r#type.as_str() {
    "user" => {
        if content_is_tool_result(&msg.message) {
            Role::ToolResult
        } else {
            Role::Human
        }
    }
    "assistant" => Role::Assistant,
    _ => continue,
};
```

Where `content_is_tool_result` checks the content array for any block with
`type: "tool_result"` (Anthropic content-block schema). If a single message
mixes human text and tool results (rare but possible), prefer `Role::Human`
and let the existing text-extraction strip the tool_result block.

## Solo-Claude is a legitimate genre — don't filter it out

Entry #237 (`After 4 Attempts, the IAM Stack Finally Deployed. But the Next
Step Failed.`) has **zero human turns** in its window — the human walked away
from a long deploy and came back later. The Claude-side monologue is the
entire story:

> Still deploying. This is 25+ minutes on the IAM stack… let me wait longer.
> …
> **Deploy IAM stack — SUCCESS!** After 4 attempts… But the next step failed.

It works because solo-Claude grinding through a long failure loop is its own
genre. Anyone who's walked away from an agent mid-deploy recognizes it. The
human's silence is part of the joke.

A naive fix to `min_user_messages` (count only `Role::Human`) would kill this
class of entry. That's a regression we cannot ship.

The right rule:
- The **session-level** `--min-user-messages` filter still gates which
  sessions Trawl bothers scanning at all, and it should count `Role::Human`
  (not the mislabeled total). The CLI flag already exists; only the default
  changes.
- **New default: 1.** One human turn anywhere in the session proves a person
  was involved at some point. The old default of 3 was rejecting solo-Claude
  sessions silently.
- **`--min-user-messages 0` is supported and meaningful.** Fully-autonomous
  Claude loops — subagents grinding in circles, scheduled tasks, runaway
  agentic-sandbox sessions — are their own genre. The user might explicitly
  want to mine those. The flag should accept 0 and the parser should not
  reject sessions on that basis.
- Inside a qualifying session, **window scoring is unchanged**. A window can
  have zero human turns and still score high — solo-Claude monologue is fair
  game and Haiku already understands that with the right prompt.

## Downstream consequences to wire up

- [ ] **`--min-user-messages` filter**: count `Role::Human`, not
      `Role::Human + Role::ToolResult`. **Default drops from 3 to 1.**
      Accept `0` as a valid value (mine fully-autonomous sessions). Today
      the filter overcounts (catches mislabeled tool results) AND undercounts
      (would reject solo-Claude sessions if we naively fixed it).
- [ ] **Window builder**: tool results stay in the window for context but
      are rendered as `[TOOL_RESULT: ...]` in the scoring prompt instead of
      `[HUMAN]:`. Haiku will then score them correctly as background.
- [ ] **Scoring prompt**: explicitly mention that `[TOOL_RESULT: ...]` blocks
      are *not* human participation and should not by themselves justify a
      high relatability or quotability score. Also mention that **solo-Claude
      windows (no human turns at all) are valid** when the assistant
      monologue tells a self-contained story — see entry #237 as the
      reference example. Don't penalise relatability just because the human
      stayed silent.
- [ ] **Entry writer**: emit `[TOOL_RESULT: ...]` as its own line type in the
      entry body (or filter it out entirely if it doesn't contribute to the
      story — content-pipeline call).
- [ ] **Backfill**: re-render existing entries' bodies with the correct role
      labels. No re-scoring needed for that step.

## Implementation

- [ ] Add `Role::ToolResult` variant + classifier helper
- [ ] Update `parse_session` to use it
- [ ] Update `min_user_messages` filtering to count only `Role::Human`
- [ ] Update window-prompt rendering to emit `[TOOL_RESULT: ...]`
- [ ] Update scoring prompt with the new label semantics
- [ ] Update entry-body writer with the new label
- [ ] Test fixtures: a session with one human turn followed by 8 multi-line
      tool results — assert the parser produces `[Human, ToolResult × 8]`,
      not `[User × 9]`
- [ ] Backfill rewrite of existing entries (label-only, not content)

## Acceptance

- [ ] Test fixture above passes
- [ ] No new entry contains `[HUMAN]: [tool_result:` after the fix lands
- [ ] With default `--min-user-messages 1`: rejects sessions with literally
      zero human turns, accepts sessions with one human prompt followed by
      hours of solo-Claude grinding
- [ ] With `--min-user-messages 0`: accepts fully-autonomous sessions (zero
      human turns) and mines them like any other
- [ ] Re-running Trawl on the session that produced #237 still produces an
      equivalent entry (regression test for the solo-Claude genre)
- [ ] Backfill pass produces a clean diff across existing entries
- [ ] Pairs cleanly with `todos/019` — the parser fix narrows the surface,
      the regex fix protects what remains
