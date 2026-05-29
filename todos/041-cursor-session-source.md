---
title: "Add Cursor session-source ingestion"
priority: medium
status: pending
---

# Add Cursor Session-Source Ingestion

## Why

Cursor is one of the session sources called out for Trawl's post-Claude expansion, but its archive format is not yet represented in `session.rs`.

## What

Identify Cursor's local chat/session persistence format and either implement ingestion or document why it is not suitable for first-class Trawl support yet.

## Scope

- Locate Cursor chat/session storage on macOS.
- Determine whether records are JSONL, SQLite, workspace storage JSON, or another format.
- Capture redacted fixtures or schema notes.
- Add a parser/normalizer if the format is stable enough.
- Preserve source-model metadata once todo #037 lands.
- Update README and handoff.

## Acceptance

- The on-disk Cursor storage shape and path pattern are documented.
- If feasible, a fixture normalizes to `[HUMAN]`, `[ASSISTANT]`, and tool blocks where present.
- If not feasible, this todo records the blocker and the next best import route.
- `cargo test -p trawl` passes after any code changes.
