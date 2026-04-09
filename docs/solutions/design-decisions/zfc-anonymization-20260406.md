---
title: ZFC Anonymization — the Anonymizer Should Learn, Not Catalog
date: 2026-04-06
tags: [trawl, anonymization, zfc, design-decision]
---

# ZFC Anonymization

## Problem

Trawl's original anonymizer was a regex catalog:

```rust
let replacements = [
    (r"\bThomas\b", self.human_name.as_str()),
    (r"(?i)French-Israeli", self.nationality.as_str()),
    (r"(?i)\bnear Paris\b", &format!("near {}", self.city)),
    // ... hardcoded personal details ...
];
```

This had three problems:

1. **Source control leak**: personal patterns committed to git history, visible in any clone, any public mirror, any PR viewer. The anonymizer hid the user's identity in the *output* while exposing it in the *source*.
2. **Maintenance burden**: every new session topic potentially needs new patterns. The catalog grows forever and is never complete.
3. **Publishability barrier**: the tool couldn't be shared without leaking its owner's identity, or without a tedious config-externalization refactor that still required users to hand-write regex patterns.

## What we tried first

Intermediate fix: load patterns from a gitignored `anonymize.local.toml` file. This removed the source-control leak but still required:

- Users to know what to anonymize before they run
- A growing local pattern file that doesn't benefit from what the tool already sees
- A setup ritual (copy the `.example`, edit, then run)

It's better than hardcoding but it's still a catalog maintained by humans.

## The insight

The tool is already reading the content. It already has an LLM in the pipeline. The anonymizer should *discover* what to anonymize as it runs, not consult a static catalog.

## The approach

1. **Generic redaction stays deterministic** (credentials, IPs, file paths, emails). These need 100% catch rate and zero false negatives — ZFC exception per `zero-framework-cognition-20260320.md`.
2. **Personal patterns become ZFC**: for each exchange window, ask Haiku "what personal details in this text should be anonymized?" Get back a list of spans. Replace them with the per-entry random pool values (Alice, random city, etc.).
3. **Discovered patterns cache locally** at `~/.trawl/cache.toml` (or similar). Subsequent runs on the same content are deterministic and cheap.
4. **First-run UX is zero-ceremony**: just run `trawl <path>`. If the path doesn't exist, print a helpful error. Otherwise, scan + discover + anonymize + extract. No init command, no setup file to edit.
5. **`needs_review()` stays as the safety net** for patterns the LLM missed — the regex-based flag for identity/family/finance keywords keeps working.

## Why this is better

- **No PII in source or git history.** Ever. The cache is user-local and gitignored by default.
- **The tool learns.** Users don't need to know what anonymization means or what patterns to write. The first run populates the cache naturally.
- **Determinism is preserved** via the cache: repeat runs on the same content produce identical output.
- **ZFC-aligned**: the model IS the framework. No decision trees, no pattern catalogs, no classifier shims.
- **Shareable by default**: a fresh clone of Trawl has zero personal data anywhere. First run on the user's machine populates everything they need.

## What this obsoletes

The previous `todos/001-trawl-refinements.md` entry about externalizing patterns to a TOML config file is replaced. The answer isn't to externalize — it's to eliminate the catalog entirely.

## Scope

This applies to Trawl specifically. Any downstream consumer of already-anonymized entries is unaffected. The deterministic credential redaction layer is unchanged.

## Implementation status

- [x] Personal patterns removed from source (commit a93b515)
- [x] Intermediate step: load from gitignored `anonymize.local.toml` (committed as transitional scaffold)
- [ ] Per-window ZFC discovery via Haiku (tracked in `todos/001-trawl-refinements.md`)
- [ ] Learned pattern cache (tracked in `todos/001-trawl-refinements.md`)
- [ ] Delete the `anonymize.local.toml` loader once ZFC discovery ships

## Related

- `docs/solutions/design-decisions/zero-framework-cognition-20260320.md` — the broader ZFC principle
- `crates/trawl/src/anonymize.rs` — current implementation
- `todos/001-trawl-refinements.md` — implementation plan
