//! Stage 1: Sonnet extractor.
//!
//! Spawns `claude -p --model sonnet --allowedTools Read` against a session
//! JSONL file and parses the resulting JSON array of draft entries. The
//! model handles parsing, role disambiguation, window selection, and
//! dedup implicitly — there is no Rust framework cognition.

use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

const EXTRACTOR_PROMPT: &str = include_str!("../prompts/extractor.md");

/// One draft entry returned by the Sonnet extractor.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftEntry {
    pub title: String,
    pub category: String,
    pub tags: Vec<String>,
    /// Verbatim exchange — alternating `[HUMAN]:` / `[ASSISTANT]:` /
    /// `[THINKING]:` / `[TOOL_INPUT:<name>]:` blocks. Tool *results*
    /// are never quoted (see the exclusion rule in the extractor
    /// prompt). The tokeniser will scrub PII downstream.
    pub quote: String,
    pub why: String,
}

/// Run the Sonnet extractor over a session jsonl file.
///
/// Returns `Ok(vec![])` when the session contains no postable moments.
/// Errors propagate when the subprocess fails or both parse attempts
/// produce invalid JSON.
pub fn extract_session(session_path: &Path, model: &str) -> Result<Vec<DraftEntry>> {
    let abs = std::fs::canonicalize(session_path)
        .with_context(|| format!("canonicalize {}", session_path.display()))?;
    let session_dir = abs
        .parent()
        .ok_or_else(|| anyhow!("session path has no parent: {}", abs.display()))?
        .to_path_buf();

    let prompt = format!(
        "{EXTRACTOR_PROMPT}\n\n## Session to read\n\n`{}`",
        abs.display()
    );

    let raw = run_claude(model, &prompt, &session_dir)?;
    match parse_draft_array(&raw) {
        Ok(entries) => Ok(entries),
        Err(_first_err) => {
            // One automatic retry — strict reminder appended.
            let retry_prompt = format!(
                "{prompt}\n\nReturn ONLY the JSON array. No prose, no markdown fencing."
            );
            let retry = run_claude(model, &retry_prompt, &session_dir)?;
            parse_draft_array(&retry).context("extractor returned invalid JSON after retry")
        }
    }
}

/// Spawn `claude -p` with the prompt piped to stdin and capture stdout.
///
/// We use stdin rather than a positional prompt argument because the
/// `--allowedTools` flag is variadic in current claude-cli and will
/// happily eat a trailing positional. Stdin sidesteps the ambiguity.
///
/// `session_dir` is granted via `--add-dir` so the model's Read tool
/// can reach the absolute jsonl path we asked it to read.
fn run_claude(model: &str, prompt: &str, session_dir: &Path) -> Result<String> {
    let mut child = Command::new("claude")
        .arg("-p")
        .arg("--model")
        .arg(model)
        .arg("--add-dir")
        .arg(session_dir)
        .arg("--allowedTools=Read")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn `claude` — is the CLI installed and on PATH?")?;

    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("failed to open stdin to claude"))?;
        stdin
            .write_all(prompt.as_bytes())
            .context("failed to write prompt to claude stdin")?;
    }

    let output = child.wait_with_output().context("waiting on claude")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "claude exited with status {:?}: {stderr}",
            output.status.code()
        ));
    }

    String::from_utf8(output.stdout).context("claude stdout was not valid UTF-8")
}

/// Robust JSON-array extractor: find the first `[` and the matching last
/// `]` in the response. Tolerates prose preamble and markdown fencing.
///
/// Error context deliberately does **not** include a slice of the raw
/// output. The extractor prompt explicitly allows the model to include
/// real names, paths, and (now-redacted) credentials in its output, so
/// a parse failure that dumped the raw slice into the error chain would
/// leak that plaintext into stderr and CI logs. We log byte positions
/// and candidate length only.
fn parse_draft_array(raw: &str) -> Result<Vec<DraftEntry>> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    // Walk every candidate `[` in the output, try to find a balanced
    // matching `]`, and attempt to deserialise the slice. The first
    // candidate that parses into a `Vec<DraftEntry>` wins. This
    // tolerates a Sonnet preamble like "I found [3] moments:" where
    // the first `[` is prose — serde will reject `[3]` as an invalid
    // array of drafts and we'll keep scanning until we hit the real
    // structured array further down.
    let mut search_from = 0;
    let mut last_attempt_err: Option<(usize, usize, String)> = None;
    loop {
        let Some(rel) = trimmed[search_from..].find('[') else {
            break;
        };
        let start = search_from + rel;
        if let Some(end) = find_matching_bracket(trimmed, start, b'[', b']') {
            let slice = &trimmed[start..=end];
            match serde_json::from_str::<Vec<DraftEntry>>(slice) {
                Ok(entries) => return Ok(entries),
                Err(_) => {
                    last_attempt_err = Some((start, end, format!("{}", slice.len())));
                }
            }
        }
        search_from = start + 1;
    }

    match last_attempt_err {
        Some((start, end, length)) => Err(anyhow!(
            "no candidate JSON array in extractor output parsed successfully (last attempt: byte range {start}..={end}, candidate length {length})"
        )),
        None => Err(anyhow!("no opening bracket in extractor output")),
    }
}

/// Walk forward from an opening bracket or brace and return the byte
/// index of its matching close, or `None` if unbalanced. Tracks depth
/// across nested structures and skips bracket characters that appear
/// inside JSON string literals (honoring backslash escapes).
///
/// Shared by the extractor and tokeniser parsers so both speak the
/// same dialect of "where does this JSON candidate end".
pub(crate) fn find_matching_bracket(
    s: &str,
    start: usize,
    open: u8,
    close: u8,
) -> Option<usize> {
    let bytes = s.as_bytes();
    if start >= bytes.len() || bytes[start] != open {
        return None;
    }

    let mut depth: i32 = 0;
    let mut in_string = false;
    let mut escape = false;
    let mut i = start;
    while i < bytes.len() {
        let b = bytes[i];
        if in_string {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                in_string = false;
            }
        } else if b == b'"' {
            in_string = true;
        } else if b == open {
            depth += 1;
        } else if b == close {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_clean_json_array() {
        let raw = r#"[
            {"title":"t","category":"c","tags":["a"],"quote":"q","why":"w"}
        ]"#;
        let entries = parse_draft_array(raw).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "t");
    }

    #[test]
    fn parses_empty_array_as_no_moments() {
        let entries = parse_draft_array("[]").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn tolerates_prose_preamble_and_markdown_fencing() {
        let raw = r#"
        Here is the array you asked for:
        ```json
        [{"title":"t","category":"c","tags":[],"quote":"q","why":"w"}]
        ```
        "#;
        let entries = parse_draft_array(raw).unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn empty_response_returns_empty_vec() {
        assert!(parse_draft_array("").unwrap().is_empty());
        assert!(parse_draft_array("   \n  ").unwrap().is_empty());
    }

    #[test]
    fn missing_brackets_errors() {
        let err = parse_draft_array("totally not json").unwrap_err();
        assert!(err.to_string().contains("opening bracket"));
    }

    #[test]
    fn malformed_json_errors() {
        let err = parse_draft_array("[ {bad json} ]").unwrap_err();
        assert!(
            err.to_string().contains("no candidate JSON array"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parser_skips_bracket_in_prose_preamble() {
        // Sonnet sometimes narrates before the array. A naive
        // find('[') would land on the "[3]" in the prose and fail.
        let raw = "I found [3] moments worth extracting:\n\n\
            [\n\
            {\"title\":\"t\",\"category\":\"c\",\"tags\":[\"x\"],\"quote\":\"q\",\"why\":\"w\"}\n\
            ]";
        let entries = parse_draft_array(raw).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].title, "t");
    }

    #[test]
    fn parser_skips_bracket_in_prose_postamble() {
        let raw = "[\n\
            {\"title\":\"t\",\"category\":\"c\",\"tags\":[],\"quote\":\"q\",\"why\":\"w\"}\n\
            ]\n\n\
            That's [1] moment found, hope this helps!";
        let entries = parse_draft_array(raw).unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn parse_error_does_not_leak_raw_output() {
        // The extractor prompt allows real names/paths/credentials in
        // the model's output until the tokeniser runs. A failed parse
        // must not paste that plaintext into the error chain.
        let secret_bearing =
            r##"[ {"title":"user alice shipped dp.sa.SECRETTOKEN","invalid": ""##;
        let err = parse_draft_array(secret_bearing).unwrap_err();
        let chain = format!("{err:#}");
        assert!(
            !chain.contains("SECRETTOKEN"),
            "error chain leaked raw content: {chain}"
        );
        assert!(
            !chain.contains("dp.sa"),
            "error chain leaked credential shape: {chain}"
        );
        assert!(
            !chain.contains("alice"),
            "error chain leaked user name: {chain}"
        );
    }
}
