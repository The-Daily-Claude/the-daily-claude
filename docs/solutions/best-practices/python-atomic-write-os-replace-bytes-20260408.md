---
title: "Python atomic write for text files: bytes mode + tempfile.mkstemp + os.replace"
category: technical
date: 2026-04-08
tags: [python, atomic-write, file-system, crlf, tempfile, os-replace]
related_commits:
  - 46c487b  # PR #4 squash
  - 7fcddfc  # atomic write helper in backfill-entities.py
---

# Python atomic write for text files without reformatting them

## Problem

The `scripts/backfill-entities.py` one-shot rewrites every
`content/entries/*.md` in place to insert an `entities: {}` line
into YAML frontmatter. The script's docstring promises "does NOT
reformat anything" — it's a pure field-presence fix. Two bot
review findings (Copilot P3, Gemini P2) fired on the naive v1
implementation for breaking that promise:

1. **Encoding / line-ending drift.** `Path.read_text()` uses
   `locale.getencoding()` and universal-newline mode — on a
   Windows machine it would pick up cp1252 and silently rewrite
   CRLF line endings to LF. On a macOS machine running in a
   non-UTF-8 locale the same thing happens. Neither case is
   "no reformatting".

2. **Non-atomic write.** `Path.write_bytes()` opens the file and
   streams the payload in; if the process is interrupted (SIGINT,
   power loss, disk full mid-flight) the entry file ends up
   truncated or partially written. For 256 entries the
   probability is low but nonzero, and git recovery is a last
   resort, not a first principle.

## What we learned

### Bytes mode for text files

For scripts that "just edit bytes in place" without caring about
semantic content, read and write **bytes, not strings**:

```python
def process_file(path: Path) -> str:
    raw = path.read_bytes()

    # Detect line-ending style upfront so we can preserve it.
    if b"\r\n" in raw:
        newline = b"\r\n"
    else:
        newline = b"\n"

    lines = raw.split(newline)
    # ... operate on bytes throughout ...
    _atomic_write_bytes(path, newline.join(lines))
```

Key points:

- `read_bytes()` / `write_bytes()` bypass locale and universal
  newlines entirely. The payload is the bytes, nothing else.
- Detect the source file's line-ending style **before** editing
  by scanning for `b"\r\n"`. Default to `b"\n"` if neither found
  (empty file edge case).
- Split on the detected separator, operate on `list[bytes]`, then
  join with the same separator. Round-trip is byte-identical
  except where you intended an edit.
- Startswith/lstrip comparisons use byte literals (`b"---"`,
  `b"entities:"`) — no accidental encoding coercion.

If your script genuinely cares about semantic text (e.g. counting
graphemes, normalizing Unicode), then use explicit
`open(path, "r", encoding="utf-8", newline="")` instead. But for
"insert a line, save the file" the bytes path is simpler and
safer.

### `tempfile.mkstemp` + `os.replace` for atomic publish

Match the Rust `state::atomic_write` contract from the same PR:
sidecar tempfile in the same directory, rename on success,
cleanup on failure.

```python
import os
import tempfile
from pathlib import Path


def _atomic_write_bytes(path: Path, payload: bytes) -> None:
    """Write ``payload`` to ``path`` atomically.

    Uses a same-directory sidecar tempfile + ``os.replace`` so a reader
    sees either the old contents or the new contents, never a partial
    write.
    """
    parent = path.parent
    fd, tmp_name = tempfile.mkstemp(
        prefix=f".{path.name}.tmp.",
        dir=parent,
    )
    try:
        with os.fdopen(fd, "wb") as f:
            f.write(payload)
        os.replace(tmp_name, path)
    except BaseException:
        try:
            os.unlink(tmp_name)
        except FileNotFoundError:
            pass
        raise
```

Why each piece:

- **`tempfile.mkstemp(dir=parent)`** — creates the sidecar in the
  same directory as the target so `os.replace` is a single POSIX
  `rename(2)` syscall (cross-device replaces aren't atomic). The
  `prefix` makes the sidecar debuggable (`.entry.md.tmp.abc123`).
- **`os.fdopen(fd, "wb")`** — wraps the raw fd that `mkstemp`
  returns, writes bytes, auto-closes on context exit. Using
  `open(tmp_name, "wb")` instead would work but leaks an open fd
  in the error path.
- **`os.replace(tmp_name, path)`** — POSIX guarantees this is
  atomic for in-directory replaces. A concurrent reader ever sees
  either the old contents or the new contents, never a partial
  write or a missing file.
- **`BaseException` handler** — catches SIGINT (`KeyboardInterrupt`)
  and `SystemExit` in addition to regular `Exception`. Best-effort
  cleanup of the sidecar on interrupt. The target is unchanged
  regardless — `os.replace` never ran.
- **`FileNotFoundError` suppression** — the sidecar may already be
  gone if the crash happened after `os.replace`; ignore that case.

## Smoke test

```python
d = tempfile.mkdtemp()
entries = os.path.join(d, "content", "entries")
scripts = os.path.join(d, "scripts")
os.makedirs(entries)
os.makedirs(scripts)
shutil.copy("scripts/backfill-entities.py", os.path.join(scripts, "backfill-entities.py"))

# LF + CRLF fixtures
with open(os.path.join(entries, "001-test.md"), "wb") as f:
    f.write(b"---\nid: test\ntitle: test\n---\n\nbody\n")
with open(os.path.join(entries, "002-crlf.md"), "wb") as f:
    f.write(b"---\r\nid: test2\r\ntitle: test2\r\n---\r\n\r\nbody\r\n")

# Run + verify
subprocess.run(["python3", os.path.join(scripts, "backfill-entities.py")], cwd=d, check=True)
lf = open(os.path.join(entries, "001-test.md"), "rb").read()
crlf = open(os.path.join(entries, "002-crlf.md"), "rb").read()
assert b"entities: {}" in lf and b"entities: {}" in crlf
assert b"\r\n" in crlf and b"\r\n" not in lf   # endings preserved
assert os.listdir(entries) == ["001-test.md", "002-crlf.md"]  # no sidecar litter
```

The first run modifies both. A second run produces
`modified: 0 skipped: 2` (idempotent).

## When to reach for this

- **Any one-shot script that edits tracked files in place.** The
  bot review bar for these is the same as for production
  code, and "my script trashed the repo" is a bad day.
- **Line-ending preservation matters.** YAML, TOML, Markdown,
  JSON, any config file format where CRLF vs LF matters to the
  next `git diff`.
- **Text files that aren't really text.** Binary-adjacent formats
  where partial encoding trips matter: SVG, XML, HTML with embedded
  binary.

## When NOT to reach for this

- **Truly write-once outputs.** If the target path is guaranteed
  fresh (e.g. unique filename in an output dir), plain
  `Path.write_bytes()` is fine.
- **Semantic text operations.** Grapheme counting, normalization,
  casefolding — use the string API with explicit encoding, don't
  reinvent it.

## Related

- `atomic-write-exclusive-link-based-20260408.md` — Rust
  equivalent with `link(2)` for race-safe create-new
- `atomic-write-monotonic-tempfile-suffix-20260408.md` — Rust
  `rename(2)`-based sibling
- `scripts/backfill-entities.py` — current implementation
