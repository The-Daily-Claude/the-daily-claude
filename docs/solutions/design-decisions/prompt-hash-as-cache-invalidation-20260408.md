---
title: Prompt Hash As Cache Invalidation Key — Prompts Are Content-Addressed Code
category: design-decision
date: 2026-04-08
tags: [zfc, caching, prompt-engineering, state, trawl, content-addressed]
related_commits: [f57348f]
---

## Context

Trawl runs over `~/.claude/projects/`, which holds hundreds of session
JSONL files and grows every day. Running Sonnet and Haiku over every
session on every invocation is both slow and expensive, so there is a
state file at `content/.trawl-state.json` that remembers which sessions
have already been trawled. Any session whose hash matches the state
record is skipped.

The naive cache key is `file_sha256(session.jsonl) + trawl_version`.
That handles "did the session change" and "did the binary change", but
it silently misses the case we iterate on most often: *the prompts
changed*.

Under ZFC (see `two-stage-zfc-pipeline-in-practice-20260408.md`) the
prompts *are* the program. Editing `prompts/extractor.md` is a code
change — the kind that should invalidate every previously-cached output
and force a re-trawl. A version-bump-on-every-prompt-edit policy would
work in principle but is a footgun: you forget to bump, the cache
shadows the edit, and the next person to look at the corpus sees
stale entries with no warning.

## The pattern

**Hash the prompts as input files. Store the hashes alongside the data
they produced. Treat mismatch as invalidation with no ceremony.**

The `SessionRecord` in Trawl's state file carries **three content
hashes and one version pin**:

```rust
pub struct SessionRecord {
    pub file_sha256: String,
    pub size_bytes: u64,
    pub mtime: String,
    pub extractor_prompt_sha256: String,  // <-- prompt as code
    pub tokeniser_prompt_sha256: String,  // <-- prompt as code
    pub trawl_version: String,
    ...
}
```

And the freshness check is a four-way conjunction — any mismatch and
the session re-trawls:

```rust
pub fn is_fresh(
    &self,
    session_path: &Path,
    current_file_sha: &str,
    extractor_sha: &str,
    tokeniser_sha: &str,
) -> bool {
    let Some(rec) = self.sessions.get(&session_key(session_path)) else {
        return false;
    };
    rec.file_sha256 == current_file_sha
        && rec.extractor_prompt_sha256 == extractor_sha
        && rec.tokeniser_prompt_sha256 == tokeniser_sha
        && rec.trawl_version == CRATE_VERSION
}
```

The hashes are computed at startup from the `include_str!`'d prompt
bodies. No separate "prompt manifest", no prompt registry, no version
field inside the markdown. The bytes that ship with the binary are the
bytes that get hashed.

```rust
const EXTRACTOR_PROMPT: &str = include_str!("../prompts/extractor.md");
const TOKENISER_PROMPT: &str = include_str!("../prompts/tokeniser.md");

// in run_trawl():
let extractor_sha = sha256_hex(EXTRACTOR_PROMPT.as_bytes());
let tokeniser_sha = sha256_hex(TOKENISER_PROMPT.as_bytes());
```

A single dash of Markdown in the prompt — fix a typo, tighten a rule,
rewrite the "did I miss anything?" section — changes the SHA and
invalidates every session the old SHA ever touched.

## Why it matters

Three things fall out of treating prompts as content-addressed code:

- **No forgotten corpus refresh.** The failure mode of manual version
  bumps is "I edited the prompt, shipped, and forgot to bump". Under
  hash-based invalidation that failure mode does not exist — the hash
  IS the version. Every prompt edit is self-labelling.
- **Freshness is verifiable after the fact.** If you look at any
  `content/.trawl-state.json` record and the prompt hash matches the
  current prompt file, you know that record was produced by the
  prompt you are currently reading. This makes corpus audits
  trivial: "which entries were produced by the old prompt?" is a
  one-pass scan for a known SHA.
- **Rollback and re-run are free.** `git checkout` an old prompt and
  the cache stops matching — the re-run will re-produce the old
  corpus. `git checkout` the new prompt and the same cache mismatch
  produces the new corpus. There is no migration step.

The broader principle is *content-addressed code*: the identity of a
prompt, a config file, or any other "text that governs behaviour" is
its hash, not a version field someone has to remember to bump. If the
bytes are the same, the behaviour is the same; if the bytes differ, the
cache must not match. Let the filesystem be the source of truth.

There is one place this pattern doesn't apply: behaviour that depends
on *non-local* model state (a model version you don't control). In
Trawl we pin `--model sonnet` and `--model haiku` on the CLI and
accept that an Anthropic-side model update silently invalidates the
cache without changing our hashes. The right answer there is the
`trawl_version` bump — or an explicit env-var override in the state
key. The lesson is: hash what you *own* and pin what you *don't*.

## Code pointer

- `crates/trawl/src/state.rs` — `SessionRecord` struct, `is_fresh`,
  `session_key` (canonicalization so a symlink can't defeat the cache)
- `crates/trawl/src/main.rs` — the `extractor_sha`/`tokeniser_sha`
  computation and the prune pass that drops fresh sessions before
  spawning the work-stealing pool
- `crates/trawl/prompts/extractor.md`, `crates/trawl/prompts/tokeniser.md`
  — the files whose bytes are the cache keys

## Related

- `docs/solutions/design-decisions/two-stage-zfc-pipeline-in-practice-20260408.md`
  — why prompts are code in the first place
- `docs/solutions/best-practices/streaming-sha256-zero-alloc-hot-loop-20260408.md`
  — the pure-Rust SHA-256 primitive this cache key uses
- `docs/solutions/best-practices/atomic-write-monotonic-tempfile-suffix-20260408.md`
  — the crash-safe write path the state file is persisted through
