---
title: "Finish PI support as both backend and session source"
priority: medium
status: pending
---

# Finish PI Backend and Session-Source Support

## Why

`trawl` already accepts `pi/<provider>/<model>` in the provider-qualified model surface and shells out to `pi`, but that path has only compile-time/unit-test coverage. Separately, PI session logs under `~/.pi/agent/sessions` are not yet normalized as input sources the way Codex archives are.

The Daily Claude is now generating real Pi-harness beats; Trawl should mine those logs directly instead of relying on ad-hoc delegate tooling.

## What

Finish PI support in two directions:

1. **Backend verification** — prove `--extractor-model pi/...` and/or `--tokeniser-model pi/...` works against a small fixture/session, or document the exact CLI incompatibility and fix it.
2. **Session-source ingestion** — detect PI session JSONL files, derive project/session metadata deterministically, and normalize them into the role-labeled transcript shape the extractor already consumes.

## Scope

- Inspect current `~/.pi/agent/sessions` JSONL schema and capture representative redacted fixtures.
- Add `session.rs` parser/detector tests for PI logs.
- Add normalization for human/assistant/thinking/tool blocks.
- Preserve original source metadata once todo #037 lands.
- Update README and handoff.

## Acceptance

- `trawl stats ~/.pi/agent/sessions` works as a read-only archive pass.
- A PI session fixture normalizes to `[HUMAN]`, `[ASSISTANT]`, and tool blocks where present.
- A dry-run over a PI session path chooses the PI normalization path without model calls.
- Backend invocation using `pi/...` is smoke-tested or the blocking CLI issue is documented in this todo.
- `cargo test -p trawl` passes.
