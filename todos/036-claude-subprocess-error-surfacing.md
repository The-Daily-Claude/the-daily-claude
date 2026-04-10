---
title: "Surface actionable Claude subprocess failures without leaking session data"
priority: high
status: pending
---

# Surface real Claude CLI failures in `trawl`

## Why this exists

A single-session smoke test on 2026-04-09 proved that the documented
build/test/read-only paths work, but the live extractor path still has
a diagnosability hole.

The observed run shape was:

- `cargo build --release -p trawl` — success
- `cargo test -p trawl` — 58/58 green
- `trawl stats <one-session.jsonl>` — success
- `trawl --dry-run <one-session.jsonl>` — success
- `trawl <one-session.jsonl> --content-root /tmp/... -o /tmp/...` — failure

What `trawl` surfaced:

- `claude exited with status Some(1):`

What a direct repro of the underlying CLI call surfaced:

- `You've hit your limit · resets 8am (UTC)`

That gap matters. The current error path makes a real operational
failure look like an opaque subprocess crash.

## Root of the issue

`extractor.rs` and `tokeniser.rs` only surface `stderr` on non-zero
Claude exits. In practice, user-facing Claude CLI failures can show up
without a useful `stderr` payload, or with terminal/UI formatting that
does not survive the current capture path in a readable way.

At the same time, we cannot simply dump all subprocess output into the
error chain, because extractor/tokeniser output may contain raw session
content and the repo already has an explicit rule against leaking PII
through errors and logs.

So the actual problem is:

- current diagnostics are too weak to explain quota/auth/tool failures
- naive "print stdout too" fixes risk leaking session-derived plaintext

## What needs to happen

1. Reproduce the failure path with a controlled fixture or with a known
   non-zero Claude CLI invocation.
2. Make `trawl` surface a useful operator message for pre-model failures
   like quota, auth, missing tool access, or CLI startup failure.
3. Keep the existing privacy boundary: no raw model output or
   session-derived plaintext should be appended to anyhow chains or logs.

## Likely fix shape

- capture both `stdout` and `stderr` on non-zero exits
- strip ANSI / terminal control sequences before inspection
- detect known safe-to-surface CLI failures (quota, auth, missing login,
  unsupported flags, permission failures) and promote them into a clear
  error message
- avoid dumping arbitrary extractor/tokeniser stdout when it may contain
  session content
- add tests around the sanitizer / classifier so future regressions do
  not silently reintroduce opaque `status 1` failures

## Acceptance criteria

- a quota-blocked Claude invocation produces an actionable `trawl`
  error that mentions quota/rate limit, not just exit code 1
- an auth/login failure produces an actionable `trawl` error
- the fix does not append raw extractor/tokeniser stdout to error chains
  when that stdout may contain session-derived plaintext
- `cargo test -p trawl` covers the new error-surfacing logic

## Out of scope

- switching away from Claude as the default backend
- changing the extractor/tokeniser prompts
- redesigning the PII registry or entry schema
