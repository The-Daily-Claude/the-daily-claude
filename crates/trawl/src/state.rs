//! Incremental state file (`content/.trawl-state.json`).
//!
//! Records what was already trawled so unchanged sessions are skipped on
//! rerun. The decision to skip is the conjunction of the file hash, both
//! prompt hashes, both backend signatures, and a version gate:
//!
//! - `file_sha256`: did the session jsonl change?
//! - `extractor_prompt_sha256`: did the extractor prompt change?
//! - `tokeniser_prompt_sha256`: did the tokeniser prompt change?
//! - `extractor_backend_signature`: did the extractor provider/model/effort change?
//! - `tokeniser_backend_signature`: did the tokeniser provider/model/effort change?
//! - `trawl_version`: did the binary itself bump in a way that
//!   invalidates prior runs?
//!
//! Any mismatch → re-trawl from scratch. No append-mode in v1; we trust
//! the selected model backend to be deterministic enough for full rerun
//! to be cheap.

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Process-wide counter for uniqueifying tempfile names across
/// threads. Each call to `atomic_write` pulls a fresh value so two
/// concurrent writes in the same process — even of the same target
/// path — never collide on their sidecar tempfile.
static TMPFILE_COUNTER: AtomicU64 = AtomicU64::new(0);

/// RAII guard that best-effort removes a sidecar tempfile when dropped.
///
/// Used by both `atomic_write` and `atomic_write_exclusive` so the
/// sidecar is cleaned up on *every* exit path — including the one
/// where `fs::write` itself fails after creating a zero- or
/// partially-written tempfile, or a panic unwinds through the write.
/// This is what makes the "no litter on failure" guarantee actually
/// hold.
///
/// On the success paths the sidecar is already gone (`rename` moved
/// it away; `hard_link` + post-success drop on the exclusive path)
/// so the guard's `remove_file` in `Drop` is a harmless no-op.
struct TmpFileGuard<'a>(&'a Path);

impl Drop for TmpFileGuard<'_> {
    fn drop(&mut self) {
        let _ = fs::remove_file(self.0);
    }
}

/// Maximum byte length of the `file_name` component embedded in a
/// sidecar tempfile name. POSIX `NAME_MAX` is typically 255 bytes;
/// the sidecar adds a leading `.`, a `.tmp.` separator, the pid
/// (worst case ~10 digits on a 32-bit pid_t), a `.`, and a `u64`
/// nonce (worst case 20 digits) ≈ 37 bytes of overhead. Capping the
/// name component at 200 bytes leaves generous headroom for every
/// realistic pid + nonce combination while still producing a
/// debugger-friendly sidecar name for normal-length files.
const MAX_SIDECAR_NAME_BYTES: usize = 200;

/// Shared sidecar-path builder for `atomic_write` / `atomic_write_exclusive`.
///
/// Extracted so the two atomic-write helpers can't drift in subtle
/// ways (nonce format, directory derivation, long-name handling).
///
/// Responsibilities:
/// - Derive the parent directory from `final_path` and ensure it exists.
/// - Pull a fresh monotonic nonce from `TMPFILE_COUNTER` for
///   cross-thread uniqueness.
/// - Truncate the original file name to `MAX_SIDECAR_NAME_BYTES` on a
///   UTF-8 char boundary so the final tempfile name can't exceed
///   POSIX `NAME_MAX` even for pathologically long source names.
fn sidecar_tmp_path(final_path: &Path) -> Result<PathBuf> {
    let parent = final_path
        .parent()
        .ok_or_else(|| anyhow!("path has no parent: {}", final_path.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("mkdir {}", parent.display()))?;

    let file_name = final_path
        .file_name()
        .ok_or_else(|| anyhow!("path has no file name: {}", final_path.display()))?;

    // `to_string_lossy` is fine here: if the source name contains
    // non-UTF-8 bytes the sidecar uses U+FFFD replacements, which is
    // allowed because the replaced bytes only need to disambiguate
    // sidecars in the same directory — they are never round-tripped
    // back onto `final_path`.
    let file_name_str = file_name.to_string_lossy();
    let truncated: &str = if file_name_str.len() > MAX_SIDECAR_NAME_BYTES {
        let mut end = MAX_SIDECAR_NAME_BYTES;
        while end > 0 && !file_name_str.is_char_boundary(end) {
            end -= 1;
        }
        &file_name_str[..end]
    } else {
        &file_name_str
    };

    let nonce = TMPFILE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let tmp_name = format!(".{}.tmp.{}.{nonce}", truncated, std::process::id());
    Ok(parent.join(tmp_name))
}

/// Write `contents` to `final_path` atomically via a same-directory
/// tempfile + rename.
///
/// The sidecar name is produced by `sidecar_tmp_path`: a dotted
/// prefix, the (possibly truncated) source basename, `.tmp.`, the pid,
/// and a process-wide monotonic `AtomicU64` nonce. That combination
/// makes the helper safe to call concurrently from multiple threads —
/// even against the same final path — without two writers colliding on
/// their sidecar.
///
/// If the process is killed mid-write, a reader sees either the
/// previous version of the file (no partial state observable) or an
/// unrelated sidecar — never a half-written target. The tempfile
/// lives on the same filesystem as the final path so `rename(2)` is a
/// single atomic syscall on POSIX. The `TmpFileGuard` cleans up the
/// sidecar on every failure path (write, rename, panic) so the
/// directory stays tidy.
///
/// Shared by `State::save`, `Registry::save`, and the main binary's
/// entry writer so all three pipeline outputs have the same crash-
/// safety guarantee.
pub fn atomic_write(final_path: &Path, contents: &[u8]) -> Result<()> {
    let tmp_path = sidecar_tmp_path(final_path)?;

    // Guard installed BEFORE the write so any exit path — write
    // failure, rename failure, panic — unlinks the sidecar. On the
    // success path `rename` moves the inode away so the guard's
    // `remove_file` no-ops.
    let _guard = TmpFileGuard(&tmp_path);

    fs::write(&tmp_path, contents)
        .with_context(|| format!("write tempfile {}", tmp_path.display()))?;

    fs::rename(&tmp_path, final_path)
        .with_context(|| format!("rename {} -> {}", tmp_path.display(), final_path.display()))?;
    Ok(())
}

/// Atomically publish `contents` at `final_path` only if the target
/// does not already exist. Returns `Ok(true)` on publish, `Ok(false)`
/// if the target was already present (so the caller can renumber and
/// retry), and `Err` on any other I/O failure.
///
/// Implementation: share sidecar naming with `atomic_write` via
/// `sidecar_tmp_path`, then `link(2)` the tempfile to `final_path`.
/// POSIX `link` fails with `EEXIST` if the target exists — that is
/// our race-safe "create new" signal. The `TmpFileGuard` removes the
/// sidecar after the link attempt regardless of outcome.
///
/// This closes the last correctness gap for concurrent trawl runs:
/// two processes that both computed the same `next_number` will race
/// on the link, exactly one of them wins, the loser bumps and
/// retries.
pub fn atomic_write_exclusive(final_path: &Path, contents: &[u8]) -> Result<bool> {
    let tmp_path = sidecar_tmp_path(final_path)?;

    // Guard installed BEFORE the write so any exit path — write
    // failure, link failure, EEXIST-and-return, panic — unlinks the
    // sidecar. On the success/EEXIST paths the guard also removes
    // the sidecar after `hard_link` returns; no separate explicit
    // cleanup needed.
    let _guard = TmpFileGuard(&tmp_path);

    fs::write(&tmp_path, contents)
        .with_context(|| format!("write tempfile {}", tmp_path.display()))?;

    match fs::hard_link(&tmp_path, final_path) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e)
            .with_context(|| format!("link {} -> {}", tmp_path.display(), final_path.display())),
    }
}

pub const CRATE_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Per-session state record.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SessionRecord {
    pub file_sha256: String,
    pub size_bytes: u64,
    pub mtime: String,
    pub extractor_prompt_sha256: String,
    pub tokeniser_prompt_sha256: String,
    #[serde(default)]
    pub extractor_backend_signature: String,
    #[serde(default)]
    pub tokeniser_backend_signature: String,
    pub trawl_version: String,
    /// Entry filenames (e.g. `312-per-your-own-rule.md`) produced by the
    /// last successful run on this session — useful for cleanup if the
    /// session is later re-trawled.
    #[serde(default)]
    pub extracted_entry_files: Vec<String>,
}

/// On-disk state file: keyed by absolute session path.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct State {
    #[serde(default)]
    pub sessions: BTreeMap<String, SessionRecord>,
}

impl State {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(State::default());
        }
        let raw = fs::read_to_string(path)
            .with_context(|| format!("read state file {}", path.display()))?;
        if raw.trim().is_empty() {
            return Ok(State::default());
        }
        serde_json::from_str(&raw).with_context(|| format!("parse state {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self).context("serialise state")?;
        atomic_write(path, json.as_bytes())
            .with_context(|| format!("write state {}", path.display()))
    }

    /// Decide whether `session_path` needs trawling given the current
    /// prompt and version hashes. Returns `true` if the session is up to
    /// date and should be skipped.
    pub fn is_fresh(
        &self,
        session_path: &Path,
        current_file_sha: &str,
        extractor_sha: &str,
        tokeniser_sha: &str,
        extractor_backend_signature: &str,
        tokeniser_backend_signature: &str,
    ) -> bool {
        let Some(rec) = self.sessions.get(&session_key(session_path)) else {
            return false;
        };
        rec.file_sha256 == current_file_sha
            && rec.extractor_prompt_sha256 == extractor_sha
            && rec.tokeniser_prompt_sha256 == tokeniser_sha
            && rec.extractor_backend_signature == extractor_backend_signature
            && rec.tokeniser_backend_signature == tokeniser_backend_signature
            && rec.trawl_version == CRATE_VERSION
    }

    /// Record a successful run for a session.
    pub fn record(&mut self, session_path: &Path, rec: SessionRecord) {
        self.sessions.insert(session_key(session_path), rec);
    }
}

/// Canonicalize a session path into a stable state-file key.
///
/// `session_path.to_string_lossy()` alone is not enough: the same
/// session can be reached via an absolute path, a relative path, or
/// through a symlink, and each one would produce a different key
/// and defeat the freshness cache. We canonicalize when possible and
/// fall back to the raw lossy form only when canonicalize fails (the
/// file no longer exists, permissions, etc).
fn session_key(session_path: &Path) -> String {
    match fs::canonicalize(session_path) {
        Ok(canon) => canon.to_string_lossy().to_string(),
        Err(_) => session_path.to_string_lossy().to_string(),
    }
}

/// Standard location for the state file: `content/.trawl-state.json`.
pub fn default_state_path(content_root: &Path) -> PathBuf {
    content_root.join(".trawl-state.json")
}

/// Stable, dependency-free SHA-256 of arbitrary bytes. Returns the
/// 32-byte digest directly — no allocation.
///
/// We avoid pulling in the `sha2` crate so the build stays minimal.
/// This is a straightforward FIPS-180-4 implementation — never used
/// for anything cryptographically load-bearing, only as a content-
/// addressed cache key for the state file and as the leak-detection
/// lookup key for the PII registry.
///
/// The implementation is **streaming**: the input is processed in
/// 64-byte blocks directly from the source slice, with padding done on
/// a stack-allocated tail buffer. No `Vec`, no heap allocation. This
/// matters because `Registry::find_leaks` calls this helper in a tight
/// sliding-window loop.
pub fn sha256_bytes(input: &[u8]) -> [u8; 32] {
    Sha256::digest(input)
}

/// Hex-encoded SHA-256 digest — convenience wrapper. Allocates a
/// 64-byte `String`, so use `sha256_bytes` in hot paths that only
/// need the raw digest (e.g., hash-set membership checks).
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex_encode(&sha256_bytes(bytes))
}

/// Hex-encode a 32-byte digest into a 64-char `String`. Allocates
/// once. For hot-path lookups prefer `hex_encode_into_buf` which
/// writes into a caller-provided stack buffer and returns a `&str`.
pub fn hex_encode(digest: &[u8; 32]) -> String {
    let mut buf = [0u8; 64];
    hex_encode_into_buf(digest, &mut buf);
    // Safe: `buf` contains only ASCII hex digits.
    String::from_utf8(buf.to_vec()).expect("hex output is ASCII")
}

/// Hex-encode a 32-byte digest into a caller-provided 64-byte stack
/// buffer and return a `&str` view. Zero allocations.
///
/// The returned `&str` borrows from `buf`, so `buf` must outlive the
/// caller's use of the slice. Intended for tight loops where the hex
/// string is only needed for a membership check and never owned.
pub fn hex_encode_into_buf<'a>(digest: &[u8; 32], buf: &'a mut [u8; 64]) -> &'a str {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for (i, &b) in digest.iter().enumerate() {
        buf[i * 2] = HEX[(b >> 4) as usize];
        buf[i * 2 + 1] = HEX[(b & 0x0f) as usize];
    }
    // Safe: every byte written is from HEX, which is ASCII.
    std::str::from_utf8(buf).expect("hex output is ASCII")
}

/// Convenience: hash the contents of a file.
pub fn sha256_file(path: &Path) -> Result<String> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    Ok(sha256_hex(&bytes))
}

// ─── Minimal pure-Rust SHA-256 ────────────────────────────────────────
// Adapted from FIPS 180-4. Not constant-time. Not for crypto. Used only
// as a content-addressed cache key and as the PII registry lookup key.

struct Sha256;

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

impl Sha256 {
    /// Streaming SHA-256 — process full 64-byte blocks directly from
    /// the input slice, then pad the trailing bytes on a stack buffer.
    /// Zero heap allocations.
    fn digest(input: &[u8]) -> [u8; 32] {
        let mut h: [u32; 8] = [
            0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
            0x5be0cd19,
        ];

        // Process every full 64-byte block directly from the source.
        let mut chunks = input.chunks_exact(64);
        for block in &mut chunks {
            Self::compress(&mut h, block);
        }

        // The remainder is 0..63 bytes that didn't fit into a full
        // block. Pad on a stack-allocated 128-byte buffer (at most two
        // blocks, since padding can force an overflow when the tail is
        // 56..63 bytes long).
        let tail = chunks.remainder();
        let mut buf = [0u8; 128];
        let tail_len = tail.len();
        buf[..tail_len].copy_from_slice(tail);
        buf[tail_len] = 0x80;

        // Length goes in the last 8 bytes of whichever block carries
        // the end of the padding.
        let bit_len = (input.len() as u64).wrapping_mul(8);
        let padded_len = if tail_len + 1 + 8 <= 64 { 64 } else { 128 };
        buf[padded_len - 8..padded_len].copy_from_slice(&bit_len.to_be_bytes());

        Self::compress(&mut h, &buf[..64]);
        if padded_len == 128 {
            Self::compress(&mut h, &buf[64..128]);
        }

        let mut out = [0u8; 32];
        for (i, word) in h.iter().enumerate() {
            out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
        }
        out
    }

    /// One SHA-256 compression round on a 64-byte block. The message
    /// schedule lives on the stack (64×u32 = 256 bytes).
    fn compress(h: &mut [u32; 8], block: &[u8]) {
        debug_assert_eq!(block.len(), 64);

        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                block[i * 4],
                block[i * 4 + 1],
                block[i * 4 + 2],
                block[i * 4 + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let mut a = h[0];
        let mut b = h[1];
        let mut c = h[2];
        let mut d = h[3];
        let mut e = h[4];
        let mut f = h[5];
        let mut g = h[6];
        let mut hh = h[7];

        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(SHA256_K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);

            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("trawl-state-test-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn sha256_known_vectors() {
        // Empty input
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        // "abc"
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sha256_long_input_crosses_block_boundary() {
        // 56 bytes is the boundary case where padding crosses into a new block
        let input = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(
            sha256_hex(input),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn empty_state_loads_as_default() {
        let dir = tmp_dir("empty");
        let path = dir.join("missing.json");
        let s = State::load(&path).unwrap();
        assert!(s.sessions.is_empty());
    }

    #[test]
    fn state_round_trips_through_disk() {
        let dir = tmp_dir("round-trip");
        let path = dir.join("state.json");

        let mut s = State::default();
        let session_path = PathBuf::from("/abs/path/abc.jsonl");
        s.record(
            &session_path,
            SessionRecord {
                file_sha256: "deadbeef".to_string(),
                size_bytes: 1024,
                mtime: "2026-04-07T22:00:00Z".to_string(),
                extractor_prompt_sha256: "ext".to_string(),
                tokeniser_prompt_sha256: "tok".to_string(),
                extractor_backend_signature: "claude-code/sonnet".to_string(),
                tokeniser_backend_signature: "claude-code/haiku".to_string(),
                trawl_version: CRATE_VERSION.to_string(),
                extracted_entry_files: vec!["312-per-your-own-rule.md".to_string()],
            },
        );

        s.save(&path).unwrap();
        let loaded = State::load(&path).unwrap();
        assert_eq!(loaded.sessions.len(), 1);
    }

    #[test]
    fn fresh_skips_when_all_hashes_match() {
        let mut s = State::default();
        let p = PathBuf::from("/x/a.jsonl");
        s.record(
            &p,
            SessionRecord {
                file_sha256: "f".to_string(),
                size_bytes: 0,
                mtime: "now".to_string(),
                extractor_prompt_sha256: "e".to_string(),
                tokeniser_prompt_sha256: "t".to_string(),
                extractor_backend_signature: "claude-code/sonnet".to_string(),
                tokeniser_backend_signature: "claude-code/haiku".to_string(),
                trawl_version: CRATE_VERSION.to_string(),
                extracted_entry_files: vec![],
            },
        );

        assert!(s.is_fresh(&p, "f", "e", "t", "claude-code/sonnet", "claude-code/haiku"));
        assert!(!s.is_fresh(
            &p,
            "different",
            "e",
            "t",
            "claude-code/sonnet",
            "claude-code/haiku"
        ));
        assert!(!s.is_fresh(
            &p,
            "f",
            "different",
            "t",
            "claude-code/sonnet",
            "claude-code/haiku"
        ));
        assert!(!s.is_fresh(
            &p,
            "f",
            "e",
            "different",
            "claude-code/sonnet",
            "claude-code/haiku"
        ));
        assert!(!s.is_fresh(
            &p,
            "f",
            "e",
            "t",
            "codex/gpt-5.4-codex",
            "claude-code/haiku"
        ));
    }

    #[test]
    fn fresh_returns_false_for_unknown_session() {
        let s = State::default();
        let p = PathBuf::from("/never/seen.jsonl");
        assert!(!s.is_fresh(
            &p,
            "any",
            "any",
            "any",
            "claude-code/sonnet",
            "claude-code/haiku"
        ));
    }

    #[test]
    fn legacy_state_without_backend_signatures_still_loads() {
        let dir = tmp_dir("legacy-state");
        let path = dir.join("state.json");
        std::fs::write(
            &path,
            r#"{
  "sessions": {
    "/tmp/demo.jsonl": {
      "file_sha256": "f",
      "size_bytes": 1,
      "mtime": "now",
      "extractor_prompt_sha256": "e",
      "tokeniser_prompt_sha256": "t",
      "trawl_version": "0.1.0",
      "extracted_entry_files": []
    }
  }
}"#,
        )
        .unwrap();

        let loaded = State::load(&path).unwrap();
        let rec = loaded.sessions.get("/tmp/demo.jsonl").unwrap();
        assert_eq!(rec.extractor_backend_signature, "");
        assert_eq!(rec.tokeniser_backend_signature, "");
    }

    #[test]
    fn session_key_canonicalizes_real_paths() {
        // Create two different references to the same real file and
        // verify both resolve to the same state-file key, regardless of
        // whether one goes through a relative path or an absolute one.
        let dir = tmp_dir("canon");
        let real = dir.join("session.jsonl");
        std::fs::write(&real, b"{}").unwrap();

        // Absolute path
        let key_abs = session_key(&real);

        // Relative path — jump to the tmp dir and back so the relative
        // resolution exercises the canonicalize logic.
        let cwd = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        let key_rel = session_key(Path::new("session.jsonl"));
        std::env::set_current_dir(&cwd).unwrap();

        assert_eq!(
            key_abs, key_rel,
            "canonicalized keys should match regardless of access path"
        );
    }

    #[test]
    fn session_key_falls_back_for_missing_files() {
        // Canonicalize fails for non-existent paths; we must still
        // produce a stable key so the fallback path doesn't panic.
        let p = PathBuf::from("/absolutely/does/not/exist.jsonl");
        let key1 = session_key(&p);
        let key2 = session_key(&p);
        assert_eq!(key1, key2);
        assert!(key1.contains("does/not/exist"));
    }

    #[test]
    fn atomic_write_leaves_target_file_and_no_sidecar() {
        let dir = tmp_dir("aw-target");
        let target = dir.join("entry.md");
        atomic_write(&target, b"hello world").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "hello world");

        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().into_string().unwrap())
            .filter(|n| n.contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "leftover sidecars: {leftovers:?}");
    }

    #[test]
    fn atomic_write_creates_parent_directories() {
        let dir = tmp_dir("aw-nested");
        let nested = dir.join("content/entries");
        let target = nested.join("312-file.md");
        atomic_write(&target, b"x").unwrap();
        assert!(target.exists());
    }

    #[test]
    fn atomic_write_overwrites_existing_file() {
        let dir = tmp_dir("aw-overwrite");
        let target = dir.join("001-same.md");
        atomic_write(&target, b"first").unwrap();
        atomic_write(&target, b"second").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "second");
    }

    #[test]
    fn atomic_write_parallel_writes_to_different_targets_do_not_collide() {
        // Many threads each writing their own target file in the same
        // directory must not trip over each other's tempfiles.
        let dir = tmp_dir("aw-parallel-distinct");
        let dir_ref = dir.clone();

        std::thread::scope(|s| {
            for i in 0..16 {
                let dir = dir_ref.clone();
                s.spawn(move || {
                    let target = dir.join(format!("{i:03}-entry.md"));
                    let contents = format!("entry number {i}");
                    atomic_write(&target, contents.as_bytes()).unwrap();
                    assert_eq!(std::fs::read_to_string(&target).unwrap(), contents);
                });
            }
        });

        // All 16 targets landed
        let count = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| {
                e.file_name()
                    .to_str()
                    .map(|n| n.ends_with("-entry.md"))
                    .unwrap_or(false)
            })
            .count();
        assert_eq!(count, 16);

        // No leftover .tmp. sidecars
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().into_string().unwrap())
            .filter(|n| n.contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "tempfile leak: {leftovers:?}");
    }

    #[test]
    fn atomic_write_parallel_writes_to_same_target_do_not_collide() {
        // The pathological case Gemini flagged: many threads writing
        // to the *same* final path. Exactly one of them wins the race
        // (whichever renames last) but no thread observes a tempfile
        // collision or a partial write — the target always contains
        // one of the valid payloads and no sidecar is left behind.
        let dir = tmp_dir("aw-parallel-same");
        let target = dir.join("contended.md");
        let target_ref = target.clone();

        std::thread::scope(|s| {
            for i in 0..16 {
                let target = target_ref.clone();
                s.spawn(move || {
                    let payload = format!("writer-{i}");
                    atomic_write(&target, payload.as_bytes()).unwrap();
                });
            }
        });

        // Exactly one final file exists and its body is one of the
        // valid payloads.
        let got = std::fs::read_to_string(&target).unwrap();
        assert!(
            got.starts_with("writer-"),
            "unexpected contents after contention: {got:?}"
        );

        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().into_string().unwrap())
            .filter(|n| n.contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "tempfile leak: {leftovers:?}");
    }

    #[test]
    fn atomic_write_exclusive_publishes_when_target_missing() {
        let dir = tmp_dir("awx-fresh");
        let target = dir.join("312-entry.md");
        let published = atomic_write_exclusive(&target, b"fresh").unwrap();
        assert!(published, "should publish when target does not exist");
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "fresh");

        // No leftover sidecars.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().into_string().unwrap())
            .filter(|n| n.contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "tempfile leak: {leftovers:?}");
    }

    #[test]
    fn atomic_write_exclusive_refuses_to_overwrite() {
        let dir = tmp_dir("awx-refuse");
        let target = dir.join("001-original.md");
        std::fs::write(&target, b"original contents").unwrap();

        let published = atomic_write_exclusive(&target, b"new contents").unwrap();
        assert!(!published, "should refuse to overwrite existing target");
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "original contents",
            "existing file must be untouched"
        );

        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().into_string().unwrap())
            .filter(|n| n.contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "tempfile leak: {leftovers:?}");
    }

    #[test]
    fn sidecar_tmp_path_truncates_long_source_names() {
        // POSIX NAME_MAX is 255 bytes. A pathological 300-byte source
        // filename would, without truncation, produce a sidecar of
        // ~337 bytes and fail `fs::write` with ENAMETOOLONG. The
        // builder must cap the name component at MAX_SIDECAR_NAME_BYTES
        // and stay within the limit no matter what.
        let dir = tmp_dir("sidecar-long");
        let long_stem = "a".repeat(300);
        let target = dir.join(format!("{long_stem}.md"));
        let tmp = sidecar_tmp_path(&target).unwrap();

        let tmp_basename = tmp.file_name().unwrap().to_string_lossy();
        assert!(
            tmp_basename.len() < 255,
            "sidecar basename must fit in NAME_MAX; got {} bytes: {}",
            tmp_basename.len(),
            tmp_basename
        );
        assert!(
            tmp_basename.starts_with(".aa"),
            "sidecar should still begin with the source name prefix: {tmp_basename}"
        );
        assert!(
            tmp_basename.contains(".tmp."),
            "sidecar should still carry the .tmp. marker: {tmp_basename}"
        );
    }

    #[test]
    fn atomic_write_handles_near_namemax_filenames() {
        // The target filename must itself be legal (<= NAME_MAX on
        // the host filesystem). The helper's job is to ensure the
        // *sidecar* it builds for that target is also legal, even
        // when the source name is large enough that the naive
        // `.<name>.tmp.<pid>.<nonce>` construction would overflow.
        //
        // Pick a target size well under NAME_MAX (so the target
        // itself can be created) but large enough that the sidecar
        // without truncation would blow through NAME_MAX. 240 bytes
        // meets both constraints: legal on macOS APFS (NAME_MAX=255),
        // and the naive sidecar would be ~272 bytes.
        let dir = tmp_dir("aw-near-namemax");
        let long_name = format!("{}.md", "z".repeat(237));
        assert!(
            long_name.len() < 255,
            "test fixture must be a legal filename"
        );
        let target = dir.join(&long_name);
        atomic_write(&target, b"hello").unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "hello");

        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().into_string().unwrap())
            .filter(|n| n.contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "tempfile leak: {leftovers:?}");
    }

    #[test]
    fn sidecar_tmp_path_truncates_on_utf8_char_boundary() {
        // 100 copies of the 4-byte char é (U+00E9 in 2-byte UTF-8
        // would be 200 bytes; we use a 4-byte char "𐐷" instead to
        // force the truncation to land on a multi-byte boundary).
        let dir = tmp_dir("sidecar-utf8");
        let heavy: String = "𐐷".repeat(100); // 400 bytes of valid UTF-8
        let target = dir.join(format!("{heavy}.md"));
        let tmp = sidecar_tmp_path(&target).unwrap();

        let tmp_basename = tmp.file_name().unwrap().to_string_lossy();
        assert!(
            tmp_basename.len() < 255,
            "utf-8 sidecar basename must fit in NAME_MAX: {} bytes",
            tmp_basename.len()
        );
        // The body of the sidecar must still be valid UTF-8 — not
        // sliced through a multi-byte codepoint — which `to_string_lossy`
        // would otherwise corrupt via U+FFFD.
        // `format!` with `{tmp_basename}` would fail to render if we'd
        // sliced mid-codepoint, so the round-trip itself is the test.
        assert!(tmp_basename.starts_with(".𐐷"));
    }

    #[test]
    fn atomic_write_exclusive_parallel_writers_elect_single_winner() {
        // The #027 race: 32 threads all targeting the same entry
        // filename. Exactly one of them should publish (`Ok(true)`);
        // every other thread should cleanly observe `Ok(false)` so the
        // caller can renumber and retry. No tempfile litter, no panics.
        let dir = tmp_dir("awx-race");
        let target = dir.join("042-contended.md");
        let target_ref = target.clone();

        let winners: std::sync::Mutex<u32> = std::sync::Mutex::new(0);
        let losers: std::sync::Mutex<u32> = std::sync::Mutex::new(0);

        std::thread::scope(|s| {
            for i in 0..32 {
                let target = target_ref.clone();
                let winners = &winners;
                let losers = &losers;
                s.spawn(move || {
                    let payload = format!("writer-{i}");
                    match atomic_write_exclusive(&target, payload.as_bytes()).unwrap() {
                        true => *winners.lock().unwrap() += 1,
                        false => *losers.lock().unwrap() += 1,
                    }
                });
            }
        });

        assert_eq!(
            *winners.lock().unwrap(),
            1,
            "exactly one writer must win the race"
        );
        assert_eq!(
            *losers.lock().unwrap(),
            31,
            "every other writer must cleanly observe Ok(false)"
        );

        // The published body is one of the valid payloads.
        let got = std::fs::read_to_string(&target).unwrap();
        assert!(
            got.starts_with("writer-"),
            "unexpected contents after race: {got:?}"
        );

        // No leftover sidecars.
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().into_string().unwrap())
            .filter(|n| n.contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "tempfile leak: {leftovers:?}");
    }

    #[test]
    fn save_is_atomic_via_tempfile() {
        let dir = tmp_dir("atomic-save");
        let path = dir.join("state.json");

        let mut s = State::default();
        s.record(
            &PathBuf::from("/x/a.jsonl"),
            SessionRecord {
                file_sha256: "f".into(),
                size_bytes: 0,
                mtime: "now".into(),
                extractor_prompt_sha256: "e".into(),
                tokeniser_prompt_sha256: "t".into(),
                extractor_backend_signature: "claude-code/sonnet".into(),
                tokeniser_backend_signature: "claude-code/haiku".into(),
                trawl_version: CRATE_VERSION.into(),
                extracted_entry_files: vec![],
            },
        );
        s.save(&path).unwrap();

        // No leftover .tmp. sidecars in the directory
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().into_string().unwrap())
            .filter(|n| n.contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty(), "tempfile leak: {leftovers:?}");

        // And the final file is readable
        let loaded = State::load(&path).unwrap();
        assert_eq!(loaded.sessions.len(), 1);
    }
}
