---
title: "Sync the Trawl README with the actual CLI contract"
priority: high
status: complete
---

# Sync the public README to the shipped CLI

This follow-up is complete.

The README now matches the actual `trawl` surface more closely:

- the tokeniser description now reflects placeholderised fields, the `entities` graph, and review flags
- scoring claims were removed
- friendly fake-name substitution is no longer described as a current `trawl` feature
- `--dry-run` is documented as a no-model-call planning pass, not an extraction preview
- `stats` is documented as a subcommand, not a `--stats` flag
- `stats` output is described conservatively
- the README now includes a direct note that leakage resistance still needs hardening

## Why this mattered

This is not cosmetic documentation drift. The README is the public interface to the project.

If it overclaims behavior:

- an HN post or README reader will form the wrong mental model of what Trawl does today
- users will try CLI invocations that do not exist
- the repo looks less trustworthy than the code deserves

The fix was to document reality instead of pretending the missing pieces already existed.

## What changed

- updated the tokeniser section to match the current Rust types and on-disk output
- updated the usage examples to match the real CLI
- added a "What lands on disk" section to make the current output contract explicit
- kept the hardening caveat around PII/confidential leakage in the README

## Verification

- [x] Every CLI example in `crates/trawl/README.md` matches `trawl --help`
- [x] README tokeniser description matches the current Rust types and written entry schema
- [x] README makes no claim about scoring unless the code actually grows a score field
- [x] README makes no claim that `--dry-run` previews extracted moments
- [x] README makes no claim that `stats` reports fields it does not currently print
