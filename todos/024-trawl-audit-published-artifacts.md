---
title: "Investigate: should `trawl audit` sweep downstream consumer outputs too?"
priority: medium
status: investigate
depends_on: 022
---

# Investigate: Auditing Downstream Consumer Outputs

`todos/022` introduces `trawl validate` over `content/entries/`. That
covers the source layer Trawl writes itself. Open question: do we also
need a separate sweep over artifacts that downstream consumers of
Trawl's output produce?

## What's downstream of an entry

- Any file a consumer generates by substituting placeholders back into
  human-readable form.
- Any rendered image, caption, or transcript derived from a tokenised
  entry.
- Anything that ever ends up in front of a reader.

The substitution layer (downstream consumers picking friendly aliases)
and human edits to those derived artifacts can both reintroduce PII
that the source-layer validator already cleared. A leak introduced
*after* tokenisation is still a leak.

## Open questions

1. **How often does post-tokenisation leakage actually happen** in
   practice? We don't know yet — #022 hasn't shipped. May turn out the
   downstream substitution is reliable enough that source-layer
   validation is sufficient. Worth measuring before building a separate
   audit subcommand.

2. **Where does the audit sit in the workflow?** Pre-publish gate? CI
   step? Pre-push hook? Manual `trawl audit` invocation? Each has
   different ergonomics and different failure modes.

3. **Do we need to OCR rendered images**, or is text-file scanning of
   the source derived files enough? Most renderers emit the slide text
   more or less verbatim, so probably enough — but a manual edit to an
   image (cropping, overlay text) could in principle leak. Probably an
   over-think for v1.

4. **What about remote published artifacts?** Once a derived artifact
   is on LinkedIn / X / Bluesky, scanning the local files is no longer
   sufficient. Remote audit is a much larger surface — separate tool?

5. **Does this collapse into a generalised `trawl validate --target
   <path>` flag** instead of a new subcommand? Probably, but the
   workflow story matters more than the CLI shape.

## What would tell us if this is needed

- Ship #022 and #023.
- Run a downstream consumer against 5–10 tokenised entries.
- Inspect the resulting derived artifacts for any literal PII.
- If zero leaks: source-layer validation is sufficient, drop this
  todo or keep it as a "nice to have."
- If any leaks: spec it out for real, with the answers from the open
  questions above.

## Next step

Defer until #022 is running with a real downstream consumer. Revisit
with real data, not speculation.
