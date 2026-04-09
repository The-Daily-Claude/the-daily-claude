use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// Serializable view of Entry frontmatter for YAML output.
/// Keeps field ordering explicit and prevents YAML injection
/// by delegating escaping/quoting to serde_yaml.
#[derive(Serialize)]
struct Frontmatter {
    id: String,
    title: String,
    project: String,
    category: String,
    source_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_range: Option<Vec<u32>>,
    extracted_at: String,
    needs_manual_review: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    review_reason: Option<String>,
    tags: Vec<String>,
    /// Sidecar entity graph emitted by the Haiku tokeniser.
    /// Maps `#TYPE_NNN#` placeholder → metadata (type, optional relations).
    /// Never contains real PII values.
    entities: Value,
}

/// A remarkable moment extracted from a Claude Code session.
///
/// Bodies always carry placeholder tokens (`#USER_001#`, `#CITY_001#`, …)
/// from the Haiku tokeniser pass — never raw PII. The `entities` graph
/// describes the relationships placeholders carry so downstream
/// consumers of this output can substitute coherently per render.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub id: Uuid,
    pub title: String,
    pub project: String,
    pub category: String,
    pub tags: Vec<String>,
    pub source: Source,
    pub needs_manual_review: bool,
    pub review_reason: Option<String>,
    pub entities: Value,
    pub body: String,
}

/// Where this moment came from — for traceability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    /// "session" for trawl-extracted, "compilation" for legacy migration
    pub source_type: String,
    /// Session ID (UUID from the JSONL filename) if from a session
    pub session_id: Option<String>,
    /// Project directory name (e.g., "[project-beta]", "kumbaya")
    pub project_path: Option<String>,
    /// Approximate message range in the session
    pub message_range: Option<(u32, u32)>,
    /// When this moment was extracted
    pub extracted_at: DateTime<Utc>,
}

impl Entry {
    /// Serialize to a markdown file with YAML frontmatter.
    pub fn to_markdown(&self) -> String {
        let fm = Frontmatter {
            id: self.id.to_string(),
            title: self.title.clone(),
            project: self.project.clone(),
            category: self.category.clone(),
            source_type: self.source.source_type.clone(),
            session_id: self.source.session_id.clone(),
            message_range: self.source.message_range.map(|(s, e)| vec![s, e]),
            extracted_at: self
                .source
                .extracted_at
                .format("%Y-%m-%dT%H:%M:%SZ")
                .to_string(),
            needs_manual_review: self.needs_manual_review,
            review_reason: self.review_reason.clone(),
            tags: self.tags.clone(),
            entities: self.entities.clone(),
        };

        let yaml = serde_yaml::to_string(&fm).expect("Failed to serialize frontmatter");
        format!("---\n{yaml}---\n\n{}\n", self.body)
    }

    /// Generate a filename slug from the title.
    pub fn filename(&self, number: u32) -> String {
        let slug: String = self
            .title
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>()
            .split('-')
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("-");

        let truncated = if slug.len() > 60 {
            // Find the last char whose end byte is within 60 bytes.
            let end = slug
                .char_indices()
                .take_while(|(i, c)| i + c.len_utf8() <= 60)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(slug.len());
            slug[..end].trim_end_matches('-').to_string()
        } else {
            slug
        };

        format!("{number:03}-{truncated}.md")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn fixture() -> Entry {
        Entry {
            id: Uuid::nil(),
            title: "Per Your Own Rule".to_string(),
            project: "[project-beta]".to_string(),
            category: "self-contradiction".to_string(),
            tags: vec!["meta".to_string(), "rule-reinterpretation".to_string()],
            source: Source {
                source_type: "session".to_string(),
                session_id: Some("abc-123".to_string()),
                project_path: None,
                message_range: Some((42, 51)),
                extracted_at: DateTime::parse_from_rfc3339("2026-04-07T22:30:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            },
            needs_manual_review: false,
            review_reason: None,
            entities: json!({
                "#USER_001#": {"type": "USER"},
            }),
            body: "[ASSISTANT]: PR first, deploy after merge.".to_string(),
        }
    }

    #[test]
    fn filename_uses_number_and_slug() {
        let e = fixture();
        assert_eq!(e.filename(312), "312-per-your-own-rule.md");
    }

    #[test]
    fn to_markdown_has_yaml_frontmatter_and_body() {
        let md = fixture().to_markdown();
        assert!(md.starts_with("---\n"));
        assert!(md.contains("title: Per Your Own Rule"));
        assert!(md.contains("\n[ASSISTANT]:"));
        // Entities are present in frontmatter as JSON-in-YAML.
        assert!(md.contains("USER_001"));
    }
}
