---
title: Variadic CLI Flags Eat Positional Arguments — Use stdin
category: technical
date: 2026-04-08
tags: [cli, claude-code, clap, stdin, subprocess]
related_commits: [f57348f]
---

# Variadic CLI Flags Eat Positional Arguments — Use stdin

## Problem

The Trawl extractor wants to invoke:

```
claude -p --model sonnet --allowedTools Read <huge prompt as a positional>
```

This silently misbehaves. `--allowedTools` is declared as a variadic
clap argument in current `claude-cli`, so it greedily consumes every
trailing token until it sees something that looks like another flag.
The "huge prompt" gets parsed as the second tool name. Claude then
runs with the wrong tool list and an empty prompt — and because there
is no parse error, the symptom is a confusingly empty/wrong response,
not a crash.

The first instinct is to wave the prompt across with `--prompt
<file>` or escape it harder. Neither helps: the variadic flag is
still adjacent to the positional, and the moment you add a new tool
the bug comes back.

## What we learned

Two complementary fixes, both shipped:

### 1. Pipe the prompt over stdin

```rust
let mut child = Command::new("claude")
    .arg("-p")
    .arg("--model").arg(model)
    .arg("--add-dir").arg(session_dir)
    .arg("--allowedTools=Read")
    .stdin(Stdio::piped())
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()?;

child.stdin.as_mut().unwrap().write_all(prompt.as_bytes())?;
let output = child.wait_with_output()?;
```

`stdin` is a different parsing channel — it cannot be eaten by a
variadic flag, full stop. The `claude` CLI happily reads its prompt
from stdin when no positional is supplied.

### 2. Use the `=`-form for the flag itself

`--allowedTools=Read` is a single token from clap's perspective. Even
if a future clap version stops being variadic, the equals form
removes the ambiguity at the source. Combine both: stdin makes the
positional disappear, `=`-form makes the flag self-contained, and the
two defences are independent.

A bonus subtlety: the extractor needs Claude's `Read` tool to access
the session JSONL by absolute path. Don't grant the whole tree —
pass `--add-dir <session_dir>` so Claude can `Read` that one path
without inheriting your home directory. The tokeniser stage doesn't
need any tools at all and **omits** `--allowedTools` entirely rather
than passing an empty value (which different CLI versions would
interpret differently).

## How to apply

- **Any time you spawn a CLI that has a variadic flag near a
  positional, use stdin.** The list of CLIs with this footgun is
  longer than you think — it's wherever the tool author wanted "pass
  N values" without a trailing terminator.
- **Always prefer `--flag=value` over `--flag value`** for flags that
  could be variadic, optional, or have ambiguous next-token semantics.
  The equals form is unambiguous in every clap-style parser.
- **Don't pass empty variadic values.** `--allowedTools=` and
  `--allowedTools` (no value) and `--allowedTools ""` are three
  different parses across different parser versions. If you don't
  want any value, omit the flag.
- **Grant least-privilege filesystem access via `--add-dir
  <path>`** rather than letting tools read your whole tree.
- **Smoke-test the subprocess invocation end-to-end on real input.**
  CodeRabbit flagged the stdin pipe as a potential concern in round 2;
  the response was "rejected — smoke-tested on 3 real sessions, the
  positional gets eaten, the stdin path works." Without that test,
  you cannot tell which side of the argument is right.

## Code pointer

- `crates/trawl/src/extractor.rs:62-104` — `run_claude` for the
  extractor: stdin pipe, `--allowedTools=Read`, `--add-dir`
- `crates/trawl/src/tokeniser.rs:91-126` — `run_claude` for the
  tokeniser: stdin pipe, no `--allowedTools` at all
