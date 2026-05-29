use anyhow::{Context, Result};
use serde_json::Value;
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

pub struct PreparedSession {
    extractor_path: PathBuf,
    pub project_name: String,
    pub session_id: String,
    _tempfile: Option<NamedTempFile>,
}

impl PreparedSession {
    pub fn extractor_path(&self) -> &Path {
        &self.extractor_path
    }
}

pub fn prepare_for_extractor(session_path: &Path) -> Result<PreparedSession> {
    let abs = std::fs::canonicalize(session_path)
        .with_context(|| format!("canonicalize {}", session_path.display()))?;

    if let Some(meta) = parse_codex_session_meta(&abs)? {
        let transcript = normalise_codex_session(&abs, &meta)?;
        let mut tempfile =
            NamedTempFile::new().context("create temp transcript for Codex session")?;
        tempfile
            .write_all(transcript.as_bytes())
            .context("write normalised Codex transcript")?;
        tempfile
            .flush()
            .context("flush normalised Codex transcript")?;

        return Ok(PreparedSession {
            extractor_path: tempfile.path().to_path_buf(),
            project_name: meta.project_name,
            session_id: meta.session_id,
            _tempfile: Some(tempfile),
        });
    }

    Ok(PreparedSession {
        extractor_path: abs.clone(),
        project_name: derive_claude_project_name(&abs),
        session_id: derive_path_session_id(&abs),
        _tempfile: None,
    })
}

struct CodexSessionMeta {
    project_name: String,
    session_id: String,
}

fn is_codex_session(originator: Option<&str>, source: Option<&str>) -> bool {
    let originator_ok = originator
        .map(|value| value.trim().to_ascii_lowercase().starts_with("codex"))
        .unwrap_or(false);
    let source_ok = source
        .map(|value| matches!(value.trim().to_ascii_lowercase().as_str(), "cli" | "exec"))
        .unwrap_or(false);
    originator_ok && source_ok
}

fn parse_codex_session_meta(session_path: &Path) -> Result<Option<CodexSessionMeta>> {
    let file =
        File::open(session_path).with_context(|| format!("open {}", session_path.display()))?;
    let mut reader = BufReader::new(file);
    let mut line = String::new();
    if reader
        .read_line(&mut line)
        .with_context(|| format!("read {}", session_path.display()))?
        == 0
    {
        return Ok(None);
    }

    let value: Value =
        serde_json::from_str(&line).with_context(|| format!("parse {}", session_path.display()))?;
    if value.get("type").and_then(Value::as_str) != Some("session_meta") {
        return Ok(None);
    }

    let payload = value.get("payload").unwrap_or(&Value::Null);
    let originator = payload.get("originator").and_then(Value::as_str);
    let source = payload.get("source").and_then(Value::as_str);
    if !is_codex_session(originator, source) {
        return Ok(None);
    }

    let session_id = payload
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| derive_path_session_id(session_path));
    let project_name = payload
        .get("git")
        .and_then(|git| git.get("repository_url"))
        .and_then(Value::as_str)
        .and_then(repo_name_from_url)
        .or_else(|| {
            payload
                .get("cwd")
                .and_then(Value::as_str)
                .and_then(|cwd| Path::new(cwd).file_name())
                .and_then(|name| name.to_str())
                .map(str::to_string)
        })
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

    Ok(Some(CodexSessionMeta {
        project_name,
        session_id,
    }))
}

fn normalise_codex_session(session_path: &Path, meta: &CodexSessionMeta) -> Result<String> {
    let file =
        File::open(session_path).with_context(|| format!("open {}", session_path.display()))?;
    let reader = BufReader::new(file);

    let mut call_names: HashMap<String, String> = HashMap::new();
    let mut blocks = Vec::new();

    for line in reader.lines() {
        let line = line.with_context(|| format!("read {}", session_path.display()))?;
        let value: Value = match serde_json::from_str(&line) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if value.get("type").and_then(Value::as_str) != Some("response_item") {
            continue;
        }

        let payload = value.get("payload").unwrap_or(&Value::Null);
        match payload.get("type").and_then(Value::as_str) {
            Some("message") => {
                let role = payload.get("role").and_then(Value::as_str).unwrap_or("");
                let text = extract_message_text(payload);
                if text.trim().is_empty() {
                    continue;
                }
                match role {
                    "user" => blocks.push(format_block("HUMAN", &text)),
                    "assistant" => blocks.push(format_block("ASSISTANT", &text)),
                    _ => {}
                }
            }
            Some("reasoning") => {
                let text = extract_reasoning_summary(payload);
                if !text.trim().is_empty() {
                    blocks.push(format_block("THINKING", &text));
                }
            }
            Some("function_call") => {
                let name = payload
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                if let Some(call_id) = payload.get("call_id").and_then(Value::as_str) {
                    call_names.insert(call_id.to_string(), name.to_string());
                }
                let args = payload
                    .get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                if !args.is_empty() {
                    blocks.push(format_block(&format!("TOOL_INPUT:{name}"), args));
                }
            }
            Some("function_call_output") => {
                let call_id = payload.get("call_id").and_then(Value::as_str).unwrap_or("");
                let name = call_names
                    .get(call_id)
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());
                let output = payload
                    .get("output")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                if !output.is_empty() {
                    blocks.push(format_block(&format!("TOOL_RESULT:{name}"), output));
                }
            }
            Some("custom_tool_call") => {
                let name = payload
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                if let Some(call_id) = payload.get("call_id").and_then(Value::as_str) {
                    call_names.insert(call_id.to_string(), name.to_string());
                }
                let input = payload
                    .get("input")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .trim();
                if !input.is_empty() {
                    blocks.push(format_block(&format!("TOOL_INPUT:{name}"), input));
                }
            }
            Some("custom_tool_call_output") => {
                let call_id = payload.get("call_id").and_then(Value::as_str).unwrap_or("");
                let name = call_names
                    .get(call_id)
                    .cloned()
                    .unwrap_or_else(|| "unknown".to_string());
                let output = payload
                    .get("output")
                    .and_then(Value::as_str)
                    .map(decode_custom_tool_output)
                    .unwrap_or_default();
                if !output.is_empty() {
                    blocks.push(format_block(&format!("TOOL_RESULT:{name}"), &output));
                }
            }
            _ => {}
        }
    }

    let mut transcript = String::from(
        "# Normalized Codex CLI transcript\n\n\
source: codex-cli\n",
    );
    transcript.push_str(&format!("session_id: {}\n", meta.session_id));
    transcript.push_str(&format!("project: {}\n", meta.project_name));
    transcript.push_str(&format!("original_path: {}\n", session_path.display()));
    transcript.push('\n');
    transcript.push_str(&blocks.join("\n\n"));
    transcript.push('\n');
    Ok(transcript)
}

fn extract_message_text(payload: &Value) -> String {
    payload
        .get("content")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| match item.get("type").and_then(Value::as_str) {
            Some("input_text") | Some("output_text") => item.get("text").and_then(Value::as_str),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn extract_reasoning_summary(payload: &Value) -> String {
    payload
        .get("summary")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| {
            item.as_str()
                .or_else(|| item.get("text").and_then(Value::as_str))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn decode_custom_tool_output(raw: &str) -> String {
    let trimmed = raw.trim();
    let Ok(value) = serde_json::from_str::<Value>(trimmed) else {
        return trimmed.to_string();
    };

    value
        .get("output")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| trimmed.to_string())
}

fn format_block(label: &str, text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.contains('\n') {
        format!("[{label}]:\n{trimmed}")
    } else {
        format!("[{label}]: {trimmed}")
    }
}

fn repo_name_from_url(url: &str) -> Option<String> {
    let tail = url.rsplit(&['/', ':'][..]).next()?;
    let trimmed = tail.trim_end_matches(".git");
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn derive_claude_project_name(session_path: &Path) -> String {
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

fn derive_path_session_id(session_path: &Path) -> String {
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
    fn repo_name_handles_ssh_and_https_urls() {
        assert_eq!(
            repo_name_from_url("git@github.com:example/acme-app.git").as_deref(),
            Some("acme-app")
        );
        assert_eq!(
            repo_name_from_url("https://github.com/The-Daily-Claude/the-daily-claude.git")
                .as_deref(),
            Some("the-daily-claude")
        );
    }

    #[test]
    fn format_block_puts_multiline_text_on_following_lines() {
        assert_eq!(format_block("HUMAN", "hello"), "[HUMAN]: hello");
        assert_eq!(
            format_block("ASSISTANT", "hello\nworld"),
            "[ASSISTANT]:\nhello\nworld"
        );
    }

    #[test]
    fn extract_reasoning_summary_joins_text_items() {
        let payload = serde_json::json!({
            "summary": [
                "first",
                {"text": "second"}
            ]
        });
        assert_eq!(extract_reasoning_summary(&payload), "first\nsecond");
    }

    #[test]
    fn decode_custom_tool_output_prefers_inner_output_field() {
        let raw = "{\"output\":\"Success. Updated the following files:\\nM /tmp/file\\n\",\"metadata\":{\"exit_code\":0}}";
        assert_eq!(
            decode_custom_tool_output(raw),
            "Success. Updated the following files:\nM /tmp/file"
        );
    }

    #[test]
    fn decode_custom_tool_output_falls_back_to_raw_text() {
        assert_eq!(decode_custom_tool_output("plain output"), "plain output");
    }

    #[test]
    fn derive_claude_project_name_keeps_existing_heuristic() {
        let p = PathBuf::from(
            "/tmp/claude/projects/-home-alice-Projects-example-the-daily-claude/abc.jsonl",
        );
        assert_eq!(derive_claude_project_name(&p), "daily-claude");
    }

    #[test]
    fn codex_detector_accepts_current_exec_shape() {
        assert!(is_codex_session(Some("codex_exec"), Some("exec")));
    }

    #[test]
    fn codex_detector_accepts_older_tui_shape() {
        assert!(is_codex_session(Some("codex-tui"), Some("cli")));
    }

    #[test]
    fn codex_detector_rejects_non_codex_sessions() {
        assert!(!is_codex_session(Some("other"), Some("exec")));
        assert!(!is_codex_session(Some("codex_exec"), Some("other")));
    }

    #[test]
    fn parse_codex_session_meta_reads_repo_name_and_id() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        std::fs::write(
            &path,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"sess-123\",\"originator\":\"codex-tui\",\"source\":\"cli\",\"cwd\":\"/tmp/acme-app\",\"git\":{\"repository_url\":\"git@github.com:example/acme-app.git\"}}}\n",
        )
        .unwrap();

        let meta = parse_codex_session_meta(&path).unwrap().unwrap();
        assert_eq!(meta.session_id, "sess-123");
        assert_eq!(meta.project_name, "acme-app");
    }

    #[test]
    fn parse_codex_session_meta_reads_current_exec_archive_shape() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        std::fs::write(
            &path,
            "{\"type\":\"session_meta\",\"payload\":{\"id\":\"sess-456\",\"originator\":\"codex_exec\",\"source\":\"exec\",\"cwd\":\"/tmp/current\",\"git\":{\"repository_url\":\"git@github.com:The-Daily-Claude/current.git\"}}}\n",
        )
        .unwrap();

        let meta = parse_codex_session_meta(&path).unwrap().unwrap();
        assert_eq!(meta.session_id, "sess-456");
        assert_eq!(meta.project_name, "current");
    }

    #[test]
    fn prepare_for_extractor_normalises_current_exec_archive_shape() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"sess-456\",\"originator\":\"codex_exec\",\"source\":\"exec\",\"cwd\":\"/tmp/current\",\"git\":{\"repository_url\":\"git@github.com:The-Daily-Claude/current.git\"}}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"Fix it\"}]}}\n"
            ),
        )
        .unwrap();

        let prepared = prepare_for_extractor(&path).unwrap();
        assert_ne!(prepared.extractor_path(), path.as_path());
        assert_eq!(prepared.session_id, "sess-456");
        assert_eq!(prepared.project_name, "current");

        let transcript = std::fs::read_to_string(prepared.extractor_path()).unwrap();
        assert!(transcript.contains("# Normalized Codex CLI transcript"));
        assert!(transcript.contains("[HUMAN]: Fix it"));
    }

    #[test]
    fn prepare_for_extractor_passes_claude_sessions_through() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("abc123.jsonl");
        std::fs::write(
            &path,
            "{\"type\":\"summary\",\"summary\":\"Claude session line\"}\n",
        )
        .unwrap();

        let prepared = prepare_for_extractor(&path).unwrap();
        assert!(prepared._tempfile.is_none());
        assert!(prepared.extractor_path().exists());
        assert_eq!(prepared.session_id, "abc123");
    }

    #[test]
    fn normalise_codex_session_emits_message_and_tool_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"sess-123\",\"originator\":\"codex-tui\",\"source\":\"cli\",\"cwd\":\"/tmp/acme-app\",\"git\":{\"repository_url\":\"git@github.com:example/acme-app.git\"}}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"user\",\"content\":[{\"type\":\"input_text\",\"text\":\"Fix it\"}]}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"type\":\"message\",\"role\":\"assistant\",\"content\":[{\"type\":\"output_text\",\"text\":\"Reading files.\"}]}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"type\":\"function_call\",\"name\":\"exec_command\",\"arguments\":\"{\\\"cmd\\\":\\\"pwd\\\"}\",\"call_id\":\"call-1\"}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"type\":\"function_call_output\",\"call_id\":\"call-1\",\"output\":\"/tmp/acme-app\"}}\n"
            ),
        )
        .unwrap();

        let meta = parse_codex_session_meta(&path).unwrap().unwrap();
        let transcript = normalise_codex_session(&path, &meta).unwrap();
        assert!(transcript.contains("[HUMAN]: Fix it"));
        assert!(transcript.contains("[ASSISTANT]: Reading files."));
        assert!(transcript.contains("[TOOL_INPUT:exec_command]: {\"cmd\":\"pwd\"}"));
        assert!(transcript.contains("[TOOL_RESULT:exec_command]: /tmp/acme-app"));
    }

    #[test]
    fn normalise_codex_session_emits_custom_tool_blocks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"session_meta\",\"payload\":{\"id\":\"sess-789\",\"originator\":\"codex_exec\",\"source\":\"exec\",\"cwd\":\"/tmp/acme-app\",\"git\":{\"repository_url\":\"git@github.com:example/acme-app.git\"}}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"type\":\"custom_tool_call\",\"name\":\"apply_patch\",\"call_id\":\"call-2\",\"input\":\"*** Begin Patch\\n*** End Patch\\n\"}}\n",
                "{\"type\":\"response_item\",\"payload\":{\"type\":\"custom_tool_call_output\",\"call_id\":\"call-2\",\"output\":\"{\\\"output\\\":\\\"Success. Updated the following files:\\\\nM /tmp/acme-app/file.txt\\\\n\\\",\\\"metadata\\\":{\\\"exit_code\\\":0}}\"}}\n"
            ),
        )
        .unwrap();

        let meta = parse_codex_session_meta(&path).unwrap().unwrap();
        let transcript = normalise_codex_session(&path, &meta).unwrap();
        assert!(transcript.contains("[TOOL_INPUT:apply_patch]:\n*** Begin Patch\n*** End Patch"));
        assert!(
            transcript.contains(
                "[TOOL_RESULT:apply_patch]:\nSuccess. Updated the following files:\nM /tmp/acme-app/file.txt"
            )
        );
    }
}
