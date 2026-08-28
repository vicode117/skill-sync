//! Structured error model for the native boundary (see design doc §68).
//!
//! Every error carries a machine-readable `code`; callers (CLI, GUI) must be
//! able to branch on the code instead of parsing message strings.

use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    /// Unexpected filesystem / OS error.
    Io,
    PermissionDenied,
    ConfigInvalid,
    InvalidSkill,
    ToolNotFound,
    ToolDisabled,
    GitNotFound,
    TargetConflict,
    BrokenSymlink,
    UnsafePath,
}

#[derive(Debug, Clone, Serialize)]
pub struct SkillSyncError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill: Option<String>,
    /// Whether the user can plausibly resolve this themselves.
    pub recoverable: bool,
}

impl std::fmt::Display for SkillSyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)?;
        if let Some(path) = &self.path {
            write!(f, " (path: {})", path.display())?;
        }
        Ok(())
    }
}

impl std::error::Error for SkillSyncError {}

impl SkillSyncError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            path: None,
            tool: None,
            skill: None,
            recoverable: false,
        }
    }

    pub fn with_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn with_tool(mut self, tool: impl Into<String>) -> Self {
        self.tool = Some(tool.into());
        self
    }

    pub fn with_skill(mut self, skill: impl Into<String>) -> Self {
        self.skill = Some(skill.into());
        self
    }

    pub fn recoverable(mut self) -> Self {
        self.recoverable = true;
        self
    }

    pub fn io(err: &std::io::Error, path: impl Into<PathBuf>) -> Self {
        let code = match err.kind() {
            std::io::ErrorKind::PermissionDenied => ErrorCode::PermissionDenied,
            _ => ErrorCode::Io,
        };
        Self::new(code, err.to_string()).with_path(path)
    }
}

pub type Result<T> = std::result::Result<T, SkillSyncError>;
