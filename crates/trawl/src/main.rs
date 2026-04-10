//! Trawl — ZFC redesign.
//!
//! Two stages, both ZFC:
//!
//!   1. Sonnet extractor — one call per session, reads the raw JSONL,
//!      returns a JSON array of draft moments.
//!   2. Haiku tokeniser — one call per draft, replaces every PII span
//!      with `#TYPE_NNN#` placeholders and emits a sidecar entity graph.
//!
//! There is no Rust framework cognition: no sliding windows, no role
//! enums, no operational-pattern blacklist, no regex anonymizer, no
//! 8-dim scoring rubric, no overlap dedup. The model is the framework.
//!
//! Persistence: a state file (`content/.trawl-state.json`) hashes
//! file content + both prompts + both backend signatures + crate
//! version to decide whether a session has already been trawled. A PII registry
//! (`content/.pii-registry.json`) records SHA-256 digests of literal
//! PII the tokeniser has flagged across all runs and validates every
//! new draft against the cumulative knowledge.

use anyhow::{Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc;
use trawl::llm::StageConfig;
use uuid::Uuid;

use trawl::entry::{Entry, Source};
use trawl::extractor;
use trawl::registry::{self, Registry};
use trawl::state::{self, SessionRecord, State, atomic_write_exclusive, sha256_file, sha256_hex};
use trawl::tokeniser::{self, TokenisedEntry};

const EXTRACTOR_PROMPT: &str = include_str!("../prompts/extractor.md");
const TOKENISER_PROMPT: &str = include_str!("../prompts/tokeniser.md");

#[derive(Parser)]
#[command(
    name = "trawl",
    version,
    about = "ZFC session miner for The Daily Claude"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,

    /// Path to a session JSONL file or a directory of sessions.
    /// Used when no subcommand is supplied (default = trawl).
    #[arg(default_value = "")]
    path: String,

    /// Output directory for extracted entry files.
    #[arg(short, long, default_value = "content/entries")]
    output: PathBuf,

    /// Content root that holds the state file and PII registry.
    #[arg(long, default_value = "content")]
    content_root: PathBuf,

    /// Extractor model in `provider/model` form.
    ///
    /// Examples:
    /// - `claude-code/claude-opus-4-6`
    /// - `codex/gpt-5.4-codex`
    /// - `gemini/gemini-3.1-pro-preview`
    /// - `opencode/kimi-for-coding/k2p5`
    /// - `pi/zai-coding-plan/glm-5.1`
    ///
    /// Bare names like `sonnet` are still accepted and treated as
    /// `claude-code/sonnet`.
    #[arg(long, default_value = "claude-code/sonnet", verbatim_doc_comment)]
    extractor_model: String,

    /// Tokeniser model in `provider/model` form.
    ///
    /// Examples:
    /// - `claude-code/claude-sonnet-4-6`
    /// - `codex/gpt-5.4-codex`
    /// - `gemini/gemini-2.5-flash`
    /// - `opencode/minimax-coding-plan/MiniMax-M2.7`
    /// - `pi/zai-coding-plan/glm-5.1`
    ///
    /// Bare names like `haiku` are still accepted and treated as
    /// `claude-code/haiku`.
    #[arg(
        long,
        default_value = "claude-code/haiku",
        visible_alias = "tokenizer-model",
        verbatim_doc_comment
    )]
    tokeniser_model: String,

    /// Optional extractor reasoning effort.
    ///
    /// Accepted values: `min`, `medium`, `high`.
    /// Supported by `claude-code`, `codex`, and `opencode`.
    #[arg(long, verbatim_doc_comment)]
    extractor_effort: Option<String>,

    /// Optional tokeniser reasoning effort.
    ///
    /// Accepted values: `min`, `medium`, `high`.
    /// Supported by `claude-code`, `codex`, and `opencode`.
    #[arg(long, visible_alias = "tokenizer-effort", verbatim_doc_comment)]
    tokeniser_effort: Option<String>,

    /// Concurrency limit for parallel session processing.
    #[arg(long, default_value = "2")]
    concurrency: usize,

    /// Parse and report without writing files or calling the model.
    #[arg(long)]
    dry_run: bool,
}

#[derive(Subcommand)]
enum Command {
    /// Walk sessions, extract moments, tokenise, write entries (default).
    Trawl { path: PathBuf },
    /// Re-validate every existing entry against the current PII registry.
    /// Prints any entry whose body contains a literal the registry has
    /// ever flagged and exits non-zero. Does NOT modify files — the
    /// operator is expected to inspect the flagged entries and fix or
    /// re-tokenise them by hand.
    Validate {
        #[arg(default_value = "content/entries")]
        entries_dir: PathBuf,
    },
    /// Print stats about a session tree without trawling.
    Stats { path: PathBuf },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match &cli.command {
        Some(Command::Validate { entries_dir }) => {
            let entries_dir = entries_dir.clone();
            run_validate(&cli, &entries_dir)
        }
        Some(Command::Stats { path }) => {
            let path = path.clone();
            run_stats(&path)
        }
        Some(Command::Trawl { path }) => {
            let path = path.clone();
            run_trawl(&cli, &path)
        }
        None => {
            if cli.path.is_empty() {
                eprintln!("usage: trawl <path>  (or: trawl trawl|validate|stats)");
                std::process::exit(2);
            }
            let path = PathBuf::from(&cli.path);
            run_trawl(&cli, &path)
        }
    }
}

// ─── trawl (default) ────────────────────────────────────────────────

fn run_trawl(cli: &Cli, path: &Path) -> Result<()> {
    let extractor_stage = StageConfig::parse(&cli.extractor_model, cli.extractor_effort.as_deref())
        .context("parse extractor model config")?;
    let tokeniser_stage = StageConfig::parse(&cli.tokeniser_model, cli.tokeniser_effort.as_deref())
        .context("parse tokeniser model config")?;
    extractor_stage.preflight()?;
    tokeniser_stage.preflight()?;

    let files = collect_session_files(path)?;
    println!("Found {} session files", files.len());

    let state_path = state::default_state_path(&cli.content_root);
    let registry_path = registry::default_registry_path(&cli.content_root);

    let mut state = State::load(&state_path)?;
    let mut reg = Registry::load(&registry_path)?;
    println!(
        "State: {} known sessions | Registry: {} known PII hashes",
        state.sessions.len(),
        reg.len()
    );

    let extractor_sha = sha256_hex(EXTRACTOR_PROMPT.as_bytes());
    let tokeniser_sha = sha256_hex(TOKENISER_PROMPT.as_bytes());
    let extractor_backend_signature = extractor_stage.signature();
    let tokeniser_backend_signature = tokeniser_stage.signature();

    // Compute next free entry number once, then increment locally as we
    // write. Trusted because all writes go through this binary.
    let mut next_number = max_existing_entry_number(&cli.output) + 1;
    let mut total_extracted = 0u32;
    let mut total_skipped_fresh = 0u32;
    let mut total_failed = 0u32;

    // Phase 1: prune the work list — drop sessions whose state is fresh.
    let mut to_process: Vec<(PathBuf, String)> = Vec::new();
    for file in &files {
        let file_sha = match sha256_file(file) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("  hash failed for {}: {e}", file.display());
                total_failed += 1;
                continue;
            }
        };
        if state.is_fresh(
            file,
            &file_sha,
            &extractor_sha,
            &tokeniser_sha,
            &extractor_backend_signature,
            &tokeniser_backend_signature,
        ) {
            total_skipped_fresh += 1;
            continue;
        }
        to_process.push((file.clone(), file_sha));
    }

    println!(
        "Sessions to process: {} (skipping {} fresh)",
        to_process.len(),
        total_skipped_fresh
    );

    if cli.dry_run {
        for (path, _) in &to_process {
            println!("  would trawl: {}", path.display());
        }
        return Ok(());
    }

    // Phase 2: parallel session extraction with a work-stealing pool.
    // Each worker calls Sonnet, then in-thread runs the Haiku tokeniser
    // for each draft and emits a `SessionResult` over the channel.
    let concurrency = cli.concurrency.max(1);
    let work_queue: Mutex<Vec<usize>> = Mutex::new((0..to_process.len()).rev().collect());

    type SessionResult = Result<(usize, String, Vec<TokenisedDraft>)>;
    let (tx, rx) = mpsc::channel::<SessionResult>();

    std::thread::scope(|s| {
        for _ in 0..concurrency {
            let tx = tx.clone();
            let to_process = &to_process;
            let work_queue = &work_queue;
            let extractor_stage = &extractor_stage;
            let tokeniser_stage = &tokeniser_stage;

            s.spawn(move || {
                loop {
                    let idx = {
                        let mut q = work_queue.lock().expect("queue poisoned");
                        q.pop()
                    };
                    let Some(idx) = idx else { break };
                    let (session_path, file_sha) = &to_process[idx];

                    let result =
                        process_session(session_path, file_sha, extractor_stage, tokeniser_stage);
                    if tx
                        .send(result.map(|drafts| (idx, file_sha.clone(), drafts)))
                        .is_err()
                    {
                        break;
                    }
                }
            });
        }
        drop(tx);

        while let Ok(result) = rx.recv() {
            match result {
                Ok((idx, file_sha, drafts)) => {
                    let session_path = &to_process[idx].0;
                    let mut written = Vec::new();

                    for draft in drafts {
                        // Grow registry from the literals the model
                        // implicitly flagged (the diff between source
                        // quote and tokenised body, captured upstream)
                        reg.grow(draft.flagged_literals.clone());

                        // Validate every tokenised user-visible field
                        // against the cumulative registry. Title, tags,
                        // category, and body all ship to disk, so all
                        // of them need the safety-net scan. The registry
                        // decides which window sizes to probe based on
                        // the literal lengths it has ever seen.
                        let mut leak_hits = Vec::new();
                        leak_hits.extend(reg.find_leaks(&draft.tokenised.title));
                        leak_hits.extend(reg.find_leaks(&draft.tokenised.category));
                        for tag in &draft.tokenised.tags {
                            leak_hits.extend(reg.find_leaks(tag));
                        }
                        leak_hits.extend(reg.find_leaks(&draft.tokenised.body));
                        let needs_review = draft.tokenised.needs_review || !leak_hits.is_empty();

                        let project = derive_project_name(session_path);
                        let session_id = derive_session_id(session_path);

                        let entry = Entry {
                            id: Uuid::now_v7(),
                            // All user-visible metadata comes from the
                            // tokeniser, never the raw extractor draft —
                            // titles, categories, and tags can carry PII
                            // just as easily as the body and must go
                            // through the same scrub.
                            title: draft.tokenised.title.clone(),
                            project: project.clone(),
                            category: draft.tokenised.category.clone(),
                            tags: draft.tokenised.tags.clone(),
                            source: Source {
                                source_type: "session".to_string(),
                                session_id: Some(session_id),
                                project_path: None,
                                message_range: None,
                                extracted_at: Utc::now(),
                            },
                            needs_manual_review: needs_review,
                            review_reason: draft
                                .tokenised
                                .review_reason
                                .clone()
                                .or_else(|| {
                                    if !leak_hits.is_empty() {
                                        Some(format!(
                                            "registry caught {} known PII hash(es) across title/category/tags/body",
                                            leak_hits.len()
                                        ))
                                    } else {
                                        None
                                    }
                                }),
                            entities: draft.tokenised.entities.clone(),
                            body: draft.tokenised.body.clone(),
                        };

                        // Probe-and-retry against concurrent trawl runs.
                        // `next_number` is only an advisory lower bound:
                        // another process may have already claimed this
                        // slot on disk. `atomic_write_exclusive` uses
                        // POSIX `link(2)` for race-safe create-new; on
                        // EEXIST we bump the number and try again.
                        let markdown = entry.to_markdown();
                        let body_bytes = markdown.as_bytes();
                        let start_number = next_number;
                        let mut write_attempt = 0u32;
                        let final_filename = loop {
                            if write_attempt >= 1024 {
                                eprintln!(
                                    "  write failed: exhausted 1024 retries finding a free entry number (started at {start_number}, reached {next_number})"
                                );
                                total_failed += 1;
                                break None;
                            }
                            let filename = entry.filename(next_number);
                            let path = cli.output.join(&filename);
                            match atomic_write_exclusive(&path, body_bytes) {
                                Ok(true) => break Some(filename),
                                Ok(false) => {
                                    // Another writer has this slot —
                                    // renumber and retry.
                                    next_number += 1;
                                    write_attempt += 1;
                                }
                                Err(e) => {
                                    eprintln!("  write failed for {filename}: {e:#}");
                                    total_failed += 1;
                                    break None;
                                }
                            }
                        };

                        let Some(filename) = final_filename else {
                            continue;
                        };

                        written.push(filename.clone());
                        next_number += 1;
                        total_extracted += 1;

                        let flag = if needs_review { " (REVIEW)" } else { "" };
                        println!("  extracted: {filename}{flag}");
                    }

                    // Record state for this session even if no entries
                    // were extracted — empty result is also a result.
                    let metadata = std::fs::metadata(session_path).ok();
                    state.record(
                        session_path,
                        SessionRecord {
                            file_sha256: file_sha,
                            size_bytes: metadata.as_ref().map(|m| m.len()).unwrap_or(0),
                            mtime: metadata
                                .as_ref()
                                .and_then(|m| m.modified().ok())
                                .map(|t| {
                                    chrono::DateTime::<Utc>::from(t)
                                        .format("%Y-%m-%dT%H:%M:%SZ")
                                        .to_string()
                                })
                                .unwrap_or_default(),
                            extractor_prompt_sha256: extractor_sha.clone(),
                            tokeniser_prompt_sha256: tokeniser_sha.clone(),
                            extractor_backend_signature: extractor_backend_signature.clone(),
                            tokeniser_backend_signature: tokeniser_backend_signature.clone(),
                            trawl_version: state::CRATE_VERSION.to_string(),
                            extracted_entry_files: written,
                        },
                    );
                }
                Err(e) => {
                    eprintln!("  session failed: {e:#}");
                    total_failed += 1;
                }
            }
        }
    });

    // Persist state + registry after the run finishes.
    state.save(&state_path)?;
    reg.save(&registry_path)?;

    println!("\nSummary:");
    println!("  Extracted entries: {total_extracted}");
    println!("  Sessions skipped (fresh): {total_skipped_fresh}");
    println!("  Failures: {total_failed}");
    println!("  State: {}", state_path.display());
    println!("  Registry: {}", registry_path.display());

    Ok(())
}

/// One draft after both stages — the tokenised output plus the literals
/// the upstream extractor included verbatim that the tokeniser must have
/// considered PII. Only the tokenised output is ever written to disk;
/// the extractor's raw draft is dropped after the diff runs because it
/// may still contain un-scrubbed PII.
struct TokenisedDraft {
    tokenised: TokenisedEntry,
    flagged_literals: Vec<String>,
}

fn process_session(
    session_path: &Path,
    _file_sha: &str,
    extractor_stage: &StageConfig,
    tokeniser_stage: &StageConfig,
) -> Result<Vec<TokenisedDraft>> {
    let drafts = extractor::extract_session(session_path, extractor_stage)
        .with_context(|| format!("extract {}", session_path.display()))?;

    // Tolerant per-draft tokenisation: a single Haiku failure must not
    // sink every other moment Sonnet found in this session. We log the
    // failure and keep going.
    let mut out = Vec::with_capacity(drafts.len());
    for draft in drafts {
        match tokeniser::tokenise_entry(&draft, tokeniser_stage) {
            Ok(tokenised) => {
                // Compare the full extractor draft against the full
                // tokenised output so the registry grows to include
                // literals that were masked anywhere — title, tags,
                // category, body. The flagged set feeds the deterministic
                // backstop for future runs.
                let mut flagged = diff_literals(&draft.quote, &tokenised.body);
                flagged.extend(diff_literals(&draft.title, &tokenised.title));
                flagged.extend(diff_literals(&draft.category, &tokenised.category));
                for (src, dst) in draft.tags.iter().zip(tokenised.tags.iter()) {
                    flagged.extend(diff_literals(src, dst));
                }
                out.push(TokenisedDraft {
                    tokenised,
                    flagged_literals: flagged,
                });
            }
            Err(e) => {
                // Intentionally do NOT include draft.title here — the
                // extractor allows real PII in metadata until the
                // tokeniser runs, and this failure path fires exactly
                // when the tokeniser didn't produce usable output. Log
                // only structural info so stderr/CI logs stay PII-free.
                eprintln!(
                    "  tokenise draft failed: {e:#} (skipping this draft, keeping session alive)"
                );
            }
        }
    }
    Ok(out)
}

/// Heuristic: any token in the original quote that no longer appears in
/// the tokenised body is something the model masked. Used to grow the
/// registry. We split on whitespace and punctuation to keep it simple —
/// false negatives are fine here, the registry is a backstop.
///
/// The minimum length threshold is tied to `registry::MIN_LITERAL_LEN`
/// so we don't bother reporting literals that the registry would drop
/// anyway. Any caller that wants to change the cutoff updates it in
/// one place.
fn diff_literals(original: &str, tokenised: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in original.split(|c: char| {
        c.is_whitespace()
            || matches!(
                c,
                '\'' | '"' | '`' | ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}'
            )
    }) {
        let trimmed = raw.trim_matches(|c: char| {
            !c.is_alphanumeric() && c != '.' && c != '-' && c != '_' && c != '/' && c != '@'
        });
        if trimmed.len() < registry::MIN_LITERAL_LEN {
            continue;
        }
        if !tokenised.contains(trimmed) {
            out.push(trimmed.to_string());
        }
    }
    out
}

// ─── validate subcommand ────────────────────────────────────────────

fn run_validate(cli: &Cli, entries_dir: &Path) -> Result<()> {
    let registry_path = registry::default_registry_path(&cli.content_root);
    let reg = Registry::load(&registry_path)?;

    if reg.is_empty() {
        println!("Registry is empty — nothing to validate against.");
        return Ok(());
    }

    let mut leaked = 0u32;
    let mut clean = 0u32;
    for entry in walk_markdown(entries_dir)? {
        let raw =
            std::fs::read_to_string(&entry).with_context(|| format!("read {}", entry.display()))?;
        let scannable = extract_scannable_content(&raw);
        let hits = reg.find_leaks(&scannable);
        if !hits.is_empty() {
            leaked += 1;
            println!(
                "  LEAK: {} ({} hash hit(s)) — needs manual review (not modified)",
                entry.display(),
                hits.len()
            );
        } else {
            clean += 1;
        }
    }

    println!("\nValidate complete: {clean} clean / {leaked} leaked");
    if leaked > 0 {
        std::process::exit(1);
    }
    Ok(())
}

/// Isolate the parts of a serialised entry that can contain PII from
/// the structural YAML frontmatter that cannot.
///
/// Entries on disk look like:
///
/// ```text
/// ---
/// title: ...
/// category: ...
/// tags: [...]
/// <other fields>
/// ---
///
/// <body>
/// ```
///
/// We scan: the title/category/tags values (user-visible metadata that
/// goes through the tokeniser) plus the body (everything after the
/// closing frontmatter delimiter). Structural frontmatter fields like
/// `id`, `source_type`, `extracted_at`, `message_range`, and the entity
/// graph are skipped — they cannot contain real PII (they are either
/// placeholders, UUIDs, timestamps, or enums) and scanning them
/// produces false positives against the registry's hash set.
///
/// The parser is deliberately forgiving: if the frontmatter is missing
/// or malformed, we fall back to scanning the entire file so a bad
/// entry format never silently bypasses the safety net.
fn extract_scannable_content(raw: &str) -> String {
    let Some(stripped) = raw.strip_prefix("---\n") else {
        return raw.to_string();
    };
    let Some(end) = stripped.find("\n---") else {
        return raw.to_string();
    };
    let (frontmatter, rest) = stripped.split_at(end);
    let body = rest
        .strip_prefix("\n---")
        .unwrap_or(rest)
        .trim_start_matches('\n');

    // Cheap line-scan — we only care about three keys. serde_yaml would
    // also work but pulls in full YAML semantics for values we don't
    // need (and would choke on the embedded JSON `entities` graph).
    let mut scannable = String::with_capacity(body.len() + 256);
    scannable.push_str(body);
    for line in frontmatter.lines() {
        if let Some(rest) = line.strip_prefix("title:") {
            scannable.push('\n');
            scannable.push_str(rest.trim());
        } else if let Some(rest) = line.strip_prefix("category:") {
            scannable.push('\n');
            scannable.push_str(rest.trim());
        } else if let Some(rest) = line.strip_prefix("tags:") {
            // Inline YAML list form: `tags: [a, b, c]`
            scannable.push('\n');
            scannable.push_str(rest.trim());
        } else if line.starts_with("- ") {
            // Block-form YAML list entry — harmless false-positive risk
            // since we don't know which key it belongs to, but tags are
            // usually user-facing so we include them for the safety net.
            scannable.push('\n');
            scannable.push_str(line.trim_start_matches("- ").trim());
        }
    }
    scannable
}

// ─── stats subcommand ───────────────────────────────────────────────

fn run_stats(path: &Path) -> Result<()> {
    let files = collect_session_files(path)?;
    println!("Session files: {}", files.len());
    let mut total: u64 = 0;
    for f in &files {
        if let Ok(meta) = std::fs::metadata(f) {
            total += meta.len();
        }
    }
    println!("Total bytes: {total}");
    Ok(())
}

// ─── shared helpers ─────────────────────────────────────────────────

fn collect_session_files(path: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();

    if path.is_file() {
        files.push(path.to_path_buf());
    } else if path.is_dir() {
        for entry in walkdir::WalkDir::new(path)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("jsonl")
                && !p.to_string_lossy().contains("subagents")
            {
                files.push(p.to_path_buf());
            }
        }
    } else {
        return Err(anyhow::anyhow!(
            "session path does not exist: {}",
            path.display()
        ));
    }

    files.sort();
    Ok(files)
}

fn walk_markdown(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !dir.exists() {
        return Ok(out);
    }
    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let p = entry.path();
        if p.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(p.to_path_buf());
        }
    }
    out.sort();
    Ok(out)
}

/// Returns the maximum N from filenames of the form `NNN-slug.md` in
/// `dir`, or 0 if the directory is empty or missing.
fn max_existing_entry_number(dir: &Path) -> u32 {
    if !dir.exists() {
        return 0;
    }
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .filter_map(|e| {
                    let name = e.file_name();
                    let s = name.to_str()?.to_string();
                    if !s.ends_with(".md") {
                        return None;
                    }
                    let (num_str, _) = s.split_once('-')?;
                    num_str.parse::<u32>().ok()
                })
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0)
}

fn derive_project_name(session_path: &Path) -> String {
    // Claude Code project paths look like:
    //   .../projects/-Users-<user>-Projects-<org>-<repo>/<uuid>.jsonl
    // We use the parent directory's last 2 dash-segments as a display
    // name. This is a heuristic, not security-sensitive — the tokeniser
    // will scrub anything that needs scrubbing.
    let parent = session_path
        .parent()
        .and_then(|p| p.file_name())
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");

    parent
        .rsplit('-')
        .take(2)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("-")
}

fn derive_session_id(session_path: &Path) -> String {
    session_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_literals_finds_what_was_masked() {
        // All fixture literals are ≥ 8 bytes (registry::MIN_LITERAL_LEN)
        // so the scan surfaces them rather than filtering them out.
        let original = "the user thomassen connected to dp.sa.SECRETKEY12345 from stockholm";
        let tokenised = "the user #USER_001# connected to #CRED_001# from #CITY_001#";
        let leaked = diff_literals(original, tokenised);
        assert!(
            leaked.iter().any(|s| s.contains("thomassen")),
            "long name should be caught: {leaked:?}"
        );
        assert!(
            leaked.iter().any(|s| s.contains("dp.sa")),
            "credential shape should be caught: {leaked:?}"
        );
        assert!(
            leaked.iter().any(|s| s == "stockholm"),
            "city name should be caught: {leaked:?}"
        );
    }

    #[test]
    fn diff_literals_skips_short_words() {
        let original = "to a is the on at";
        let tokenised = "fully gone";
        // All source words are short — none should appear in the diff
        let leaked = diff_literals(original, tokenised);
        assert!(leaked.is_empty(), "got: {leaked:?}");
    }

    #[test]
    fn derive_project_name_takes_last_two_segments() {
        let p = PathBuf::from(
            "/Users/x/.claude/projects/-Users-alice-Projects-example-the-daily-claude/abc.jsonl",
        );
        let name = derive_project_name(&p);
        assert_eq!(name, "daily-claude");
    }

    #[test]
    fn max_existing_entry_number_handles_missing_dir() {
        let dir = std::env::temp_dir().join(format!("trawl-no-such-{}", std::process::id()));
        assert_eq!(max_existing_entry_number(&dir), 0);
    }

    #[test]
    fn scannable_content_includes_body_and_user_metadata() {
        let raw = "---\n\
            id: 019d6a53-1a27-74a2-8367-033f8df5f901\n\
            title: The #USER_001# Problem\n\
            category: rage\n\
            tags:\n\
            - regex\n\
            - hubris\n\
            source_type: session\n\
            extracted_at: 2026-04-08T00:00:00Z\n\
            ---\n\
            \n\
            [HUMAN]: hello #USER_001#\n";
        let scan = extract_scannable_content(raw);
        assert!(scan.contains("hello #USER_001#"), "body missing");
        assert!(scan.contains("The #USER_001# Problem"), "title missing");
        assert!(scan.contains("rage"), "category missing");
        assert!(scan.contains("regex"), "tag missing");
        // Structural keys must NOT leak in
        assert!(!scan.contains("019d6a53"), "id leaked into scan: {scan}");
        assert!(!scan.contains("2026-04-08T00:00:00Z"), "timestamp leaked");
    }

    #[test]
    fn scannable_content_falls_back_to_whole_file_when_frontmatter_missing() {
        let raw = "no frontmatter at all, just text with #USER_001#";
        let scan = extract_scannable_content(raw);
        assert_eq!(scan, raw);
    }

    #[test]
    fn scannable_content_handles_inline_tag_list() {
        let raw = "---\n\
            title: t\n\
            category: c\n\
            tags: [one, two, three]\n\
            ---\n\
            \n\
            body content\n";
        let scan = extract_scannable_content(raw);
        assert!(scan.contains("body content"));
        assert!(scan.contains("one, two, three"));
    }
}
