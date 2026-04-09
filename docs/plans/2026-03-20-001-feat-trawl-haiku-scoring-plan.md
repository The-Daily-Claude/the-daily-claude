---
title: "feat: Trawl Haiku scoring engine"
type: feat
status: active
date: 2026-03-20
origin: docs/plans/2026-03-19-001-feat-the-daily-claude-v1-pipeline-plan.md
---

# feat: Trawl Haiku Scoring Engine

## Overview

Wire Claude Haiku into the Trawl to score session exchanges on 8 dimensions.
Zero Framework Cognition approach (Yegge): no heuristic pre-filtering, no regex
classifiers. Send each exchange to Haiku, get structured scores back, extract
what scores above threshold.

## Approach

1. **Segment exchanges better** — current gap threshold (5) is too tight for long
   sessions. Use role-alternation patterns and topic shifts instead of index gaps.
2. **Score via Haiku** — each exchange → Haiku with structured output schema
   matching the `Scores` struct (8 floats 0.0–1.0 + title + category + tags)
3. **Filter** — extract if any dimension ≥ threshold (default 0.6)
4. **Anonymize** — run the existing anonymizer on extracted exchanges
5. **Write entry files** — markdown with YAML frontmatter, same format as
   compilation-migrated entries
6. **Deduplicate** — compare against existing entries in output dir by content hash

## Implementation

### Exchange segmentation (`session.rs`)
- [ ] Replace fixed-gap segmentation with sliding window approach:
  - Window of N turns (default 6-10) slides through the conversation
  - Each window is a candidate exchange sent to Haiku
  - Overlapping windows are fine — Haiku decides if the window is interesting
  - Skip windows that are mostly tool results or system messages

### Haiku scoring (`score.rs` — new)
- [ ] Call Claude Haiku via `claude -p` CLI (non-interactive, model flag)
  - Uses existing CLAUDE_CODE_OAUTH_TOKEN — no extra API key needed
  - Structured prompt: exchange text + scoring dimensions + output schema
  - Parse JSON response into `Scores` struct
- [ ] Prompt design (Zero Framework Cognition — let the model do the work):
  ```
  You are scoring a conversation exchange between a human and an AI coding
  assistant for potential inclusion in a meme/comedy compilation.

  Score each dimension 0.0 to 1.0:
  - remarkableness: how unusual or noteworthy
  - saliency: would someone stop scrolling
  - humor: is it funny
  - relatability: would devs using AI tools recognize this
  - emotional_intensity: strength of feeling (rage, triumph, despair)
  - quotability: can you pull a short punchy quote that stands alone
  - narrative_completeness: tells a story without external context
  - irony: gap between expectation and reality

  Also provide:
  - title: a catchy title for this moment
  - category: a short category label
  - tags: 2-5 tags from [rage, loop, failure, comedy, wholesome, existential,
    roast, meta, irony, callout, over-engineering, hallucination, ...]

  Respond with JSON only.
  ```
- [ ] Rate limiting: configurable delay between calls (default 100ms)
- [ ] Batch mode: process N exchanges concurrently (default 4)

### Entry serialization (`entry.rs` — extend)
- [ ] `Entry::to_markdown()` — serialize to markdown with YAML frontmatter
- [ ] Filename: `{number:03}-{slug}.md` with slug from title

### Deduplication (`dedup.rs` — new)
- [ ] Content hash: SHA-256 of the exchange text (pre-anonymization)
- [ ] Check against existing entries' source quotes
- [ ] Skip if substantially similar (fuzzy match on quotes)

### CLI integration (`main.rs` — extend)
- [ ] Wire scoring into the extraction pipeline
- [ ] Progress output: session name, exchanges found, entries extracted
- [ ] `--model` flag to override the scoring model (default: haiku)
- [ ] `--concurrency` flag for batch parallelism

## Acceptance Criteria

- [ ] `trawl ~/.claude/projects/path/to/session.jsonl` extracts entries from a real session
- [ ] Extracted entries have scores in frontmatter matching the 8 dimensions
- [ ] Anonymization applied to all extracted entries
- [ ] Deduplication prevents re-extracting known entries
- [ ] `trawl --dry-run` shows what would be extracted without writing or calling Haiku
- [ ] Running against the [project-beta] mega-session extracts recognizable moments

## Scope Boundaries

- NOT building: custom LLM client (use `claude -p` CLI)
- NOT building: fine-tuned scoring model
- NOT building: any downstream consumer of the extracted entries — Trawl just writes entries to disk
- NOT optimizing: token cost (Haiku is cheap enough for batch processing)

## Notes

- `claude -p "prompt" --model haiku` is the simplest possible LLM integration
- No API key management — inherits the user's Claude Code auth
- Future: could swap to direct API calls for speed, but CLI-first is simpler
