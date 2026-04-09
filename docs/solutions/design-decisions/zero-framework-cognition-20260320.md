---
date: 2026-03-20
problem_type: architecture_decision
severity: high
tags: [zfc, haiku, scoring, profanity, anonymization, yegge]
symptom: "Regex-based approaches couldn't handle edge cases, other languages, creative spellings"
root_cause: "Building frameworks around LLMs instead of letting the LLM be the framework"
---

## Zero Framework Cognition — Let the Model Do the Work

### Context
Steve Yegge's "Zero Framework Cognition" principle: stop building elaborate frameworks around LLMs. The model IS the framework.

### Applied Three Times This Session

**1. Trawl Scoring Engine**
- ❌ WRONG: Regex heuristics to detect frustration (ALL CAPS, profanity patterns, exclamation marks)
- ✅ CORRECT: Send the exchange to Haiku with 8 scoring dimensions, get structured JSON back
- Result: Haiku scored 12 exchanges from one session, found moments like "The Kumbaya Manifesto" (0.95 peak) that no regex would catch

**2. Profanity Scrubbing**
- ❌ WRONG: Regex dictionary (`fucking` → `effin'`, `shit` → `sh*t`, etc.)
- ✅ CORRECT: Haiku call with few-shot examples showing the desired replacements
- Why: Regex can't handle other languages (`putain de merde`), creative spellings, or context (don't censor "Shitake" or "assess")

**3. Template Selection for Memes**
- ❌ WRONG: Tag-matching function (entry tags → template suitability matrix)
- ✅ CORRECT: Haiku reads the entry + available templates, picks the best match with reasoning
- Why: The model understands joke structure (contrast, escalation, deadpan) in ways a tag matcher can't

### The Pattern
When you catch yourself building a classification system, a pattern matcher, or a decision tree around an LLM call — stop. Make it one LLM call with good examples instead.

### When This DOESN'T Apply
- Credential redaction (regex is faster, cheaper, and deterministic — you want 100% catch rate)
- Structural parsing (markdown → frontmatter extraction is deterministic)
- File I/O, API calls, image rendering (not cognition tasks)
