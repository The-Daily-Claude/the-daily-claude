---
title: "Anonymization hardening — credentials, identifiers, project names"
priority: P0
status: reshaped
reshaped_by: 022
---

> **Reshaped 2026-04-07 by `todos/022-trawl-zfc-redesign.md`.** The
> regex pattern list below is no longer the implementation target — it
> moves into the **ANONYMIZER_PROMPT** prose for the Haiku scrubber in
> Stage 2 of ZFC Trawl. Patterns become the model's instructions, not
> Rust regex tables. The leak risk is unchanged and the priority stays
> **P0** (still required before any public-repo milestone). The audit /
> backfill steps remain valid — re-run them against existing entries
> after the ZFC pipeline lands.
>
> **The Doppler leak hit twice in two runs (#222 yesterday, #292 today).**
> Token already rotated. Anonymization is the most-load-bearing piece
> of the rebuild.

# Anonymization Hardening

## Why P1

Entry #222 (`What Would You Like Me to Do With This Doppler Token?`) shipped
to `content/entries/` with a **literal Doppler service account token in the
body** (matching the `dp.sa.[A-Za-z0-9_-]{20,}` pattern — the actual token
has been rotated and redacted from repo history), plus launchd plist names,
internal project names, and operational fingerprinting (loop intervals, red
team cadence, tmux script paths). `needs_manual_review` was `false` — the
safety net didn't catch any of it.

The deterministic regex in `anonymize.rs` only knew about a small set of
formats. Anything outside that set sails through. The Haiku scoring prompt
also doesn't currently mention anonymization at all, so the model isn't
participating in the safety check.

The repo will go public. The current state is unacceptable.

## Scope

Two paired changes, both required:

### 1. Regex pass — expand the deterministic redactor

Add patterns to `crates/trawl/src/anonymize.rs` for the formats below. All of
these are easy to detect and easy to anonymize — there's no excuse for missing
them.

**Cloud / SaaS credentials:**
- Doppler: `dp\.(sa|st|pt|ct|scim|audit|st|gw)\.[A-Za-z0-9_-]{20,}`
- AWS access keys: `AKIA[0-9A-Z]{16}`, `ASIA[0-9A-Z]{16}`
- AWS secret keys: `[A-Za-z0-9/+=]{40}` (context-gated to avoid false positives)
- AWS ARNs: `arn:aws[a-zA-Z-]*:[a-z0-9-]+:[a-z0-9-]*:[0-9]{12}:[A-Za-z0-9:_/.+=@-]+`
- AWS account ids: bare `[0-9]{12}` adjacent to "account", "AWS", or `arn:`
- GitHub tokens: `gh[pousr]_[A-Za-z0-9]{36,}`, fine-grained `github_pat_[A-Za-z0-9_]{82}`
- GitLab tokens: `glpat-[A-Za-z0-9_-]{20,}`
- Slack tokens: `xox[baprs]-[A-Za-z0-9-]{10,}`
- Stripe keys: `sk_(live|test)_[A-Za-z0-9]{24,}`, `pk_(live|test)_[A-Za-z0-9]{24,}`, `rk_(live|test)_[A-Za-z0-9]{24,}`
- OpenAI: `sk-[A-Za-z0-9]{20,}`, `sk-proj-[A-Za-z0-9_-]{20,}`
- Anthropic: `sk-ant-[A-Za-z0-9_-]{20,}`
- Google API keys: `AIza[0-9A-Za-z_-]{35}`
- Linear: `lin_api_[A-Za-z0-9]{40}`, `lin_oauth_[A-Za-z0-9]{40}`
- Sentry DSNs: `https://[a-f0-9]+@[^/]+/[0-9]+`
- npm tokens: `npm_[A-Za-z0-9]{36}`
- PyPI tokens: `pypi-[A-Za-z0-9_-]{50,}`
- HuggingFace: `hf_[A-Za-z0-9]{34,}`
- Cloudflare: `[A-Za-z0-9_-]{40}` adjacent to "CF_API_TOKEN" / "cloudflare"
- Generic JWTs: `eyJ[A-Za-z0-9_-]+\.eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+`
- Private key blocks: `-----BEGIN [A-Z ]+PRIVATE KEY-----[\s\S]+?-----END [A-Z ]+PRIVATE KEY-----`
- SSH known formats: `ssh-(rsa|ed25519|dss) [A-Za-z0-9+/=]+`

**Identifiers worth scrubbing even if not strictly secret:**
- UUIDs in identifiers (`[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}`)
  → `[uuid]`
- IPv4 / IPv6 addresses → `[ip-address]`
- MAC addresses → `[mac-address]`
- Email addresses → `[email]` (already exists, audit coverage)
- Phone numbers (E.164 + common formats) → `[phone]`
- Credit card numbers (Luhn-validated) → `[card]`
- Hostnames matching `*.internal`, `*.lan`, `*.local`, `*.tailnet`, `*.ts.net`
- Bearer tokens in headers: `Authorization:\s*Bearer\s+[A-Za-z0-9._-]+`
- Basic auth in URLs: `https?://[^:/@]+:[^@/]+@`

**Project / org names from this codebase specifically:** the actual list
of tokens lives in `crates/trawl/anonymize.local.toml` (gitignored) — see
the accompanying `.example` file for the schema. Coverage categories:

- Org names and aliases
- Side-project codenames
- `com.*` launchd service prefixes → `[launchd-service]`
- Internal script paths (e.g. `~/bin/*`) → `[script]`
- The home project-tree path prefix → `[project-path]`

Do NOT enumerate the actual tokens in this public todo file — **the list
is the leak.** Keep the concrete strings in the gitignored TOML and only
describe the shape here.

### 2. Haiku scoring prompt — make the model a participant

The current scoring prompt asks Haiku to rate dimensions. Add a section that
asks Haiku to **also flag any anonymization failures it sees in the window**:

> SAFETY CHECK: Before scoring, scan the exchange for any of:
> - API keys, tokens, secrets, or credentials of any kind
> - Real names, emails, phone numbers, addresses
> - Internal project names, hostnames, IPs, account IDs
> - Anything that looks like a unique identifier tied to a real person,
>   org, or system
>
> If you find any, set `needs_manual_review: true` in the JSON and add a
> `review_reason` field describing what you saw (without copying the secret).

This is the ZFC layer on top of the regex — the model catches what regex
can't, and the regex catches what the model misses. Belt and suspenders.

## Implementation

- [ ] Audit existing patterns in `anonymize.rs`, list current coverage
- [ ] Add the cloud/SaaS credential patterns above as a single `CREDENTIAL_PATTERNS`
      array, with one regex + replacement per kind
- [ ] Add identifier patterns to a separate `IDENTIFIER_PATTERNS` array
- [ ] Move project/org name list to `anonymize.local.toml` under a new
      `[project_names]` section
- [ ] Test fixtures: one `.jsonl` per credential type with a known token,
      assert it gets redacted
- [ ] Update the Haiku prompt template with the SAFETY CHECK section
- [ ] When Haiku flags `needs_manual_review: true`, write the entry with
      that flag honoured (today the writer overrides it from the Rust side)
- [ ] **Backfill**: run the new redactor across all 229 existing entries and
      flag any that contain matches as `needs_manual_review: true`. Don't
      auto-edit bodies — just flag.
- [ ] Audit `~/trawl-scores-fullrun.md` (and any other dump files) for the
      same patterns; redact or delete

## Acceptance

- [ ] Test fixture for every pattern in the list above passes
- [ ] Backfill across existing entries surfaces every token-shaped string
      (manual spot-check on a sample of 20)
- [ ] Entry #222 specifically gets flagged on backfill
- [ ] Haiku prompt update verified by feeding it a synthetic window with a
      fake token and confirming it sets `needs_manual_review: true`
- [ ] No regression: clean entries still extract cleanly, no false-positive
      review flags on the existing safe corpus
