# Entry `id:` is UUID7; filename integer is display ordering

**Date:** 2026-04-09
**Context:** Entry ID collision fix during 2026-04-09 cleanup
**Category:** design-decisions

## The schema

Each entry in `content/entries/` has two distinct identifiers:

1. **Filename integer prefix** — e.g. `213-progress-now-it-fails-loudly.md`. Used for ordering / display / human-friendly reference. Not stable across time.
2. **`id:` field in YAML frontmatter** — a UUID7 string like `019d6419-6ca5-7f62-bed7-e42f2f244cf7`. Stable, globally unique, never reused.

These serve **different purposes**:

| Purpose | Filename integer | UUID7 `id:` |
|---|---|---|
| Human-readable ordering | yes | no |
| Globally unique | no (collisions possible) | yes |
| Stable under rename | no (changes) | yes |
| Downstream linking | don't use | use this |

Older entries from the original Compilation (`#001`–`#128`) also carry an explicit `number:` field in frontmatter that mirrors the filename integer. Newer ZFC-era entries (`#211+`) omit `number:` — the filename prefix is the sole source of the display integer. Both schemas coexist. Don't try to normalize them without a specific reason.

## Why this matters for collision fixes

When the ZFC trawl extracts entries and assigns integer IDs starting from `next_number`, two concurrent runs can pick the same integer. The previous iteration of Trawl was vulnerable to this race. PR #4's `atomic_write_exclusive` + probe-and-retry closes the race for new runs (see `docs/solutions/best-practices/atomic-write-exclusive-link-based-20260408.md`). For **existing** collisions on disk, the fix is to rename the filename — incrementing the integer — **but leave the UUID `id:` field alone.**

A new session doing a collision fix might be tempted to update the `id:` field to match the new filename integer. **Do not do this.** The UUID7 is the stable identifier used by:

- Downstream tooling
- Cross-entry references in `related_entries:` fields
- `content/used.jsonl` publication tracking
- Any future Linear-pipeline mapping

Overwriting it would break those links silently.

## The 2026-04-09 collision fix (near-miss)

Ten tracked-vs-untracked ID collisions in the #213–#227 range, plus a dual-untracked `#297`, were resolved by renaming the loser to `#312`–`#321`:

- **Tracked wins.** The entry already in git has the older claim to the integer.
- **Loser renamed** via `mv` (since it was never in git). No `git mv` needed.
- **Filename integer** updated to the next free slot (starting at `#312`, after the previous max `#311`).
- **UUID `id:` frontmatter preserved as-is.**

The collision-fix subagent was initially instructed to *"update the `id:` field to match the new integer if present."* When it read the existing entries, it saw the UUID7 schema and caught its own error — refused the instruction, left the UUIDs untouched, and flagged the mismatch in its return summary. This solution doc exists so the next session doesn't repeat the near-miss without the self-catch.

## Practical rules

1. **Filename integer is display metadata.** Rename freely when collisions happen.
2. **UUID `id:` is the canonical identifier.** Never rewrite it. Never generate it outside Trawl's entry creation path.
3. **Cross-entry references use UUIDs, not filename integers.** `related_entries:` fields point to UUIDs.
4. **`content/used.jsonl` tracks UUIDs**, not filename integers, so publication state survives renames.
5. **Collision fixes touch filenames only.** No frontmatter edits beyond what's explicitly justified by the task.
6. **`number:` field** (old Compilation entries only) mirrors the filename integer and can be updated alongside a rename. Skip it for ZFC-era entries — they don't have the field.

## Why UUID7 specifically

UUID7 is time-ordered: two IDs generated close in time sort close together. This means **sorting by UUID approximately sorts by creation time** — giving time-ordering for free without a separate timestamp field. The filename integer is a coarser human-friendly proxy for the same ordering, useful for cross-referencing in conversation (*"check entry 213"*) but not as a stable key.

## References

- Commit `2b79256` (2026-04-09 cleanup — 10 collision renames, UUIDs preserved)
- `crates/trawl/src/main.rs` — `run_trawl` function, `next_number` + `atomic_write_exclusive`
- `docs/solutions/best-practices/atomic-write-exclusive-link-based-20260408.md` — PR #4 race fix for new runs (prevents future collisions of this class)
