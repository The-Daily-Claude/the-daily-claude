//! Stage 2: tokeniser.
//!
//! Replaces every PII span in a draft entry with `#TYPE_NNN#` placeholders
//! and emits a sidecar entity graph describing the relationships between
//! placeholders. The graph never contains real values — types and links
//! only. The placeholder is the canonical form on disk; downstream
//! consumers of this output pick friendly substitutes at render time.
//!
//! The tokeniser takes the **whole draft** — title, category, tags, quote —
//! and returns all four fields tokenised. This keeps diarisation consistent
//! (a name mentioned in both title and quote gets the same placeholder)
//! and prevents PII from slipping through metadata channels the way it
//! would if only the quote were tokenised.

use crate::extractor::{DraftEntry, find_matching_bracket};
use crate::llm::StageConfig;
use anyhow::{Context, Result, anyhow};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const TOKENISER_PROMPT: &str = include_str!("../prompts/tokeniser.md");

/// What the Haiku tokeniser returns for a draft entry — every user-
/// visible string field is tokenised, plus the sidecar entity graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenisedEntry {
    /// Tokenised entry title. Same placeholder conventions as `body`.
    pub title: String,
    /// Tokenised category. Usually unchanged, but a custom category
    /// like "The alice arc" would get tokenised too.
    pub category: String,
    /// Tokenised tags. Tags referencing people, orgs, or private
    /// identifiers get placeholderised alongside the rest.
    pub tags: Vec<String>,
    /// Tokenised entry body.
    pub body: String,
    /// Sidecar entity graph: placeholder → metadata. Never contains
    /// real values.
    #[serde(default)]
    pub entities: Value,
    /// Set when the model is unsure about anything — including a
    /// possibly-credential string or a name that might be a real person.
    #[serde(default)]
    pub needs_review: bool,
    #[serde(default)]
    pub review_reason: Option<String>,
}

/// Run the Haiku tokeniser over a full draft entry.
///
/// Every identifying field (title, category, tags, quote body) goes
/// through the same tokeniser call so placeholder ids stay consistent
/// across fields — a single person mentioned in both title and body
/// becomes the same `#USER_001#`.
pub fn tokenise_entry(draft: &DraftEntry, stage: &StageConfig) -> Result<TokenisedEntry> {
    let draft_json = serde_json::to_string_pretty(&DraftPayload {
        title: &draft.title,
        category: &draft.category,
        tags: &draft.tags,
        quote: &draft.quote,
    })
    .context("serialise draft for tokeniser")?;

    let prompt =
        format!("{TOKENISER_PROMPT}\n\n## Draft to tokenise\n\n```json\n{draft_json}\n```");

    let raw = stage.run_tokeniser(&prompt)?;
    match parse_tokenised(&raw) {
        Ok(t) => Ok(t),
        Err(_) => {
            let retry_prompt =
                format!("{prompt}\n\nReturn ONLY the JSON object. No prose, no markdown fencing.");
            let retry = stage.run_tokeniser(&retry_prompt)?;
            parse_tokenised(&retry).context("tokeniser returned invalid JSON after retry")
        }
    }
}

/// Subset of `DraftEntry` that we show the tokeniser — drops `why`
/// since the editor's rationale is internal and never published.
#[derive(Serialize)]
struct DraftPayload<'a> {
    title: &'a str,
    category: &'a str,
    tags: &'a [String],
    quote: &'a str,
}

/// Robust JSON-object extractor — find the first `{` and the matching
/// last `}`. Tolerates prose preamble and markdown fencing.
///
/// Error context deliberately does **not** include a slice of the raw
/// output: the tokeniser's input can contain PII that the model is
/// supposed to scrub, and a failed scrub should not leak plaintext
/// through our error messages into stderr or CI logs. We log only the
/// byte positions and the candidate length.
fn parse_tokenised(raw: &str) -> Result<TokenisedEntry> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("tokeniser produced empty output"));
    }

    // Balanced-brace scan across every candidate `{`. The first one
    // that yields a balanced slice AND parses as a `TokenisedEntry`
    // wins. Nested objects (the `entities` map) and prose braces
    // (`{just a sec}` in a preamble) are both handled — the scan
    // tracks depth and string state, and a candidate that balances
    // but doesn't deserialise is skipped rather than terminating the
    // search.
    let mut search_from = 0;
    let mut last_attempt_err: Option<(usize, usize, String)> = None;
    loop {
        let Some(rel) = trimmed[search_from..].find('{') else {
            break;
        };
        let start = search_from + rel;
        if let Some(end) = find_matching_bracket(trimmed, start, b'{', b'}') {
            let slice = &trimmed[start..=end];
            match serde_json::from_str::<TokenisedEntry>(slice) {
                Ok(t) => return Ok(t),
                Err(_) => {
                    last_attempt_err = Some((start, end, format!("{}", slice.len())));
                }
            }
        }
        search_from = start + 1;
    }

    match last_attempt_err {
        Some((start, end, length)) => Err(anyhow!(
            "no candidate tokeniser JSON object parsed successfully (last attempt: byte range {start}..={end}, candidate length {length})"
        )),
        None => Err(anyhow!("no opening brace in tokeniser output")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_full_tokenised_draft() {
        let raw = r##"{
            "title": "Going Sideways, Again",
            "category": "spectacular-failure",
            "tags": ["regex", "hubris"],
            "body": "hello #USER_001#",
            "entities": {"#USER_001#": {"type": "USER"}},
            "needs_review": false
        }"##;
        let t = parse_tokenised(raw).unwrap();
        assert_eq!(t.title, "Going Sideways, Again");
        assert_eq!(t.category, "spectacular-failure");
        assert_eq!(t.tags, vec!["regex".to_string(), "hubris".to_string()]);
        assert_eq!(t.body, "hello #USER_001#");
        assert!(!t.needs_review);
    }

    #[test]
    fn parses_object_with_review_flag_set() {
        let raw = r#"{
            "title": "redacted",
            "category": "cred",
            "tags": [],
            "body": "redacted",
            "entities": {},
            "needs_review": true,
            "review_reason": "saw a token-shaped string"
        }"#;
        let t = parse_tokenised(raw).unwrap();
        assert!(t.needs_review);
        assert_eq!(
            t.review_reason.as_deref(),
            Some("saw a token-shaped string")
        );
    }

    #[test]
    fn tolerates_markdown_fencing_and_preamble() {
        let raw = r#"
        Here you go:
        ```json
        { "title": "t", "category": "c", "tags": [], "body": "x", "entities": {}, "needs_review": false }
        ```
        "#;
        let t = parse_tokenised(raw).unwrap();
        assert_eq!(t.body, "x");
        assert_eq!(t.title, "t");
    }

    #[test]
    fn missing_brace_errors() {
        let err = parse_tokenised("nope").unwrap_err();
        assert!(err.to_string().contains("opening brace"));
    }

    #[test]
    fn empty_input_errors() {
        let err = parse_tokenised("").unwrap_err();
        assert!(err.to_string().contains("empty"));
    }

    #[test]
    fn entities_are_a_value_so_arbitrary_relations_round_trip() {
        let raw = r##"{
            "title": "t",
            "category": "c",
            "tags": [],
            "body": "in #CITY_001#",
            "entities": {
                "#CITY_001#": {"type": "CITY", "in": "#REGION_002#"},
                "#REGION_002#": {"type": "REGION", "in": "#COUNTRY_003#"}
            },
            "needs_review": false
        }"##;
        let t = parse_tokenised(raw).unwrap();
        let region = t.entities["#CITY_001#"]["in"].as_str().unwrap();
        assert_eq!(region, "#REGION_002#");
    }

    #[test]
    fn parser_skips_brace_in_prose_preamble() {
        let raw = "I'll process this draft {just a sec} and return:\n\n\
            {\n\
            \"title\":\"t\",\"category\":\"c\",\"tags\":[],\"body\":\"x\",\"entities\":{},\"needs_review\":false\n\
            }";
        let t = parse_tokenised(raw).unwrap();
        assert_eq!(t.body, "x");
    }

    #[test]
    fn parser_skips_brace_in_prose_postamble() {
        let raw = "{\"title\":\"t\",\"category\":\"c\",\"tags\":[],\"body\":\"x\",\"entities\":{},\"needs_review\":false}\n\n\
            Processed successfully {no issues}";
        let t = parse_tokenised(raw).unwrap();
        assert_eq!(t.body, "x");
    }

    #[test]
    fn parse_error_does_not_leak_raw_output() {
        // If Haiku ever violates the contract and returns plaintext
        // PII wrapped in a malformed object, the error chain must
        // not contain the raw literal.
        let secret_bearing = r##"{"title":"leaked dp.sa.SECRETTOKEN","this is not valid json"}"##;
        let err = parse_tokenised(secret_bearing).unwrap_err();
        let chain = format!("{err:#}");
        assert!(
            !chain.contains("SECRETTOKEN"),
            "error chain leaked raw content: {chain}"
        );
        assert!(
            !chain.contains("dp.sa"),
            "error chain leaked credential shape: {chain}"
        );
    }
}
