//! PII registry — the deterministic safety net.
//!
//! After Haiku tokenises a draft, the transient `placeholder → real_value`
//! map exists only in memory and evaporates. We never write that map to
//! disk. But we DO record the set of literal strings that the model has
//! flagged as PII at any point in any run — as SHA-256 digests and byte
//! lengths, never as plaintext. Every new draft body is validated
//! against the union of every literal the registry has ever learned: if
//! any of them appears verbatim in the body, the entry is marked
//! `needs_manual_review` so a human catches the leak before publish.
//!
//! This is the deterministic backstop the regex anonymizer used to be —
//! but its contents are *grown by the model*, not hand-curated.
//!
//! Storage policy: literals are SHA-256 hashed before being written to
//! disk so the registry file itself never carries plaintext PII. The
//! registry also records the *byte length* of each literal it has ever
//! seen, so `find_leaks` can scan only those exact window sizes — both
//! for correctness (so a 120-byte literal can't hide from a 96-byte
//! max) and for performance (so we don't hash every possible window
//! size between min and max).
//!
//! # Storage location
//!
//! **v1 (today):** `content/.pii-registry.json`, inside the caller's
//! content repo, gitignored.
//!
//! **Target (tracked in `todos/023-trawl-state-out-of-repo.md`):**
//! `~/.local/share/com.the-daily-claude.trawl/pii-registry.json` on both
//! macOS and Linux — `$XDG_DATA_HOME` on Linux, the same path literal on
//! macOS for cross-platform parity (deliberately NOT
//! `~/Library/Application Support/`). Outside any repo, per-machine,
//! `0700` perms. Cloning the repo on a fresh machine inherits the
//! user's accumulated registry from their home dir, and wiping
//! `content/` no longer destroys runtime state.

use crate::state::{atomic_write, hex_encode_into_buf, sha256_bytes, sha256_hex};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Shortest literal length the registry stores. Literals shorter than
/// this are too short to be a reliable leak signal and would bloat the
/// on-disk set without ever being matchable.
pub const MIN_LITERAL_LEN: usize = 8;

/// Fallback length range used when validating against a legacy registry
/// that predates length tracking. New registries populate the `lengths`
/// set as they grow and never hit this fallback; we keep it only so an
/// old `.pii-registry.json` loaded from disk still produces meaningful
/// leak detection without a hard migration.
const LEGACY_MIN_LEN: usize = MIN_LITERAL_LEN;
const LEGACY_MAX_LEN: usize = 256;

/// On-disk PII registry. The plaintext literals never touch disk —
/// only their SHA-256 hex digests and their byte lengths do.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Registry {
    /// SHA-256 hex digests of literal PII strings the model has flagged.
    #[serde(default)]
    hashes: BTreeSet<String>,
    /// Byte lengths of every literal that has ever been added. Used by
    /// `find_leaks` to probe only the window sizes that can actually
    /// match. If this set is empty but `hashes` is not, the registry
    /// was loaded from a legacy file that predates length tracking and
    /// `find_leaks` falls back to a wider scan range.
    #[serde(default)]
    lengths: BTreeSet<usize>,
    /// Global set of human-readable category tags observed anywhere in
    /// the registry — useful for debugging which classes of PII have
    /// been recorded (e.g. `"CRED"`, `"USER"`). This is a flat set, not
    /// a per-hash map: the whole point of hashing literals on disk is
    /// that the plaintext-to-category mapping evaporates with the run.
    #[serde(default)]
    categories: BTreeSet<String>,
}

impl Registry {
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Registry::default());
        }
        let raw = fs::read_to_string(path)
            .with_context(|| format!("read registry {}", path.display()))?;
        if raw.trim().is_empty() {
            return Ok(Registry::default());
        }
        serde_json::from_str(&raw)
            .with_context(|| format!("parse registry {}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self).context("serialise registry")?;
        atomic_write(path, json.as_bytes())
            .with_context(|| format!("write registry {}", path.display()))
    }

    pub fn len(&self) -> usize {
        self.hashes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty()
    }

    /// Add every transient PII literal the tokeniser learned about for
    /// this draft. Each literal is hashed before being recorded and its
    /// byte length is recorded alongside so `find_leaks` can probe only
    /// the exact window sizes present in the registry.
    ///
    /// Literals shorter than `MIN_LITERAL_LEN` are dropped because they
    /// would bloat the on-disk set without ever being a reliable leak
    /// signal.
    pub fn grow<I, S>(&mut self, literals: I)
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        for literal in literals {
            let s = literal.as_ref().trim();
            let len = s.len();
            if len < MIN_LITERAL_LEN {
                continue;
            }
            self.hashes.insert(sha256_hex(s.as_bytes()));
            self.lengths.insert(len);
        }
    }

    /// Record a category label (e.g., "CRED", "USER") so debugging the
    /// registry doesn't require remembering what each hash meant.
    pub fn note_category(&mut self, category: impl Into<String>) {
        self.categories.insert(category.into());
    }

    /// Validate a tokenised body against the registry. Returns the list
    /// of registry hashes that matched a substring of `body`. A non-empty
    /// list means the body contains literal PII the registry has learned
    /// about and the entry must be marked `needs_manual_review`.
    ///
    /// The scan iterates only the window sizes the registry has actually
    /// seen — so a literal of any length, including lengths outside the
    /// old hard-coded 8..=96 band, is matchable. This closes the hole
    /// where a 120-byte credential in the hash set could hide from a
    /// bounded scan.
    ///
    /// For legacy registries that predate length tracking (loaded from a
    /// file where the `lengths` field was empty), we fall back to a
    /// wider static range so old data still catches leaks.
    pub fn find_leaks(&self, body: &str) -> Vec<String> {
        if self.hashes.is_empty() {
            return Vec::new();
        }

        let bytes = body.as_bytes();
        let n = bytes.len();
        let mut hits = Vec::new();

        // Pick the length set to iterate. New registries use the tracked
        // set; legacy registries fall back to a wide static range.
        let legacy_range: BTreeSet<usize> = if self.lengths.is_empty() {
            (LEGACY_MIN_LEN..=LEGACY_MAX_LEN).collect()
        } else {
            BTreeSet::new()
        };
        let lengths: &BTreeSet<usize> = if self.lengths.is_empty() {
            &legacy_range
        } else {
            &self.lengths
        };

        // Stack buffer for hex-encoding each digest. Reused across every
        // iteration of the hot loop so we pay zero heap allocations per
        // byte position; only the rare positive match allocates an
        // owned `String` to push onto `hits`.
        let mut hex_buf = [0u8; 64];

        for &window in lengths {
            if window == 0 || window > n {
                continue;
            }
            let mut i = 0;
            while i + window <= n {
                let slice = &bytes[i..i + window];
                let digest = sha256_bytes(slice);
                let hex = hex_encode_into_buf(&digest, &mut hex_buf);
                if self.hashes.contains(hex) {
                    hits.push(hex.to_owned());
                }
                i += 1;
            }
        }

        hits.sort();
        hits.dedup();
        hits
    }
}

/// Standard registry path: `<content_root>/.pii-registry.json`.
pub fn default_registry_path(content_root: &Path) -> PathBuf {
    content_root.join(".pii-registry.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "trawl-test-{}-{}",
            name,
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join("registry.json")
    }

    #[test]
    fn empty_registry_finds_no_leaks() {
        let r = Registry::default();
        assert!(r.find_leaks("anything goes").is_empty());
    }

    #[test]
    fn grow_then_validate_catches_known_literal() {
        let mut r = Registry::default();
        r.grow(["dp.sa.SECRETTOKENVALUE12345"]);

        let body = "and then he said: dp.sa.SECRETTOKENVALUE12345 — oops";
        let hits = r.find_leaks(body);
        assert!(!hits.is_empty(), "registry should have caught the leak");
    }

    #[test]
    fn validate_misses_unrelated_text() {
        let mut r = Registry::default();
        r.grow(["dp.sa.LEAKEDTOKEN"]);
        let body = "this body has no secrets at all";
        assert!(r.find_leaks(body).is_empty());
    }

    #[test]
    fn short_literals_are_skipped_on_grow() {
        let mut r = Registry::default();
        r.grow(["", "ab", "abc", "1234567"]);
        assert!(r.is_empty());
    }

    #[test]
    fn grow_accepts_literals_at_min_length_boundary() {
        let mut r = Registry::default();
        r.grow(["12345678"]);
        assert_eq!(r.len(), 1);
    }

    #[test]
    fn long_literal_beyond_old_96_cap_is_matchable() {
        // A 150-byte literal — longer than the old hard-coded max_len
        // that used to bound `find_leaks`. Must still be caught.
        let long = "x".repeat(150);
        let mut r = Registry::default();
        r.grow([long.as_str()]);

        let body = format!("preamble {long} postamble");
        let hits = r.find_leaks(&body);
        assert!(
            !hits.is_empty(),
            "registry must catch literals longer than any hard-coded max"
        );
    }

    #[test]
    fn find_leaks_only_iterates_tracked_lengths() {
        // Record two distinct literal lengths. The scan must only probe
        // those two window sizes, not every length in between.
        let mut r = Registry::default();
        r.grow(["12345678"]); // 8 bytes
        r.grow(["abcdefghijklmnop"]); // 16 bytes

        let body = "noise 12345678 more noise abcdefghijklmnop tail";
        let hits = r.find_leaks(body);
        assert_eq!(hits.len(), 2, "both literals should be caught");
    }

    #[test]
    fn round_trip_through_disk_preserves_hashes_and_lengths() {
        let path = tmp("round-trip");

        let mut r = Registry::default();
        r.grow(["sk-ant-LEAKEDKEYVALUE12345"]);
        r.note_category("CRED");
        r.save(&path).unwrap();

        let loaded = Registry::load(&path).unwrap();
        assert_eq!(loaded.len(), 1);

        // The length set round-tripped too
        assert!(
            !loaded.lengths.is_empty(),
            "lengths should round-trip through disk"
        );

        // Plaintext must NOT be in the file on disk.
        let raw = std::fs::read_to_string(&path).unwrap();
        assert!(!raw.contains("LEAKEDKEY"));
        assert!(raw.contains("CRED"));
    }

    #[test]
    fn missing_file_loads_as_empty() {
        let path = tmp("missing").with_file_name("absent.json");
        let r = Registry::load(&path).unwrap();
        assert!(r.is_empty());
    }

    #[test]
    fn legacy_registry_without_lengths_falls_back_to_wide_scan() {
        // Simulate a pre-length-tracking registry: hashes populated,
        // lengths empty. The scan must still find a known literal.
        let mut r = Registry::default();
        r.grow(["dp.sa.FAKESECRET12345"]);
        // Wipe the length set to simulate a loaded-from-disk legacy file.
        r.lengths.clear();

        let body = "leak here: dp.sa.FAKESECRET12345 ok";
        let hits = r.find_leaks(body);
        assert!(!hits.is_empty(), "legacy fallback must still catch leaks");
    }

    #[test]
    fn doppler_token_shape_round_trip() {
        let token = "dp.sa.fake_doppler_service_account_token_for_test";
        let mut r = Registry::default();
        r.grow([token]);

        let entry_body = format!("Claude said: \"{token}\" — and then echoed it 4 more times");
        let leaks = r.find_leaks(&entry_body);
        assert!(!leaks.is_empty(), "Doppler-shaped leak must be caught");
    }
}
