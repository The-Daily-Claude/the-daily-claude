---
title: "Add OpenCode session-source ingestion"
priority: medium
status: pending
---

# Add OpenCode Session-Source Ingestion

## Why

`trawl` can use OpenCode as an extractor/tokeniser backend, but it cannot yet mine OpenCode's own archived conversations as source sessions.

## What

Inspect the local OpenCode session archive format, add deterministic detection/metadata derivation, and normalize conversations into the extractor's role-labeled transcript shape.

## Scope

- Locate the OpenCode session archive directory and document the path pattern.
- Capture redacted fixtures for the current event schema.
- Add `session.rs` detector/parser tests.
- Normalize user/assistant/reasoning/tool input/tool result events.
- Preserve source-model metadata once todo #037 lands.
- Update README and handoff.

## Acceptance

- `trawl stats <opencode-session-root>` works as a read-only archive pass.
- A fixture proves OpenCode logs normalize to `[HUMAN]`, `[ASSISTANT]`, and tool blocks where present.
- Non-OpenCode JSONL is not misdetected.
- `cargo test -p trawl` passes.
