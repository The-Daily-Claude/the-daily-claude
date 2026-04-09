---
title: "Investigate: registry-driven re-tokenisation when registry grows manually"
priority: medium
status: investigate
depends_on: 022
---

# Investigate: Re-Tokenising Existing Entries After Registry Edits

`todos/022` makes the PII registry grow automatically from Haiku's
findings. Humans can also add to it manually — when they spot a leak,
when a new credential is issued, when a previously unknown name appears.
Open question: what should happen to *existing* entries when the
registry grows manually?

## The scenario

- Day 1: Trawl extracts entry #500. Haiku tokenises it. Registry has
  no record of "alex" being PII at that point.
- Day 7: A human notices that "alex" is a real coworker name and adds
  it to the registry by hand.
- Day 7+ε: Entry #500 still contains "alex" in its body, untouched.
- `trawl validate` (from #022) flags entry #500 as a leak. Good — but
  now what?

Today the human's only recourse is to either edit entry #500 by hand
(tedious for many entries, error-prone) or re-trawl the source session
(re-spends Sonnet, may pick different windows the second time, doesn't
compose with the state file's hash-skip logic).

## Open questions

1. **Is manual registry addition a frequent case** or an edge case?
   Don't know yet. May turn out Haiku catches enough on the first
   pass that manual additions are rare and hand-editing is fine. Or
   may turn out it's the dominant mode after a few weeks. Measure
   first.

2. **Should re-tokenisation operate on the existing body or re-extract
   from source?** Re-extraction is cleaner (one canonical pipeline)
   but expensive and risks drift. Body-only is cheaper and surgical
   but introduces a "second path" through the codebase.

3. **What about hand-edited entries?** If a human has manually
   tweaked the body of an entry between extraction and re-tokenisation,
   automated re-tokenisation could overwrite their edits. Need a
   detection mechanism (content hash on the body at write time?) and
   a confirmation gate.

4. **How does this interact with the Stage 1 prompt hash invalidating
   the state file?** A bumped extractor prompt re-trawls the session
   from scratch. A bumped tokeniser prompt could either also re-trawl
   from scratch, or just re-tokenise existing entries. Different cost
   profiles.

5. **Numbering conflicts.** If entry #500 already uses `#USER_001#`
   and re-tokenisation introduces another USER, Haiku assigns
   `#USER_002#` — local to the entry. Probably fine, but worth
   confirming any downstream consumer handles per-entry placeholder
   numbering correctly.

6. **What's the user-facing surface?** A `trawl retokenise <pattern>`
   subcommand? An automatic step that runs as part of `trawl validate`
   when it finds dirty entries? A `--fix` flag on `trawl validate`?
   Each shapes the workflow differently.

## What would tell us if this is needed

- Ship #022.
- Run manually for a week or two. Count how often `trawl validate`
  flags entries due to manual registry additions.
- If rare: hand-editing is fine, drop this todo.
- If frequent: spec out the subcommand with the answers from the
  open questions above.

## Next step

Defer until #022 has been in use for 1–2 weeks. Decide based on real
manual-edit frequency, not speculation.
