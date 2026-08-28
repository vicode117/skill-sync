//! Core domain model: skills, metadata, validation, scopes and sources.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Where a skill lives conceptually.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SkillScope {
    /// User-level skills (the MVP focus).
    Global,
    /// Skills inside a project directory.
    Project,
}

/// Where a discovered skill comes from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SkillSource {
    /// Lives in the canonical store — the single source of truth.
    Canonical,
    /// Observed inside a tool's skill directory.
    Observed { tool_id: String },
}

/// YAML frontmatter of `SKILL.md`. Only the fields relevant across tools
/// are extracted; the full raw map is preserved so tool-specific metadata
/// survives round trips untouched (design doc §23).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillFrontmatter {
    pub name: Option<String>,
    pub description: Option<String>,
    /// Complete frontmatter as JSON (camelCase not applied — raw keys).
    pub raw: serde_json::Value,
}

impl SkillFrontmatter {
    /// The effective skill name: frontmatter `name`, falling back to the
    /// directory name.
    pub fn effective_name<'a>(&'a self, dir_name: &'a str) -> &'a str {
        self.name.as_deref().unwrap_or(dir_name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ValidationSeverity {
    Error,
    Warning,
    Note,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidationIssue {
    pub severity: ValidationSeverity,
    pub message: String,
    /// Stable issue code, e.g. `missing_skill_md`, `invalid_frontmatter`.
    pub code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

impl ValidationIssue {
    pub fn error(code: &str, message: impl Into<String>) -> Self {
        Self {
            severity: ValidationSeverity::Error,
            message: message.into(),
            code: code.to_string(),
            file: None,
        }
    }

    pub fn warning(code: &str, message: impl Into<String>) -> Self {
        Self {
            severity: ValidationSeverity::Warning,
            message: message.into(),
            code: code.to_string(),
            file: None,
        }
    }

    pub fn note(code: &str, message: impl Into<String>) -> Self {
        Self {
            severity: ValidationSeverity::Note,
            message: message.into(),
            code: code.to_string(),
            file: None,
        }
    }

    pub fn with_file(mut self, file: impl Into<String>) -> Self {
        self.file = Some(file.into());
        self
    }
}

/// One file (or directory marker) inside a skill, with a slash-normalized
/// relative path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillFileEntry {
    /// Slash-separated path relative to the skill root.
    pub relative_path: String,
    pub is_dir: bool,
    pub is_symlink: bool,
}

/// A discovered skill: a directory anchored by `SKILL.md`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Skill {
    /// Directory name.
    pub id: String,
    /// Frontmatter name or directory name.
    pub display_name: String,
    pub description: Option<String>,
    /// Absolute path of the skill directory.
    pub root: PathBuf,
    pub scope: SkillScope,
    pub source: SkillSource,
    pub files: Vec<SkillFileEntry>,
    /// Deterministic content fingerprint (SHA-256), when computable.
    pub fingerprint: Option<String>,
    pub frontmatter: Option<SkillFrontmatter>,
    pub validation: Vec<ValidationIssue>,
}

impl Skill {
    pub fn has_errors(&self) -> bool {
        self.validation
            .iter()
            .any(|v| v.severity == ValidationSeverity::Error)
    }
}
