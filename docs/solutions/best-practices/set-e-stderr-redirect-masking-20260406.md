---
title: set -e Plus 2>/dev/null Silently Masks Failures
date: 2026-04-06
category: docs/solutions/best-practices
module: shell-scripts
problem_type: best_practice
component: tooling
severity: medium
applies_when:
  - Writing bash scripts that use `set -e` for fail-fast semantics
  - Tempted to silence noisy stderr from a tool with `2>/dev/null`
  - Debugging a script that "succeeds" but produces wrong output
tags: [bash, set-e, error-handling, debugging, shell]
---

# set -e Plus 2>/dev/null Silently Masks Failures

## Context

A bash script used `set -e` for fail-fast semantics and also redirected
stderr from a noisy renderer to `/dev/null` to keep the terminal clean.
When the renderer started failing, the script kept "succeeding" — the
non-zero exit code still propagated through `set -e`, but the *reason*
the command failed was gone, and intermediate commands that produced
empty/garbage output instead of erroring kept the pipeline alive long
enough to write a broken artifact.

## Guidance

**Do not blanket-redirect stderr to `/dev/null` in scripts that use
`set -e`.** The two patterns work against each other:

- `set -e` relies on you noticing *which* command failed and *why*.
- `2>/dev/null` deletes the only signal you have for the "why".

If a tool is too noisy, prefer one of:

1. **Capture, then conditionally show.** Tee stderr to a file and only
   print it on failure:

   ```bash
   if ! my-tool > out 2> err; then
     cat err >&2
     exit 1
   fi
   ```

2. **Filter, don't drop.** Use `grep -v` or `sed` to remove just the
   known-noisy lines, leaving real errors visible.

3. **Drop only at the call site that you have personally verified is
   safe**, never as a default at the top of the script. And add a comment
   explaining what you're hiding and why.

## Why This Matters

`set -e` gives you two things: process termination on failure, and a
visible failure signal so you can fix it. Silencing stderr keeps the first
and removes the second. You end up with a script that fails *eventually*,
several commands downstream of the real cause, with no breadcrumbs to find
your way back. Debugging time goes from seconds (read the error) to
minutes or hours (bisect the pipeline by hand).

The general principle: **never delete information you might need to debug
the script you are writing.** Disk is cheap, terminal scrollback is cheap,
your time is not.

## When to Apply

- Any new bash script using `set -e` / `set -euo pipefail`
- When auditing existing scripts that "mysteriously" produce wrong output
- Before adding `2>/dev/null` to any command — ask "what error am I
  promising will never matter?"

## Examples

**Smell:**

```bash
set -e
carbon-now "$slide" -o "$out" 2>/dev/null   # silenced — failures invisible
magick "$out" ...                            # operates on missing/empty file
```

**Fix:**

```bash
set -e
carbon-now "$slide" -o "$out"   # let stderr through
magick "$out" ...
```

Or, if the tool really is too noisy:

```bash
set -e
if ! carbon-now "$slide" -o "$out" 2> /tmp/carbon.err; then
  cat /tmp/carbon.err >&2
  exit 1
fi
```

## Related

- `tooling-issues/carbon-consistent-frame-20260406.md` — found alongside the Carbon frame fix in PR #2
