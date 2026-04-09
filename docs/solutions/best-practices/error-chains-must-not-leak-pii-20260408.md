---
title: Error Chains Must Not Leak PII — Log Structure, Never Content
category: technical
date: 2026-04-08
tags: [error-handling, pii, anonymization, observability, security]
related_commits: [f57348f]
---

# Error Chains Must Not Leak PII — Log Structure, Never Content

## Problem

The first version of Trawl's JSON extractor had this on parse failure:

```rust
return Err(anyhow!(
    "no candidate JSON array in extractor output: {slice:.200}"
));
```

The thinking was: a 200-byte preview helps debug bad model output.
The reality: the **whole purpose** of the tokeniser stage is that the
extractor's output may contain raw PII — names, paths, credentials,
the exact Doppler token that leaked twice in earlier sessions. Any
parse failure dumps that plaintext into stderr, into CI logs, into
the developer's terminal scrollback, into wherever stderr is being
captured. The error message is a covert exfiltration channel for the
exact data the rest of the pipeline is trying to scrub.

This is not theoretical. Copilot's first review pass on PR #3 caught
it and CodeRabbit flagged the same shape elsewhere. The fact that two
LLM reviewers independently found it suggests it's the obvious mistake
that humans miss because the slice is "just for debugging."

## What we learned

**Error messages on PII-bearing inputs must log only structural
information.** Byte ranges, candidate lengths, parser positions,
counts. Never a slice of the content itself.

```rust
match last_attempt_err {
    Some((start, end, length)) => Err(anyhow!(
        "no candidate JSON array in extractor output parsed successfully \
         (last attempt: byte range {start}..={end}, candidate length {length})"
    )),
    None => Err(anyhow!("no opening bracket in extractor output")),
}
```

This still tells you everything you need to debug — *where* the
candidate was, *how big* it was, *which candidates failed* — without
ever quoting a byte of model output. If you need the raw output for
deep debugging, write it to a temp file under a known-safe path with
restrictive permissions and reference that path in the error. **Don't
inline it.**

The principle generalises to any error path that crosses a trust
boundary:

- A parser whose input came from an LLM that may emit raw user data.
- A validator that received untrusted user input.
- A subprocess whose stderr you concatenate into your error chain
  (because that stderr may contain prompts or echoed file contents).
- Anywhere your `log::error!`, `tracing::error!`, or `eprintln!` could
  be tee'd into a CI artifact.

## How to apply

1. **Audit every `format!` and `with_context` in your error paths**
   that interpolates input. Replace `{input:.N}`,
   `{slice}`, `{stdout}` with structural facts: lengths, indices,
   counts, hashes if you must.
2. **Pin the contract with a test.** Construct an input containing a
   synthetic token shape (e.g., `sk-ant-XXXX` or `[deadbeef@host]`)
   plus a synthetic name, force the parse failure, and assert
   neither string appears anywhere in the resulting error chain. This
   is the test that survives a refactor.
3. **Treat subprocess `stderr` as untrusted** when you propagate it.
   Either redact it before joining, or replace the body with
   "subprocess wrote N bytes to stderr; see /tmp/<path>" and write
   the raw bytes to a path you control.
4. **The PII registry is the safety net.** If a redacted error string
   still slips a literal through, the cumulative literal hash set
   from `find_leaks` will catch it on the validate pass — but the
   defence in depth only works if you don't leak in the first place.

## Code pointer

- `crates/trawl/src/extractor.rs:107-160` — `parse_draft_array` with
  the structural-only error message and the doc-comment explaining
  *why* the slice is excluded
- `crates/trawl/src/tokeniser.rs:128-186` — `parse_tokenised` with the
  same shape and the same doc rationale
- `crates/trawl/src/registry.rs` — the validate-pass safety net that
  catches anything that escapes redaction

## Related

- `docs/solutions/design-decisions/zfc-anonymization-20260406.md` —
  the broader anonymization-via-Haiku design
- `docs/solutions/best-practices/length-aware-substring-registry-20260408.md` —
  the deterministic backstop that runs after every tokenisation
