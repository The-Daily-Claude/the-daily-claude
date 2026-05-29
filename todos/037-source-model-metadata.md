---
title: "Record original source-model metadata on extracted entries"
priority: medium
status: pending
---

# Record Original Source-Model Metadata

## Why

`trawl` now separates two concepts that used to be implicitly Claude-only:

- the **source session** being mined (`claude`, `codex`, future `gemini`, `opencode`, `pi`, etc.)
- the **extractor/tokeniser backend** used to mine it (`claude-code/...`, `codex/...`, `gemini/...`)

Generated entries currently record the extractor/tokeniser configuration only indirectly via state freshness. The entry itself does not preserve which assistant/model produced the original conversation.

That matters for later corpus analysis: a Codex-authored self-own and a Claude-authored self-own are editorially different even if Claude extracted both.

## What

Add source metadata to the entry frontmatter when determinable:

```yaml
source_model:
  family: codex
  model: gpt-5.4
```

Use `null` / omit the nested model when the archive format does not expose it.

## Scope

- Extend `Entry` frontmatter schema.
- Extend `PreparedSession` with source family/model fields.
- Populate Claude Code and Codex where the on-disk session metadata exposes enough information.
- Keep extractor/tokeniser backend signatures in state, not in entry frontmatter.
- Update README and tests.

## Acceptance

- Codex-normalized sessions produce entries with `source_model.family: codex` when detectable.
- Claude sessions produce `source_model.family: claude` or omit the field only with a documented reason.
- Existing entries without the field still parse.
- `cargo test -p trawl` passes.
