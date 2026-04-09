---
title: "Extractor prompt defers credential redaction to later pass"
priority: medium
status: resolved
source: coderabbit
depends_on: 022
resolved_at: 2026-04-08
resolved_by: extractor.md inline credential directive (PR #3)
followup: 032-extractor-regex-credential-prepass
---

## Resolution (2026-04-08)

The "Leave real names, paths, credentials in place" directive is gone.
`crates/trawl/prompts/extractor.md:138-144` now carries an explicit
**"Credentials are the one exception"** block that requires the extractor
to redact any API key, token, session cookie, bearer header,
private-key block, connection string, password, or other secret using
`[REDACTED_SECRET]` before returning JSON. Titles and tags must also
stay clear of credential-shaped strings.

Defense-in-depth downstream is unchanged: tokeniser + Haiku + length-aware
registry all scan the draft body again, so a single stage regression
cannot leak a credential to disk.

**Deferred (new todo #032):** the original coderabbit finding also
suggested a regex pre-pass that scrubs obvious credential shapes
*before* the content reaches Sonnet. That is genuinely additional
hardening (three independent layers instead of two) but adds real
complexity — pattern choice, false-positive handling, test coverage,
where the pass lives in the pipeline. Tracked separately; not blocking.

# Extractor prompt defers credential redaction to later pass

## Finding

`crates/trawl/prompts/extractor.md` currently tells the extractor to
"Leave real names, paths, credentials in place. A later pass handles
tokenisation." This is defense-in-depth anti-pattern: if the tokeniser
or registry stages fail, regress, or are skipped, credentials can enter
the published pipeline. Extraction should immediately redact obvious
credential shapes (API keys, tokens, passwords, private keys,
connection strings) even though non-sensitive tokenisation (names,
paths, orgs) stays in the later pass.

## Location

`crates/trawl/prompts/extractor.md:135-136`

## Proposed fix

Replace the existing directive with something along the lines of:

```diff
-- Do NOT anonymise at this stage. Leave real names, paths, credentials
-  in place. A later pass handles tokenisation.
+- Immediately redact credentials (API keys, tokens, passwords, private
+  keys, connection strings) from quotes using the placeholder
+  `[REDACTED_CREDENTIAL]`. Real names, paths, and organisations may be
+  left for the later tokenisation pass, but credentials must never
+  leave the extraction stage in plaintext.
```

Any change here should be cross-checked against
`todos/022-trawl-zfc-redesign.md` and
`todos/019-anonymization-hardening.md` so the redaction vocabulary
matches what the tokeniser + registry expect downstream.

## Severity

P2 — defense-in-depth hardening. The extractor is upstream of the
tokeniser and registry, which already cover credentials, but relying on
a single downstream stage for a security-critical property is fragile.
Not P1 because the current pipeline does pass credentials through
tokeniser + Haiku anonymiser before anything is published.
