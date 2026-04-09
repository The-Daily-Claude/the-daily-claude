---
title: "fix: Address PR #1 code review findings from Codex and Gemini (trawl slice)"
type: fix
status: active
date: 2026-04-04
---

# fix: Address PR #1 Code Review Findings (Trawl Slice)

## Overview

Codex (gpt-5.4) and Gemini (2.5-pro) independently reviewed PR #1 and found several issues in `crates/trawl/`. This plan addresses the trawl subset: two critical PII leaks, a YAML injection vector, and a UTF-8 panic risk. Non-trawl findings are tracked separately.

## Problem Frame

The Trawl session miner had security gaps (PII in frontmatter, pre-anonymization scoring) and correctness issues (YAML injection via model-generated fields, UTF-8 byte-slicing panics). These need fixing before Trawl becomes publishable.

## Requirements Trace

- R1. No PII leaks in entry frontmatter or scoring pipeline
- R4. YAML frontmatter is safe from injection via model-generated fields
- R5. UTF-8 safe string operations throughout

## Scope Boundaries

- NOT fixing: prompt injection in the LLM pipeline (inherent to the design — entries are anonymized before any downstream consumer reads them)
- NOT fixing: hardcoded personal details in anonymize.rs (already tracked in `todos/001-trawl-refinements.md` as externalize-config work)
- NOT fixing: verbatim validator prefix fallback looseness (acceptable trade-off — false negatives caught by human review)
- NOT restructuring the `segment_exchanges` dead code (cleanup, not a review finding)

## Context & Research

### Relevant Code and Patterns

- `crates/trawl/src/main.rs` — orchestrator, builds Entry structs, calls scoring
- `crates/trawl/src/entry.rs` — Entry struct, hand-built YAML serialization, filename generation
- `crates/trawl/src/anonymize.rs` — Anonymizer with regex patterns, `needs_review` checker
- `crates/trawl/src/score.rs` — sends exchange text to `claude -p` for scoring

### Institutional Learnings

- Zero Framework Cognition: profanity scrubbing uses Haiku, not regex (see `docs/solutions/design-decisions/zero-framework-cognition-20260320.md`)
- Simplicity First: don't over-engineer (see `docs/solutions/process-patterns/simplicity-over-architecture-20260320.md`)

## Key Technical Decisions

- **Anonymize project_path in frontmatter**: Strip to last path segment (project name only) rather than storing the full filesystem path. This matches how `extract_project_name` already works for the `project` field — apply the same treatment to `source.project_path`.
- **Anonymize before scoring**: Run credential/path redaction (the cheap, regex-based pass) on exchange text before sending to the scoring model. The full anonymization (name replacement, etc.) still happens after scoring since it needs the entry seed.
- **YAML via serde_yaml**: Replace hand-built frontmatter interpolation with `serde_yaml::to_string()`. This handles escaping, quoting, and special characters correctly.
- **UTF-8 safe truncation**: Use `char_indices()` instead of byte indexing for the filename slug.

## Implementation Units

- [ ] **Unit 1: Anonymize project_path in entry frontmatter**

**Goal:** Prevent local filesystem paths from leaking into published entries.

**Requirements:** R1

**Dependencies:** None

**Files:**
- Modify: `crates/trawl/src/main.rs`
- Modify: `crates/trawl/src/anonymize.rs`
- Test: `crates/trawl/src/anonymize.rs` (inline tests)

**Approach:**
- In `main.rs:216`, apply `extract_project_name()` to `info.project_path` before storing in `Source::project_path` — it should store the clean project name, not the raw Claude Code path.
- Extend `needs_review()` in anonymize.rs to also check frontmatter-destined fields (title, category, project_path) for PII patterns, not just `anon_body`.

**Patterns to follow:**
- `extract_project_name()` already exists at `main.rs:413` — reuse it.

**Test scenarios:**
- Happy path: project_path `-Users-[user]-Projects-foo` becomes `foo` in frontmatter
- Edge case: project_path with single segment stays as-is
- Integration: `needs_review` flags entries where title contains a real name pattern

**Verification:**
- No entry frontmatter contains `/Users/` or home directory paths after trawl extraction

---

- [ ] **Unit 2: Pre-anonymization redaction before scoring**

**Goal:** Ensure credentials and filesystem paths are redacted before exchange text leaves the machine via `claude -p`.

**Requirements:** R1

**Dependencies:** None

**Files:**
- Modify: `crates/trawl/src/main.rs`
- Modify: `crates/trawl/src/anonymize.rs`

**Approach:**
- Extract a `redact_credentials(text: &str) -> String` public function from the first half of `Anonymizer::anonymize()` — the credential/IP/path/email regex replacements that don't need a seed.
- Call `redact_credentials()` on `exchange_text` in `main.rs` before it reaches `score::score_exchange()`.
- The full `anonymize()` (with name/location replacement) still runs after scoring as before.

**Patterns to follow:**
- The existing `CREDENTIAL_PATTERNS` static is already separated from personal-detail patterns — just expose it as a standalone function.

**Test scenarios:**
- Happy path: text with `sk-ant-api03-xxx` and `/Users/[user]/` gets both redacted
- Happy path: text without credentials passes through unchanged
- Edge case: text with IP address `192.168.1.1` gets redacted to `[ip-address]`

**Verification:**
- Exchange text passed to scoring contains no credential patterns or filesystem paths

---

- [ ] **Unit 3: Replace hand-built YAML with serde_yaml**

**Goal:** Eliminate YAML injection via model-generated fields (title, category, tags).

**Requirements:** R4

**Dependencies:** None

**Files:**
- Modify: `crates/trawl/src/entry.rs`
- Modify: `crates/trawl/Cargo.toml`

**Approach:**
- Add `serde_yaml` dependency.
- Create a `FrontmatterData` struct (with serde attributes) that mirrors the frontmatter fields.
- `to_markdown()` builds `FrontmatterData`, serializes via `serde_yaml::to_string()`, wraps in `---` fences, appends body.
- Remove the manual `format!()` interpolation and the `escaped_title` logic.

**Patterns to follow:**
- The `Entry` struct already derives `Serialize` — create a dedicated frontmatter view struct to control field ordering and naming.

**Test scenarios:**
- Happy path: entry with normal title/category serializes to valid YAML
- Edge case: title containing `'`, `"`, `:`, `\n` produces valid YAML (no injection)
- Edge case: tags containing YAML-significant strings (e.g., `"true"`, `"null"`) are properly quoted
- Happy path: round-trip — serialize then deserialize produces equivalent data

**Verification:**
- All existing entries can be regenerated without corruption; output parses as valid YAML

---

- [ ] **Unit 4: UTF-8 safe filename generation**

**Goal:** Prevent panics on non-ASCII titles when generating entry filenames.

**Requirements:** R5

**Dependencies:** None

**Files:**
- Modify: `crates/trawl/src/entry.rs`

**Approach:**
- Replace `slug[..60]` byte slice with `char_indices()` to find the char boundary at or before byte 60.
- The `is_alphanumeric()` filter already works correctly on chars — only the truncation is broken.

**Test scenarios:**
- Happy path: ASCII title truncates correctly at 60 chars
- Edge case: title with `e\u{0301}` (e + combining accent) truncates without panic
- Edge case: title with CJK characters (multi-byte) truncates at char boundary
- Edge case: title exactly 60 bytes needs no truncation

**Verification:**
- `filename()` does not panic on any valid UTF-8 input

## System-Wide Impact

- **Interaction graph:** Unit 2 changes the data flow in trawl's main loop (redact → score → anonymize → write).
- **Error propagation:** No change to trawl's error surface — these units only tighten safety of existing paths.
- **Unchanged invariants:** Trawl's scoring prompt, threshold logic, dedup, and window creation are not modified. Entry body content and anonymization behavior (beyond the credential pre-pass) are unchanged.

## Risks & Dependencies

| Risk | Mitigation |
|------|------------|
| serde_yaml output ordering differs from hand-built YAML | Existing entries are already written — only new entries use new format. Use `#[serde(rename)]` for field ordering if needed. |
| Pre-scoring redaction changes scoring behavior | Only credentials/paths are redacted (replaced with `sk-ant-***`, `/Users/[user]/`). The conversational content that scoring evaluates is preserved. |

## Sources & References

- Codex (gpt-5.4) review: `/tmp/codex-review.md`
- Gemini (2.5-pro) review: `/tmp/gemini-review.md`
- Related: `todos/001-trawl-refinements.md` (Trawl publishability)
- ZFC principle: `docs/solutions/design-decisions/zero-framework-cognition-20260320.md`
- Simplicity principle: `docs/solutions/process-patterns/simplicity-over-architecture-20260320.md`
