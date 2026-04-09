---
title: Your Historical Output Is Your Cheapest Regression Dataset
category: implementation
date: 2026-04-08
tags: [testing, regression, ground-truth, trawl, prompt-engineering, smoke-test]
related_commits: [f57348f]
---

## Problem

Rewriting a pipeline means you need to know whether the rewrite is as
good as what it replaced. For a ZFC pipeline the question is
specifically: *does the new extractor find the same quality of moments
the old one did?* — or at least, *would it have found the ones we
already published?*

The obvious answer is "set up a fixture", and the obvious failure mode
is "the fixture takes three days to build and biases toward what you
remember". During the PR #3 smoke-test phase we started down that path.
The question went: *which session files should I use as the smoke
set?* And the instinct was to ask the user to hand-pick some good ones.

The user's response was a rebuttal:

> "Why would you need me to pick jsonls? *You* have 200+ quality
> entries referencing the session they came from ?"

That sentence reframed the whole testing approach. We already had the
ground truth. It had been sitting in `content/entries/*.md` for weeks,
already curated, already published, already holding the session UUID
it came from as a front-matter field. The cheapest regression dataset
in the project was the project's own historical output.

## What we learned

**When you rewrite a pipeline that produces content, your previously
published content is the regression set.** You don't build a fixture;
you query the corpus.

Concretely, for Trawl:

1. **Every historical entry records its source.** The front matter of
   every `content/entries/NNN-*.md` file has a `source:` block with a
   `session_id` field pointing at the JSONL that produced it. That
   field existed from day one for attribution reasons; it turned out
   to also be the smoke-test index.

2. **Pick sessions, not fixtures.** A one-liner extracts the set of
   session UUIDs that have ever produced quality output:

   ```shell
   grep -h "^\s*session_id:" content/entries/*.md \
     | awk '{print $2}' \
     | sort -u
   ```

   Every UUID on that list is a session that the old pipeline
   extracted at least one postable moment from. Running the new
   pipeline over them and comparing outputs is a direct apples-to-
   apples regression test.

3. **Find the JSONL by UUID.** The session UUID is also the filename
   of the jsonl in `~/.claude/projects/<project>/<uuid>.jsonl`, so
   the lookup is a straight path join — no search, no index, no
   state.

4. **Define "pass" as recall against the known good set.** A new
   extractor passes if, for each session in the smoke set, it returns
   **at least one** draft whose quote overlaps meaningfully with the
   entry we already published from that session. *Overlapping* here
   is a loose metric: did the new pipeline catch the same beat? It
   does not need to produce the exact same title, the exact same
   category, or the exact same phrasing — we are testing recall of
   the joke, not string equality.

The PR #3 smoke run used this shape. On the "be harsh" first-draft
extractor, recall against the ground-truth set was roughly 1/3 (one
entry per session, usually not the beat we had historically
picked). On the rewritten prompt with the "session length is a poor
proxy" reframe and the self-check, recall shot to 12 entries across
the 3-session smoke set with 0 tokeniser failures in ~16 minutes wall
time — including the beats we had previously published and several
new ones on top.

## How to apply

- **Emit source attribution from day one.** Even if you don't plan to
  test against your own output, record what produced each artefact.
  It costs one extra field in a frontmatter block and buys you a
  ground-truth dataset the first time you rewrite the pipeline.
- **Grep the corpus when someone asks you to pick fixtures.** If the
  ask is "which sessions should I smoke-test against", the answer is
  probably "the union of every session my existing corpus cites". You
  are not picking. You are querying.
- **Don't demand string-equality recall.** A ZFC pipeline is
  non-deterministic (see
  `temperature-variance-acceptance-20260408.md`); demanding the new
  extractor reproduce the old title or the old category is a false
  positive generator. Match on beat, not on string.
- **Use recall plus wall-time as the smoke-test pair.** "Did it find
  the known beats, and did it do so in reasonable time" is a two-line
  report you can eyeball in seconds. More elaborate metrics come
  later — start with recall and latency.
- **Don't throw away the smoke set once you're happy.** Every new
  prompt edit (and remember, prompt edits *are* code edits in ZFC)
  should re-run the same recall check. The set lives as long as the
  corpus lives.

## Code pointer

- `content/entries/*.md` — every historical entry's front matter
  carries a `source.session_id` field that indexes the smoke-test set
- `crates/trawl/src/main.rs` — `derive_session_id(session_path)`
  writes the same field on freshly-extracted entries, closing the
  loop so tomorrow's corpus is also tomorrow's regression set
- `crates/trawl/prompts/extractor.md` — the `## Find them all`
  section that the recall check drove us to write in its final form
- PR #3 transcript — the "Why would you need me to pick jsonls?"
  moment that turned a fixture-building task into a one-liner grep

## Related

- `docs/solutions/design-decisions/two-stage-zfc-pipeline-in-practice-20260408.md`
  — the pipeline whose iterative prompt edits needed this regression
  set to validate
- `docs/solutions/design-decisions/temperature-variance-acceptance-20260408.md`
  — why recall on beats is the right metric instead of string equality
- `docs/solutions/design-decisions/prompt-hash-as-cache-invalidation-20260408.md`
  — the mechanism that re-runs every smoke-set session automatically
  on the next run after a prompt edit
