---
title: The Tokeniser Is The PII Boundary — It Must See Every User-Visible Field
category: design-decision
date: 2026-04-08
tags: [zfc, anonymization, pii, tokeniser, coreference, trawl]
related_commits: [f57348f]
---

## Context

The first-draft ZFC pipeline in PR #3 ran Sonnet over the session jsonl
to produce draft entries with four user-visible fields — `title`,
`category`, `tags`, `quote` — and then ran Haiku over *just the quote*
to scrub PII. The rationale seemed clean: the body is where the long
anonymizable text lives, so point the scrubber at the body.

Copilot's review flagged this as P1. The finding: the extractor is free
to emit a real name in the `title`, a real project in the `tags`, and a
real person's username in a punchy category label. If the tokeniser only
sees the quote, every one of those fields flows to disk verbatim —
including into the filename slug, because `NNN-slug.md` is derived from
`title`. A caller that searched the filesystem for a leaked username
would find it in `ls content/entries/` before they even opened a file.

This is the same class of bug as logging a parse error with a slice of
the raw extractor output: a PII "side channel" the scrubber never looks
at. The ZFC anonymization note from 2026-04-06 told us to let the model
do the scrubbing. It did not tell us *what the model has to be given*
to scrub correctly. This note fills that gap.

## The pattern

**When the tokeniser is the PII boundary, it must receive every string
that will become user-visible, in a single call, with coreference
consistency across all of them.**

Three properties are load-bearing:

1. **Every field goes in, every field comes out.** The tokeniser input
   is a JSON object with exactly the fields that will ship to disk.
   The output is a JSON object of the same shape with each field
   tokenised. No field is "trusted" or scrubbed by a separate pass.
   The Rust wrapper does not pre-sanitise anything — it hands the
   whole draft over and receives the whole tokenised draft back.

2. **Placeholder ids are consistent across all fields.** If Alice
   appears in `title` *and* in `quote`, both mentions become the same
   `#USER_001#`. The prompt makes this explicit: *"a placeholder id
   assigned in `title` MUST match any placeholder assigned for the
   same entity anywhere else in the draft. This coreference consistency
   is load-bearing: any downstream consumer reads the title alongside
   the body, and a mismatch would reveal the real identity by
   elimination."* Two different placeholders for the same person
   across `title` and `body` is a **de-anonymization attack by
   juxtaposition** — a reader can compare the title against the body
   and deduce the mapping.

3. **The registry safety-net validates every field the tokeniser
   returns, not just the body.** The main loop calls
   `reg.find_leaks(..)` on `title`, `category`, every `tag`, and
   `body`. If the registry has ever learned the literal "alice@acme.io"
   and it appears in *any* of those four places in *any* future draft,
   the entry is marked `needs_manual_review`. The scrubber and the
   backstop agree on the same boundary.

The concrete shape in Rust:

```rust
// tokeniser.rs
pub struct TokenisedEntry {
    pub title: String,     // tokenised title
    pub category: String,  // tokenised category
    pub tags: Vec<String>, // tokenised tags
    pub body: String,      // tokenised body
    pub entities: Value,   // sidecar placeholder graph
    pub needs_review: bool,
    pub review_reason: Option<String>,
}

pub fn tokenise_entry(draft: &DraftEntry, model: &str) -> Result<TokenisedEntry> {
    let draft_json = serde_json::to_string_pretty(&DraftPayload {
        title: &draft.title,
        category: &draft.category,
        tags: &draft.tags,
        quote: &draft.quote,
    })?;
    // One call. Full draft in. Full tokenised draft out.
    ...
}
```

And the invariant the prompt enforces:

> Every placeholder that appears in `title`, `category`, `tags`, or
> `body` MUST appear as a key in `entities`. Every key in `entities`
> MUST appear at least once somewhere in `title`, `category`, `tags`,
> or `body`. Placeholder ids MUST be consistent across all four fields.

## Why it matters

PII does not care which field you thought it would live in. The
tokeniser prompt can be as clever as you like about the body, but if
the Rust wrapper hands it only the body, everything in the other three
fields is *already a leak by the time the call returns*. The boundary
isn't where you put the anonymizer — it's where the data crosses from
"model output" to "disk". Whatever strings cross that line must all go
through the same scrub, together, or the scrub is incomplete.

The coreference requirement is the part that is not obvious. If you
had three separate scrubber calls — one per field — you would get
three independent placeholder namespaces, and a downstream reader who
sees `"#USER_001# said it"` in the title and `"#USER_003# said it"` in
the body can infer the identity by elimination: there is only one
person who said it. One call, one namespace, or you've invented a
de-anonymization side-channel.

This also explains why the tokeniser returns the **entity graph** as a
single sidecar map keyed on placeholder id. The graph has to be
global across all fields by construction — there is no other shape
that satisfies the invariant.

## Code pointer

- `crates/trawl/src/tokeniser.rs` — `TokenisedEntry` struct + the
  `tokenise_entry` function that passes the whole draft as one JSON
  blob
- `crates/trawl/prompts/tokeniser.md` — `## Input format` section
  (coreference requirement) and `## Output format > Hard rules` (every
  field must round-trip through placeholders)
- `crates/trawl/src/main.rs` — the `for draft in drafts` loop that
  writes **only** tokenised fields into `Entry` and calls
  `reg.find_leaks(..)` on every one of them before emitting
- PR #3 Copilot review comment on the early `main.rs` that flagged the
  title/tag/category leak path

## Related

- `docs/solutions/design-decisions/zfc-anonymization-20260406.md` — the
  decision that Haiku scrubs instead of regex; this note extends it
  with "scrub the whole draft, not just the body"
- `docs/solutions/design-decisions/two-stage-zfc-pipeline-in-practice-20260408.md`
  — the surrounding ZFC pipeline these scrubbing rules sit inside
- `docs/solutions/best-practices/error-chains-must-not-leak-pii-20260408.md`
  — the sibling rule for a different PII side-channel: error messages
- `docs/solutions/best-practices/length-aware-substring-registry-20260408.md`
  — the deterministic backstop that validates every tokenised field
