---
title: "Trawl refinements"
priority: medium
status: pending
---

# Trawl Refinements

## Session parser cleanup
- [ ] Filter system messages from extracted bodies (command-message, local-command-stdout, task-notification XML)
- [ ] Filter hook feedback messages ("Stop hook feedback:")
- [ ] Strip tool-result noise — keep only human-readable content
- [ ] Better handling of context compaction summaries (skip or summarize)

## Sliding window tuning
- [ ] Current window/step (8/4) extracts too many low-value windows from long sessions
- [ ] Consider: score a sample first, then expand windows around high-scoring areas
- [ ] Add `--max-entries-per-session` flag to cap extraction from mega-sessions

## Anonymization — go full ZFC

The regex pattern catalog is the wrong abstraction. Trawl should **learn**
what to anonymize as it runs, not ship with (or load) a pattern file.

- [x] Remove hardcoded personal patterns from source (a93b515)
- [ ] Delete the `anonymize.local.toml` file-loading scaffold — it's a
      transitional hack, not the end state
- [ ] Per-window anonymization via Haiku: "identify personal details in
      this exchange that should be anonymized" → list of spans → replace
      with seed-based random pool values (Alice, random city, etc.)
- [ ] Persist discovered patterns to `~/.trawl/cache.toml` (or similar)
      so repeat runs on the same content are deterministic and fast.
      The cache grows naturally — no manual setup, no init command.
- [ ] Credentials/IPs/paths/emails keep the deterministic regex pass
      (ZFC exception — needs 100% catch rate)
- [ ] First-run UX: if no sessions at default path, print helpful
      error with example invocation. No setup ceremony.
- [ ] Verify `needs_review()` still acts as a safety net for patterns
      the LLM missed

## Publishability
- [ ] README.md with the Reynolds reference
- [ ] License (MIT)
- [ ] `cargo publish` readiness — clean deps, no path dependencies
- [ ] Example output in repo
- [ ] GitHub repo: [org-1]/trawl (or standalone?)
