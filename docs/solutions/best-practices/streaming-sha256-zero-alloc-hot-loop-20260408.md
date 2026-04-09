---
title: Streaming SHA-256 with Stack-Only Padding for Sliding-Window Hashing
category: technical
date: 2026-04-08
tags: [rust, sha256, hot-loop, allocation, registry]
related_commits: [f57348f]
---

# Streaming SHA-256 with Stack-Only Padding for Sliding-Window Hashing

## Problem

Trawl's PII registry validates every published draft by sliding a window
across the body and checking each window's SHA-256 against a hash set.
For a 4 KB body and a single window length, that's ~4000 hashes per
validation; multiply by every length the registry has ever seen and the
hash function is squarely on the hot path.

The textbook way to write a "minimal pure-Rust SHA-256" is to push the
input plus its padding into a `Vec<u8>` and call `compress` on each
64-byte chunk. That allocates a fresh vector on every call. In a sliding
window loop, that means one heap allocation per byte position — most of
the cost of the validation pass is malloc/free, not hashing.

## What we learned

A streaming implementation with **stack-only padding** zero-allocates
the hash function entirely:

1. Walk the source slice with `chunks_exact(64)` — full blocks are
   compressed directly from the input, no copy.
2. The remainder (0..63 bytes) is copied into a 128-byte stack buffer
   (one or two extra blocks worth, since padding can overflow when the
   tail is 56..63 bytes long).
3. Append the `0x80` terminator and the big-endian bit length, then
   compress the 1 or 2 trailing blocks from that stack buffer.

Pair this with a `hex_encode_into_buf(&[u8;32], &mut [u8;64]) -> &str`
helper that writes hex digits into a caller-owned stack buffer. The
sliding-window loop reuses one `[0u8; 64]` buffer for every window:
zero allocations on every iteration, and we only pay an owned `String`
on the rare positive hit.

The compression function itself keeps its 64×u32 message schedule on
the stack (`let mut w = [0u32; 64]`). Combined with the in-place state
array, the entire hash is stack-resident.

## How to apply

When you're hashing a fixed-shape input in a hot loop:

1. **Write the streaming form, not the buffered form.** `chunks_exact`
   plus a small stack tail buffer is shorter than the Vec version and
   strictly faster.
2. **Separate "give me the digest" from "give me a hex string."** The
   raw `[u8; 32]` is what your hash set wants; only allocate hex when
   you're about to hand it back to a caller.
3. **Hand out a `&str` view into a caller-provided stack buffer** for
   membership checks. The borrow checker keeps the lifetimes honest and
   you skip every per-iteration `String` allocation.
4. **Don't pull in `sha2`** for cache-key uses. ~150 lines of pure Rust
   compiles instantly, lets you keep the dependency closure tight, and
   makes the hot-path allocation behaviour obvious by inspection.

## Code pointer

- `crates/trawl/src/state.rs:184-225` — `sha256_bytes`, `sha256_hex`,
  `hex_encode`, `hex_encode_into_buf`
- `crates/trawl/src/state.rs:246-360` — pure-Rust `Sha256::digest` /
  `compress` with stack-only padding
- `crates/trawl/src/registry.rs:140-188` — sliding-window consumer that
  reuses one `hex_buf` across the entire scan
