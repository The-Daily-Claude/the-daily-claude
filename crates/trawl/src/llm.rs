use anyhow::{Context, Result, anyhow, bail};
use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\x1b' {
            if i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                let mut j = i + 2;
                while j < bytes.len() && !matches!(bytes[j], b'A'..=b'z') {
                    j += 1;
                }
                if j < bytes.len() {
                    i = j + 1;
                    continue;
                }
            }
            i += 1;
            continue;
        }
        result.push(bytes[i] as char);
        i += 1;
    }
    result
}

fn classify_failure(raw_output: &str) -> Option<String> {
    let stripped = strip_ansi(raw_output);
    let combined = raw_output.to_string() + "\n" + &stripped;
    let lower = combined.to_lowercase();

    if lower.contains("you've hit your limit")
        || lower.contains("rate limit")
        || lower.contains("quota")
    {
        return Some(
            "Claude API quota exceeded or rate limit hit. Check the CLI output for reset timing."
                .to_string(),
        );
    }
    if lower.contains("not authenticated")
        || lower.contains("please run `claude auth")
        || lower.contains("claude auth")
    {
        return Some("Claude is not authenticated. Run `claude auth` to log in.".to_string());
    }
    if lower.contains("invalid api key") || lower.contains("api key invalid") {
        return Some(
            "Claude API key is invalid. Check your ANTHROPIC_API_KEY environment variable."
                .to_string(),
        );
    }
    if lower.contains("unsupported flag") || lower.contains("unknown flag") {
        return Some(
            "Claude CLI received an unsupported flag. Ensure your Claude CLI version is up to date."
                .to_string(),
        );
    }
    if lower.contains("permission denied") || lower.contains("permission_error") {
        return Some(
            "Claude CLI permission error. Check file/directory access permissions.".to_string(),
        );
    }
    if lower.contains("econnrefused") || lower.contains("connection refused") {
        return Some(
            "Could not connect to Claude API. Check your network connection and API endpoint."
                .to_string(),
        );
    }
    None
}

fn nonzero_exit_error(
    program: &str,
    code: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
) -> anyhow::Error {
    let combined = format!(
        "{}\n{}",
        String::from_utf8_lossy(stderr),
        String::from_utf8_lossy(stdout)
    );
    if let Some(actionable) = classify_failure(&combined) {
        return anyhow!("{program} exited with status {code:?}: {actionable}");
    }

    anyhow!(
        "{program} exited with status {code:?}; no safe diagnostic matched (stdout {} bytes, stderr {} bytes)",
        stdout.len(),
        stderr.len()
    )
}

/// One stage's LLM configuration: runner/provider, model identifier,
/// and optional normalized reasoning effort.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageConfig {
    provider: Provider,
    model: String,
    effort: Option<Effort>,
}

impl StageConfig {
    /// Parse a user-supplied model string.
    ///
    /// Accepted forms:
    /// - `claude-code/claude-opus-4-6`
    /// - `codex/gpt-5.4-codex`
    /// - `gemini/gemini-3.1-pro-preview`
    /// - `opencode/kimi-for-coding/k2p5`
    /// - `pi/zai-coding-plan/glm-5.1`
    ///
    /// Backward compatibility: a bare model name is treated as a
    /// Claude Code model (`sonnet` -> `claude-code/sonnet`).
    pub fn parse(model: &str, effort: Option<&str>) -> Result<Self> {
        let trimmed = model.trim();
        if trimmed.is_empty() {
            bail!("model must not be empty");
        }

        let (provider, backend_model) = match trimmed.split_once('/') {
            Some((raw_provider, rest)) => {
                let provider = Provider::parse(raw_provider).ok_or_else(|| {
                    anyhow!(
                        "unsupported model provider `{raw_provider}`; use one of: claude-code, codex, gemini, opencode, pi"
                    )
                })?;
                let backend_model = rest.trim();
                if backend_model.is_empty() {
                    bail!(
                        "model string `{trimmed}` is missing the model portion after `{raw_provider}/`"
                    );
                }
                (provider, backend_model.to_string())
            }
            None => (Provider::ClaudeCode, trimmed.to_string()),
        };

        let effort = effort.map(Effort::parse).transpose()?;
        if effort.is_some() && !provider.supports_effort() {
            bail!(
                "provider `{}` does not support --effort; omit it for this backend",
                provider.as_str()
            );
        }
        if provider == Provider::Pi && !backend_model.contains('/') {
            bail!(
                "pi models must keep the upstream provider/model path after the `pi/` prefix, e.g. `pi/zai-coding-plan/glm-5.1`"
            );
        }

        Ok(Self {
            provider,
            model: backend_model,
            effort,
        })
    }

    pub fn signature(&self) -> String {
        match self.effort {
            Some(effort) => format!(
                "{}/{}#{}",
                self.provider.as_str(),
                self.model,
                effort.as_str()
            ),
            None => format!("{}/{}", self.provider.as_str(), self.model),
        }
    }

    pub fn preflight(&self) -> Result<()> {
        let status = Command::new("which")
            .arg(self.provider.binary_name())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .with_context(|| format!("run `which {}`", self.provider.binary_name()))?;

        if !status.success() {
            bail!(
                "provider `{}` requires the `{}` CLI on PATH",
                self.provider.as_str(),
                self.provider.binary_name()
            );
        }

        Ok(())
    }

    pub fn run_extractor(&self, prompt: &str, session_dir: &Path) -> Result<String> {
        self.build_invocation(prompt, Some(session_dir), true)?
            .run()
    }

    pub fn run_tokeniser(&self, prompt: &str) -> Result<String> {
        self.build_invocation(prompt, None, false)?.run()
    }

    fn build_invocation(
        &self,
        prompt: &str,
        session_dir: Option<&Path>,
        allow_read_tool: bool,
    ) -> Result<Invocation> {
        match self.provider {
            Provider::ClaudeCode => {
                let mut args = vec!["-p".to_string(), "--model".to_string(), self.model.clone()];
                if let Some(effort) = self.effort {
                    args.push("--effort".to_string());
                    args.push(effort.claude_arg().to_string());
                }
                if let Some(dir) = session_dir {
                    args.push("--add-dir".to_string());
                    args.push(dir.display().to_string());
                }
                if allow_read_tool {
                    args.push("--allowedTools=Read".to_string());
                }

                Ok(Invocation {
                    program: "claude".to_string(),
                    args,
                    current_dir: None,
                    prompt: PromptTransport::Stdin(prompt.to_string()),
                    output: OutputDecoder::Plain,
                })
            }
            Provider::Codex => {
                let mut args = vec![
                    "exec".to_string(),
                    "-".to_string(),
                    "--model".to_string(),
                    self.model.clone(),
                    "--sandbox".to_string(),
                    "read-only".to_string(),
                    "--skip-git-repo-check".to_string(),
                ];
                if let Some(effort) = self.effort {
                    args.push("-c".to_string());
                    args.push(format!("model_reasoning_effort=\"{}\"", effort.codex_arg()));
                }
                if let Some(dir) = session_dir {
                    args.push("-C".to_string());
                    args.push(dir.display().to_string());
                }

                Ok(Invocation {
                    program: "codex".to_string(),
                    args,
                    current_dir: None,
                    prompt: PromptTransport::Stdin(prompt.to_string()),
                    output: OutputDecoder::Plain,
                })
            }
            Provider::Gemini => {
                let mut args = vec![
                    "-p".to_string(),
                    prompt.to_string(),
                    "--model".to_string(),
                    self.model.clone(),
                    "--sandbox".to_string(),
                    "-o".to_string(),
                    "text".to_string(),
                ];
                if let Some(dir) = session_dir {
                    args.push("--include-directories".to_string());
                    args.push(dir.display().to_string());
                }

                Ok(Invocation {
                    program: "gemini".to_string(),
                    args,
                    current_dir: None,
                    prompt: PromptTransport::None,
                    output: OutputDecoder::Plain,
                })
            }
            Provider::OpenCode => {
                let mut args = vec![
                    "run".to_string(),
                    "--model".to_string(),
                    self.model.clone(),
                    "--format".to_string(),
                    "json".to_string(),
                ];
                if let Some(effort) = self.effort {
                    args.push("--variant".to_string());
                    args.push(effort.opencode_arg().to_string());
                }
                if let Some(dir) = session_dir {
                    args.push("--dir".to_string());
                    args.push(dir.display().to_string());
                }
                args.push(prompt.to_string());

                Ok(Invocation {
                    program: "opencode".to_string(),
                    args,
                    current_dir: None,
                    prompt: PromptTransport::None,
                    output: OutputDecoder::OpenCodeJson,
                })
            }
            Provider::Pi => {
                let (upstream_provider, upstream_model) = self.model.split_once('/').ok_or_else(|| {
                    anyhow!(
                        "pi model must be in provider/model form after the `pi/` prefix, got `{}`",
                        self.model
                    )
                })?;
                let args = vec![
                    "--provider".to_string(),
                    upstream_provider.to_string(),
                    "--model".to_string(),
                    upstream_model.to_string(),
                    "-p".to_string(),
                    prompt.to_string(),
                ];

                Ok(Invocation {
                    program: "pi".to_string(),
                    args,
                    current_dir: session_dir.map(Path::to_path_buf),
                    prompt: PromptTransport::None,
                    output: OutputDecoder::Plain,
                })
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Provider {
    ClaudeCode,
    Codex,
    Gemini,
    OpenCode,
    Pi,
}

impl Provider {
    fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "claude" | "claude-code" => Some(Self::ClaudeCode),
            "codex" => Some(Self::Codex),
            "gemini" => Some(Self::Gemini),
            "opencode" => Some(Self::OpenCode),
            "pi" => Some(Self::Pi),
            _ => None,
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude-code",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::OpenCode => "opencode",
            Self::Pi => "pi",
        }
    }

    fn binary_name(self) -> &'static str {
        match self {
            Self::ClaudeCode => "claude",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::OpenCode => "opencode",
            Self::Pi => "pi",
        }
    }

    fn supports_effort(self) -> bool {
        matches!(self, Self::ClaudeCode | Self::Codex | Self::OpenCode)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Effort {
    Min,
    Medium,
    High,
}

impl Effort {
    fn parse(raw: &str) -> Result<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "min" | "minimal" | "low" => Ok(Self::Min),
            "medium" | "med" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            other => bail!("unsupported effort `{other}`; use one of: min, medium, high"),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Min => "min",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    fn claude_arg(self) -> &'static str {
        match self {
            Self::Min => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    fn codex_arg(self) -> &'static str {
        match self {
            Self::Min => "minimal",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    fn opencode_arg(self) -> &'static str {
        match self {
            Self::Min => "minimal",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

#[derive(Debug, Clone)]
struct Invocation {
    program: String,
    args: Vec<String>,
    current_dir: Option<PathBuf>,
    prompt: PromptTransport,
    output: OutputDecoder,
}

impl Invocation {
    fn run(self) -> Result<String> {
        let mut command = Command::new(&self.program);
        command.args(&self.args);
        if let Some(dir) = &self.current_dir {
            command.current_dir(dir);
        }
        match self.prompt {
            PromptTransport::None => {}
            PromptTransport::Stdin(_) => {
                command.stdin(Stdio::piped());
            }
        }
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to spawn `{}`", self.program))?;

        if let PromptTransport::Stdin(prompt) = self.prompt {
            let stdin = child
                .stdin
                .as_mut()
                .ok_or_else(|| anyhow!("failed to open stdin to `{}`", self.program))?;
            stdin
                .write_all(prompt.as_bytes())
                .with_context(|| format!("failed to write prompt to `{}` stdin", self.program))?;
        }

        let output = child
            .wait_with_output()
            .with_context(|| format!("waiting on `{}`", self.program))?;

        if !output.status.success() {
            return Err(nonzero_exit_error(
                &self.program,
                output.status.code(),
                &output.stdout,
                &output.stderr,
            ));
        }

        let stdout = String::from_utf8(output.stdout)
            .with_context(|| format!("{} stdout was not valid UTF-8", self.program))?;
        self.output.decode(stdout)
    }
}

#[derive(Debug, Clone)]
enum PromptTransport {
    None,
    Stdin(String),
}

#[derive(Debug, Clone, Copy)]
enum OutputDecoder {
    Plain,
    OpenCodeJson,
}

impl OutputDecoder {
    fn decode(self, raw: String) -> Result<String> {
        match self {
            Self::Plain => Ok(raw),
            Self::OpenCodeJson => decode_opencode_output(&raw),
        }
    }
}

fn decode_opencode_output(raw: &str) -> Result<String> {
    let mut chunks = Vec::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: Value =
            serde_json::from_str(line).with_context(|| "parse OpenCode JSON event stream")?;
        if value.get("type").and_then(Value::as_str) == Some("text")
            && let Some(text) = value.get("text").and_then(Value::as_str)
        {
            chunks.push(text.to_string());
        }
    }

    if !chunks.is_empty() {
        return Ok(chunks.join("\n"));
    }

    Ok(raw.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_models_default_to_claude_code() {
        let parsed = StageConfig::parse("sonnet", None).unwrap();
        assert_eq!(parsed.signature(), "claude-code/sonnet");
    }

    #[test]
    fn provider_model_keeps_nested_model_path() {
        let parsed = StageConfig::parse("opencode/kimi-for-coding/k2p5", None).unwrap();
        assert_eq!(parsed.signature(), "opencode/kimi-for-coding/k2p5");
    }

    #[test]
    fn claude_alias_normalizes_to_claude_code() {
        let parsed = StageConfig::parse("claude/claude-opus-4-6", Some("high")).unwrap();
        assert_eq!(parsed.signature(), "claude-code/claude-opus-4-6#high");
    }

    #[test]
    fn gemini_rejects_effort() {
        let err = StageConfig::parse("gemini/gemini-3.1-pro-preview", Some("high")).unwrap_err();
        assert!(err.to_string().contains("does not support --effort"));
    }

    #[test]
    fn pi_requires_nested_provider_model() {
        let err = StageConfig::parse("pi/glm-5.1", None).unwrap_err();
        assert!(err.to_string().contains("provider/model"));
    }

    #[test]
    fn codex_invocation_uses_reasoning_config() {
        let parsed = StageConfig::parse("codex/gpt-5.4-codex", Some("min")).unwrap();
        let invocation = parsed
            .build_invocation("hello", Some(Path::new("/tmp/session")), true)
            .unwrap();
        assert_eq!(invocation.program, "codex");
        assert!(invocation.args.contains(&"exec".to_string()));
        assert!(invocation.args.contains(&"gpt-5.4-codex".to_string()));
        assert!(
            invocation
                .args
                .contains(&"model_reasoning_effort=\"minimal\"".to_string())
        );
    }

    #[test]
    fn claude_invocation_uses_effort_add_dir_and_read_tool() {
        let parsed = StageConfig::parse("claude-code/claude-opus-4-6", Some("medium")).unwrap();
        let invocation = parsed
            .build_invocation("hello", Some(Path::new("/tmp/session")), true)
            .unwrap();
        assert_eq!(invocation.program, "claude");
        assert!(invocation.args.contains(&"--effort".to_string()));
        assert!(invocation.args.contains(&"medium".to_string()));
        assert!(invocation.args.contains(&"--allowedTools=Read".to_string()));
        assert!(invocation.args.contains(&"/tmp/session".to_string()));
    }

    #[test]
    fn opencode_invocation_passes_variant_and_nested_model() {
        let parsed =
            StageConfig::parse("opencode/minimax-coding-plan/MiniMax-M2.7", Some("high")).unwrap();
        let invocation = parsed.build_invocation("hello", None, false).unwrap();
        assert_eq!(invocation.program, "opencode");
        assert!(invocation.args.contains(&"--variant".to_string()));
        assert!(invocation.args.contains(&"high".to_string()));
        assert!(
            invocation
                .args
                .contains(&"minimax-coding-plan/MiniMax-M2.7".to_string())
        );
    }

    #[test]
    fn strip_ansi_removes_escape_sequences() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
        assert_eq!(strip_ansi("\x1b[1;32mgreen\x1b[0m"), "green");
        assert_eq!(strip_ansi("plain text"), "plain text");
        assert_eq!(strip_ansi(""), "");
        assert_eq!(strip_ansi("\x1b[38;5;208mOrange\x1b[0m"), "Orange");
    }

    #[test]
    fn classify_failure_detects_quota_limit() {
        let msg = "You've hit your limit · resets 8am (UTC)";
        let got = classify_failure(msg);
        assert!(got.is_some());
        let msg = got.unwrap();
        assert!(msg.contains("quota") || msg.contains("rate limit"));
    }

    #[test]
    fn classify_failure_detects_quota_ansi_escaped() {
        let msg = "\x1b[31m\x1b[1mYou've hit your limit\x1b[0m · resets 8am (UTC)";
        let got = classify_failure(msg);
        assert!(got.is_some());
    }

    #[test]
    fn classify_failure_detects_auth_not_authenticated() {
        let msg = "Not authenticated. Please run `claude auth` to log in.";
        let got = classify_failure(msg);
        assert!(got.is_some());
        let msg = got.unwrap();
        assert!(msg.contains("authenticate") || msg.contains("claude auth"));
    }

    #[test]
    fn classify_failure_detects_auth_please_run_claude_auth() {
        let msg = "Please run `claude auth` to authenticate.";
        let got = classify_failure(msg);
        assert!(got.is_some());
    }

    #[test]
    fn classify_failure_detects_invalid_api_key() {
        let msg = "Error: invalid api key";
        let got = classify_failure(msg);
        assert!(got.is_some());
        assert!(got.unwrap().contains("API key"));
    }

    #[test]
    fn classify_failure_detects_unsupported_flag() {
        let msg = "error: unsupported flag '--foo'";
        let got = classify_failure(msg);
        assert!(got.is_some());
        assert!(got.unwrap().contains("unsupported flag"));
    }

    #[test]
    fn classify_failure_detects_permission_denied() {
        let msg = "Permission denied accessing /some/path";
        let got = classify_failure(msg);
        assert!(got.is_some());
        assert!(got.unwrap().contains("permission"));
    }

    #[test]
    fn classify_failure_detects_connection_refused() {
        let msg = "Error: ECONNREFUSED";
        let got = classify_failure(msg);
        assert!(got.is_some());
        let msg = got.unwrap();
        assert!(msg.contains("connect") || msg.contains("network"));
    }

    #[test]
    fn classify_failure_returns_none_for_session_like_content() {
        let content = r#"The user said "hello world" and then asked about dp.sa.SECRETTOKEN"#;
        let got = classify_failure(content);
        assert!(
            got.is_none(),
            "session plaintext should not be classified as actionable: {got:?}"
        );
    }

    #[test]
    fn classify_failure_returns_none_for_mixed_content_without_known_patterns() {
        let content = r#"Some error happened but nothing we recognize about limits or login"#;
        let got = classify_failure(content);
        assert!(got.is_none());
    }

    #[test]
    fn nonzero_exit_error_surfaces_known_stdout_failure() {
        let err = nonzero_exit_error(
            "claude",
            Some(1),
            b"You've hit your limit \xc2\xb7 resets 8am (UTC)",
            b"",
        );
        let msg = err.to_string();
        assert!(msg.contains("quota") || msg.contains("rate limit"));
        assert!(!msg.contains("resets 8am"));
    }

    #[test]
    fn nonzero_exit_error_does_not_dump_unknown_output() {
        let stdout = b"session plaintext dp.sa.SECRETTOKEN";
        let stderr = b"more sensitive stderr";
        let err = nonzero_exit_error("claude", Some(1), stdout, stderr);
        let msg = err.to_string();
        assert!(msg.contains(&format!("stdout {} bytes", stdout.len())));
        assert!(msg.contains(&format!("stderr {} bytes", stderr.len())));
        assert!(!msg.contains("SECRETTOKEN"));
        assert!(!msg.contains("sensitive stderr"));
    }
}
