---
title: "Trawl ZFC redesign — thin orchestrator + Sonnet/session + Haiku/anonymize"
priority: critical
status: pending
supersedes: [017, 020, 021]
reshapes: [019]
---

# Trawl ZFC Redesign

## Why

The 2026-04-07 Sonnet audit (6 sessions, 19 entries reviewed) empirically
validated the ZFC principle the project already documented:

- **Recall 63%** — Trawl missed ~37% of postable moments
- **Precision 63%** — ~37% of Trawl's outputs were noise or weak
- Every miss traced back to **framework cognition** Trawl had baked in:
  sliding windows, role labels, tool-result skips, narrative_completeness
  thresholds, overlap heuristics, regex anonymization, per-session caps
- A Sonnet pass given the raw session jsonl + a one-paragraph brief found
  every miss and rejected every false positive

The model is the framework. Trawl's ~500 lines of Rust pre/post-processing
are exactly the kind of brittle scaffolding ZFC says to throw away.

## Architecture

**Two** stages, both ZFC. Sonnet extracts the moment; Haiku tokenises any
PII into placeholders that **carry relational context**. The placeholders
ship as-is in the on-disk entry — downstream consumers do final
substitution at render time. No global registry, no stored PII map, no
Stage 3.

This means the same entry can be substituted differently for different
consumers: Tokyo on Tuesday, Berlin on Wednesday. The placeholder is the
canonical form on disk; the friendly alias is rendered just-in-time by
whoever reads the entry.

```
┌──────────────────────────────────────────────────────────────┐
│ Trawl (Rust, ~250 lines)                                     │
│                                                               │
│   1. walk ~/.claude/projects/                                │
│   2. consult state file: which sessions need trawling?       │
│   3. load PII registry (content/.pii-registry.json)          │
│   4. work-stealing pool of N workers                         │
│        each worker:                                           │
│          a. claude -p --model sonnet  ←── Stage 1            │
│             "<EXTRACTOR_PROMPT>"                              │
│             --add-dir <session.jsonl path>                   │
│             returns: [{title, category, tags, quote, why}, …]│
│                                                               │
│          b. for each draft entry:                            │
│               claude -p --model haiku  ←── Stage 2           │
│               "<TOKENISER_PROMPT>"                            │
│               draft → tokenised body + entity graph +        │
│                       transient {placeholder → real_value}   │
│                                                               │
│          c. registry.grow(transient_map)  ←── grow phase     │
│          d. registry.validate(tokenised_body)  ← validate    │
│             if any literal PII matches → needs_manual_review │
│                                                               │
│          e. write tokenised entry to content/entries         │
│             (placeholders ship as-is; downstream consumers   │
│              pick friendly substitutes at render time)       │
│          f. update state file                                 │
└──────────────────────────────────────────────────────────────┘
```

### Stage 1: Sonnet extractor (one call per session)

The Sonnet worker is given the absolute path to a session jsonl and reads
it directly via the Read tool. It handles parsing, role disambiguation,
window selection, scoring, and dedup — all implicitly. Returns a JSON array
of draft entries.

EXTRACTOR_PROMPT (sketch — full version lives in
`crates/trawl/prompts/extractor.md`):

> You are an editorial assistant for a meme publication about AI coding
> assistant failures, told from the assistant's self-deprecating
> perspective.
>
> Read the Claude Code session at `<ABSOLUTE_PATH>`. The file is JSONL —
> one JSON object per line, alternating user / assistant / tool messages.
>
> Your job: find every moment in this session that would make a good
> post. A good moment is:
> - Funny, ironic, self-deprecating, or dark-comic
> - Quotable on its own (a developer would stop scrolling)
> - Narratively complete (the joke lands without external setup)
> - Anchored in **verbatim** human or assistant text — never tool noise
>
> Solo-Claude monologues are valid IF the assistant tells a complete
> story. Tool results, build output, "let me wait", and operational
> chatter are NOT moments — they're the noise around moments. Thinking
> blocks ARE valid sources of quotes when the joke lives in Claude's
> internal reasoning.
>
> BE HARSH. Most exchanges are routine. Reserve "extract" for moments
> you'd genuinely stop and read.
>
> For each moment, return one JSON object with:
>   - title: catchy headline (≤80 chars)
>   - category: rage / comedy / existential / spectacular-failure /
>               wholesome / role-reversal / dark-comedy / meta / etc.
>   - tags: 2-5 short labels
>   - quote: the verbatim exchange, formatted as alternating
>            [HUMAN]: / [ASSISTANT]: / [TOOL_RESULT]: blocks. Include
>            enough surrounding turns for the joke to land. Do NOT
>            paraphrase. Do NOT truncate the punchline.
>   - why: one sentence on why this is postable
>
> Return a JSON array of all moments. Empty array if nothing in the
> session is postable.
>
> Output ONLY the JSON array. No prose, no markdown fencing.

What the model handles implicitly (no Rust code needed):
- Distinguishing real human input from tool_result blocks
- Catching the joke when it spans 5 turns or 50 turns
- Recognizing assistant monologues that tell a story alone
- Reading thinking blocks where the meta-jokes live
- Reading tool inputs (Write/Edit args) where Claude's published
  self-indictments live
- Skipping operational chatter without a hardcoded blacklist
- Deduping overlapping moments (one beat = one entry)
- Capping per-session output at "what's actually good"

### Stage 2: Haiku tokeniser (one call per draft entry)

Each draft from Stage 1 still contains real names, paths, credentials,
project names, places. Haiku replaces every PII span with a **placeholder
token** that carries an entity type and a numeric id, and emits a sidecar
**entity graph** describing the relationships between placeholders. The
graph never contains real values — just types and links.

The placeholder shipped on disk is the canonical form. Friendly
substitution (`#CITY_001#` → "Tokyo" or "Berlin") happens at render time
in whatever downstream consumer reads the entry.

TOKENISER_PROMPT:

> You are tokenising a draft entry. The extracted output is public; the
> source session is private. Real names, places, credentials, and
> internal identifiers must NOT appear in the on-disk entry.
>
> Read the draft below. Return a copy with every PII span replaced by
> a placeholder of the form `#TYPE_NNN#`, where:
>
> - `TYPE` is one of: USER, ORG, PROJECT, REPO, FILE, PATH, EMAIL,
>   PHONE, URL, HOST, IP, MAC, CITY, REGION, COUNTRY, ISLAND, ADDRESS,
>   CRED (api key / token / secret / private key block / JWT), ID (uuid
>   / account id / arn), DATE (specific dates that pin a real event),
>   TIME, OTHER
> - `NNN` is a stable 3-digit id assigned within this entry. The same
>   real-world entity must always get the same placeholder. Different
>   entities get different placeholders even if they share a type.
>
> **Diarisation:** if "Alice" and "alice" and "A." all refer to the
> same person, they all become `#USER_001#`. Use context to decide.
>
> **Relational context:** entities often have hierarchical or
> structural relationships that downstream consumers need to preserve
> a coherent substitution. Capture these in a sidecar entity graph.
> Examples:
>
>   - `#CITY_001#` is in `#REGION_002#` which is on `#ISLAND_003#`
>     which is in `#COUNTRY_004#`
>   - `#USER_005#` is affiliated with `#ORG_006#`
>   - `#REPO_007#` belongs to `#ORG_006#`
>   - `#FILE_008#` lives in `#REPO_007#`
>   - `#CRED_009#` belongs to service `#ORG_006#`
>
> Only include relationships that **matter for the joke** or that the
> reader would notice if substituted incoherently. Don't over-link.
>
> Be aggressive about anything token-shaped (long strings of base64,
> dp.sa.*, sk-*, AKIA*, eyJ*, etc.) — when in doubt, treat as `#CRED_*#`.
>
> Preserve everything else verbatim. Do not change the joke. Do not
> rewrite the prose.
>
> Output JSON:
> ```json
> {
>   "body": "...tokenised entry with #PLACEHOLDERS#...",
>   "entities": {
>     "#USER_001#": {"type": "USER"},
>     "#CITY_001#": {"type": "CITY", "in": "#REGION_002#"},
>     "#REGION_002#": {"type": "REGION", "in": "#COUNTRY_003#"},
>     "#COUNTRY_003#": {"type": "COUNTRY"},
>     ...
>   },
>   "needs_review": true/false,
>   "review_reason": "..." or null
> }
> ```
>
> If you encounter ANYTHING you're not sure about — a string that
> *might* be a credential, a name that *might* be a real person —
> placeholder it AND set `needs_review: true` with a reason.

This stage replaces the entire `crates/trawl/src/anonymize.rs` regex
machinery, the gitignored `anonymize.local.toml`, and the Alice/Bob
assignment logic. The model decides what's PII and what relationships
matter. No real values are ever stored.

The `entities` graph is written into the entry frontmatter so downstream
consumers can pick coherent substitutes. If the model marks
`needs_review: true`, the entry frontmatter gets `needs_manual_review: true`
and a human checks it before publish.

### Why no Stage 3 (deterministic substitution)

The earlier draft of this todo had a third stage that ran a deterministic
Rust pass to replace `#USER_001#` with "Alice" via a gitignored project-
wide registry. That's been **dropped**. Three reasons:

1. **Per-render substitution is more flexible.** A recurring location can
   be Tokyo in one render and Berlin in another. The placeholder is
   the canonical form; the alias is rendered just-in-time.
2. **No persistent PII map.** The Haiku stage never writes the
   `placeholder → real_value` map to disk. It exists only in memory
   during the tokenisation call, then evaporates. There is no
   `anonymize.local.toml` equivalent.
3. **Simpler.** No Stage 3 means no registry, no schema migration, no
   "what happens if two entries assign the same placeholder to
   different people" edge case. The downstream consumer is the
   substitution authority.

## State file (carry over from #018)

Trawl maintains `content/.trawl-state.json`, keyed by session path:

```json
{
  "/Users/[user]/.claude/projects/.../abc.jsonl": {
    "file_sha256": "…",
    "size_bytes": 184320,
    "mtime": "2026-04-06T19:25:12Z",
    "extractor_prompt_sha256": "…",
    "anonymizer_prompt_sha256": "…",
    "trawl_version": "0.4.0",
    "extracted_entry_ids": ["uuid-1", "uuid-2"]
  }
}
```

Decision matrix for whether to re-trawl a session:

| Condition | Action |
|---|---|
| File hash unchanged AND both prompt hashes unchanged AND version ≥ last | **skip** |
| Anything changed | **re-trawl from scratch** (no append-mode in v1; trust Sonnet) |
| Trawl version bumped through `rescore: true` migration | **re-trawl** |

A prompt edit invalidates everything that prompt produced. A session
content change invalidates that session. Simple, conservative, correct.

## What gets deleted

| File / construct | Status |
|---|---|
| `crates/trawl/src/session.rs::parse_session` | DELETE — Sonnet reads jsonl directly |
| `Role::User` / `Role::Assistant` enum | DELETE — Sonnet handles role semantics |
| `OPERATIONAL_PATTERNS` blacklist | DELETE |
| `create_sliding_windows` + `--window` / `--step` flags | DELETE |
| `window_is_worth_scoring` pre-filter | DELETE |
| `format_exchange` 2000-char truncation | DELETE |
| `score_exchange` + 8-dim rubric + `worth_extracting` threshold | DELETE |
| `--threshold` / `--model` (replaced by `--extractor-model` / `--anonymizer-model`) | RENAME |
| `dedup_overlapping_windows` + `overlap_ratio` | DELETE |
| Per-session cap `[3, 10]` | DELETE |
| `crates/trawl/src/anonymize.rs` (entire regex machinery) | DELETE |
| `anonymize.local.toml` (gitignored personal patterns) | DELETE |
| `Anonymizer::new` Alice/Bob assignment | DELETE — anonymizer prompt handles it |
| `needs_review` heuristic regex | DELETE — anonymizer flags |
| `--dump-scores` flag | DELETE — no scores |

What stays:
- Concurrency / work-stealing pool (now per-session, not per-window)
- `--concurrency` flag
- `count_existing_entries` → `max_existing_entry_number` (already fixed)
- The state file from #018
- Entry filename + Markdown writer
- `Entry` / `Source` / frontmatter structs (simplified — no `Scores`)

## Implementation phases

- [ ] **Phase 0:** lock the two prompts. Iterate them by hand on 3-5 known
      sessions until extraction quality matches the Sonnet audit. No Rust
      yet.
- [ ] **Phase 1:** rip out the framework-cognition modules listed above
- [ ] **Phase 2:** new `extractor.rs` — spawn `claude -p sonnet`, parse JSON
- [ ] **Phase 3:** new `anonymizer.rs` — spawn `claude -p haiku`, parse JSON
- [ ] **Phase 4:** new main loop — walk → state-check → pool → extract →
      anonymize → write
- [ ] **Phase 5:** state file (#018 logic) wired in
- [ ] **Phase 5b:** PII registry (`content/.pii-registry.json`) +
      `registry.grow()` + `registry.validate()` + `trawl validate`
      subcommand. Add `.pii-registry.json` to `.gitignore`.
- [ ] **Phase 6:** smoke test on the 6 audited sessions; compare against
      Sonnet audit's findings as ground truth. Confirm the registry
      catches the Doppler tokens that have leaked twice.
- [ ] **Phase 7:** full corpus run

## Open design questions

1. **Session size ceiling.** Multi-MB jsonl files exist. Sonnet's context
   is large but not infinite. Options:
   - (a) trust 200k context, fail loudly on overflow, defer chunking to v2
   - (b) chunk at conversation-arc boundaries (long time gaps, topic shifts)
   - (c) chunk at fixed message counts with overlap
   Recommend (a) for v1 — measure how often it fires before adding
   complexity.

2. **JSON output reliability.** Sonnet may wrap JSON in prose or markdown.
   Same risk as the HEAT orchestrator. Mitigation:
   - Robust extractor (find first `[`, last `]`)
   - One automatic retry with `Return ONLY the JSON array. No prose.`
   - On second failure, write the raw output to a debug file and skip the
     session. Log to stderr.

3. **Haiku per entry vs batch per session.** Batching N entries into one
   Haiku call would save subprocess startup, but risks the same
   cross-entry mixing problem we hit earlier. **Default: one Haiku call
   per draft entry.** Reconsider if it dominates wall time.

4. **Idempotent retokenisation sweep.** Should the tokeniser run once
   in-pipeline, or as a separate sweep over `content/entries/` that can
   be rerun when the prompt changes? Recommend **in-pipeline** for v1
   (simpler), with the option to add `trawl retokenise` as a
   maintenance command later. The bumped `tokeniser_prompt_sha256` in
   the state file already triggers re-trawl on prompt edits.

5. **`--allowedTools` for the extractor.** Sonnet needs `Read` to consume
   the session jsonl. Probably also needs nothing else. Try
   `--allowedTools "Read"` first; expand only if a smoke test fails.

6. **Cost / rate limits.** N parallel Sonnet calls × ~12k sessions on a
   subscription. Will it hit a daily token cap? Unknown until we run.
   Mitigation: incremental state means we can run in batches of N
   sessions per day if needed.

## What this supersedes

- **#017** Two-stage HEAT batch scoring — moot. Scoring lives in the
  Sonnet prompt; there's nothing to batch.
- **#020** Trawl tool-result mislabeling — moot. No Role enum. Sonnet
  reads the raw jsonl and understands what's a tool result.
- **#021** Trawl overlap dedup too loose — moot. No windows.

The 5 todos I was about to write (window cropping, monologue splitting,
thinking blocks, tool-input prose, narrative_completeness as veto) are
all moot for the same reason. Sonnet handles all of them implicitly.

## What this reshapes

- **#019** Anonymization hardening — **shape changes**. The list of
  formats and patterns moves from `anonymize.rs` regex into the
  ANONYMIZER_PROMPT prose. The `needs_manual_review` flag stays, but
  it's now set by the model, not by a regex heuristic. Still P0 because
  the leak risk is unchanged. Audit existing entries the same way.
- **#018** Incremental state + migrations — **stays**, with the
  decision matrix above (simpler than the original — no append-mode,
  just hash-based skip).

## Acceptance

- [ ] Ripping out the framework-cognition code reduces `crates/trawl/src/`
      from ~1000 lines to ~250
- [ ] Smoke test on the 6 audited sessions extracts ≥90% of the moments
      Sonnet identified manually
- [ ] Smoke test produces zero entries anchored on `[tool_result:` or
      `[tool: Bash]` text
- [ ] No entries with `narrative_completeness: 0.25`-equivalent slop
- [ ] Doppler-token-style leaks: zero across the smoke test
- [ ] State file correctly skips unchanged sessions on rerun
- [ ] Bumping either prompt hash triggers re-trawl on next run
- [ ] PII registry validation catches a synthetic leak in a test fixture
      (manually paste a known credential into a draft, run Stage 2 +
      validate, assert `needs_manual_review: true` is set)
- [ ] `.pii-registry.json` listed in `.gitignore`
- [ ] `trawl validate` re-runs validation over all existing entries
