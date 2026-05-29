---
title: "Add Gemini CLI session-source ingestion"
priority: medium
status: pending
---

# Add Gemini CLI Session-Source Ingestion

## Why

`trawl` can call Gemini as a backend, and Gemini subset runs have produced useful extractor/tokeniser output. It still cannot mine Gemini CLI's own archived conversations as source sessions.

## What

Find Gemini CLI's local session/history format, define a deterministic parser, and normalize it into the same role-labeled transcript shape used for Codex archives.

## Scope

- Locate Gemini CLI session/history files and document the path pattern.
- Capture redacted fixtures for representative user/assistant/tool events.
- Add source detection that avoids false positives on unrelated JSONL/text files.
- Normalize human/assistant/tool blocks.
- Preserve source-model metadata once todo #037 lands.
- Update README and handoff.

## Acceptance

- `trawl stats <gemini-session-root>` works if the archive is file-tree based, or the todo documents the alternate history storage shape.
- A fixture proves Gemini logs normalize to role-labeled transcript blocks.
- Non-Gemini sessions are not misdetected.
- `cargo test -p trawl` passes.
