---
title: "Tokeniser prompt: geographical example has country nested in island"
priority: low
status: resolved
source: coderabbit
depends_on: 022
resolved_at: 2026-04-08
resolved_by: prompts/tokeniser.md hierarchy fix
---

## Resolution (2026-04-08)

Inverted the relationship in `crates/trawl/prompts/tokeniser.md:122-125`:
`#COUNTRY_003#` now stands alone, `#ISLAND_004#` is `"in": "#COUNTRY_003#"`.
Matches real-world geography (islands inside countries) and the
example no longer teaches the tokeniser an upside-down hierarchy.

# Tokeniser prompt: geographical example has country nested in island

## Finding

`crates/trawl/prompts/tokeniser.md` includes a sample `entities` map
whose chain says `#COUNTRY_003#` is `"in": "#ISLAND_004#"`. Outside of
narrow edge cases, islands live inside countries, not the reverse.
The example teaches the model an upside-down hierarchy for no payoff
and adds mental friction on review.

## Location

`crates/trawl/prompts/tokeniser.md:122-124`

## Proposed fix

Either drop the island from the chain, or invert the relationship so
the country contains the island:

```diff
     "#CITY_001#":    {"type": "CITY",    "in": "#REGION_002#"}
     "#REGION_002#":  {"type": "REGION",  "in": "#COUNTRY_003#"}
-    "#COUNTRY_003#": {"type": "COUNTRY", "in": "#ISLAND_004#"}
+    "#COUNTRY_003#": {"type": "COUNTRY"}
+    "#ISLAND_004#":  {"type": "ISLAND",  "in": "#COUNTRY_003#"}
```

## Severity

Nitpick — cosmetic prompt example. The tokeniser will still work;
the example just looks wrong on read.
