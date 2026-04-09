---
title: Rust 2024 Raw Strings — `r#"..."#` Collides With Reserved-Prefix Syntax
category: technical
date: 2026-04-08
tags: [rust, rust-2024, raw-strings, syntax, gotcha]
related_commits: [f57348f]
---

# Rust 2024 Raw Strings — `r#"..."#` Collides With Reserved-Prefix Syntax

## Problem

The tokeniser's tests embed JSON fixtures that include placeholder
tokens like `#USER_001#` and `#COUNTRY_003#`. The first draft used
the standard one-`#` raw-string delimiter:

```rust
let raw = r#"{"body": "in #CITY_001#", "entities": {"#CITY_001#": {"type": "CITY"}}}"#;
```

Under Rust 2024, this fails to compile with:

```
error: prefix `COUNTRY_003` is unknown
   --> crates/trawl/src/tokeniser.rs:148:60
    |
148 |   "#REGION_002#": {"type": "REGION", "in": "#COUNTRY_003#"}
    |                                              ^^^^^^^^^^^ unknown prefix
```

The parser sees the `"#` inside the literal and treats it as the
*end* of the raw string — because `r#"..."#` says "the string is
delimited by exactly one `#`." The trailing characters (`COUNTRY_003`)
then look like a fresh identifier with a `#`-reserved prefix, which
Rust 2024 reserved for future syntax extensions and now diagnoses as
a hard error rather than a warning. What used to be a parse oddity is
now a compile failure.

## What we learned

When your raw-string body contains `#` characters, you need **more
delimiter padding than you have `#`s in the body**. The mechanical
fix is two `#`s on each side:

```rust
let raw = r##"{"body": "in #CITY_001#", "entities": {"#CITY_001#": {"type": "CITY"}}}"##;
```

`r##"..."##` says "this string is delimited by exactly two `#`s,"
and a single `#` inside the body is just a character. If you ever
need to embed `"##` inside the literal, escalate to `r###"..."###`,
and so on.

The general rule, formalised: **the delimiter must have one more `#`
than the longest run of `#` immediately following a `"` inside the
literal.** A literal containing `"#"` needs `r##"..."##`. A literal
containing `"##"` needs `r###"..."###`.

This bites worse under Rust 2024 because reserved-prefix syntax was
promoted from a future-compat warning to a hard error. The same code
that compiled with a warning on 2021 is a build failure on 2024 — so
it's the kind of thing that bites the moment you bump your edition,
not the moment you write the code.

## How to apply

- **Default to `r##"..."##` for fixtures with `#` characters.** It
  costs one keystroke, removes a class of regression, and reads
  identically to `r#"..."#` to anyone scanning the test.
- **When you see "unknown prefix" on a string-literal-adjacent
  identifier**, it's almost always raw-string delimiter collision —
  not a missing import, not a macro typo. Look at the closing
  delimiter, not the offending identifier.
- **The fix scales:** if your body legitimately contains `"##`, jump
  to `r###`. The parser only ever cares about the longest consecutive
  `#`-after-`"` run inside the body.
- **Edition bumps reveal latent bugs.** When upgrading to a new
  edition, audit raw strings before you fight the diagnostics — the
  fix is mechanical but the error messages point at the wrong line.

## Code pointer

- `crates/trawl/src/tokeniser.rs:180-220` — test fixtures using
  `r##"..."##` for JSON bodies containing `#PLACEHOLDER#` tokens
- `crates/trawl/src/extractor.rs` — sibling tests that don't contain
  `#` characters and use `r#"..."#` happily, for contrast
