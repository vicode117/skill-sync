//! Local application configuration (`~/.skillsync/config.json`).
//!
//! The canonical skill store is *files on disk*; this file only holds
//! SkillSync's own settings (canonical root, sync method, per-tool path
//! overrides, future repository metadata). Mutable skill content never
//! lives here.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::env::{expand_home, EnvContext};
use crate::error::{ErrorCode, Result, SkillSyncError};

/// How installations are materialized in tool directories. `Auto` decides
/// per platform/tool: prefer symlinks, fall back to copies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncMethod {
    #[default]
    Auto,
    Symlink,
    Copy,
}

/// Per-tool settings. Every path is an *override* on top of the adapter's
/// auto-detected default (design doc §46).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ToolOverride {
    /// Enable/disable the whole integration for this tool.
    pub enabled: Option<bool>,
    /// Manual override for the global (user-level) skills directory.
    pub global_skill_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepositoryConfig {
    /// Local path of a git repository used as canonical skill store.
    pub path: String,
    #[serde(default)]
    pub auto_sync: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Config {
    /// Canonical skill root. May start with `~`.
    pub canonical_skill_root: String,
    pub sync_method: SyncMethod,
    pub tools: BTreeMap<String, ToolOverride>,
    pub repositories: Vec<RepositoryConfig>,
    /// Optional automatic synchronization toggle (Slice 6). Off by default.
    pub auto_sync: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            canonical_skill_root: "~/.agents/skills".to_string(),
            sync_method: SyncMethod::Auto,
            tools: BTreeMap::new(),
            repositories: Vec::new(),
            auto_sync: false,
        }
    }
}

impl Config {
    /// The canonical skill root, with `~` expanded.
    pub fn canonical_root(&self, env: &EnvContext) -> PathBuf {
        expand_home(&self.canonical_skill_root, env)
    }

    pub fn tool(&self, tool_id: &str) -> Option<&ToolOverride> {
        self.tools.get(tool_id)
    }

    pub fn is_tool_enabled(&self, tool_id: &str) -> bool {
        self.tool(tool_id).and_then(|t| t.enabled).unwrap_or(true)
    }
}

/// SkillSync's own home directory: `SKILLSYNC_HOME` or `~/.skillsync`.
/// Config, logs and backups live here — never inside skill folders.
#[derive(Debug, Clone)]
pub struct AppPaths {
    pub home: PathBuf,
}

impl AppPaths {
    pub fn discover(env: &EnvContext) -> Self {
        let home = env
            .var("SKILLSYNC_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| env.home.join(".skillsync"));
        Self { home }
    }

    pub fn config_file(&self) -> PathBuf {
        self.home.join("config.json")
    }

    pub fn backups_dir(&self) -> PathBuf {
        self.home.join("backups")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.home.join("logs")
    }
}

/// Load the config, returning defaults when the file does not exist yet.
pub fn load_config(paths: &AppPaths) -> Result<Config> {
    let file = paths.config_file();
    match std::fs::read(&file) {
        Ok(bytes) => serde_json::from_slice(&bytes).map_err(|e| {
            SkillSyncError::new(
                ErrorCode::ConfigInvalid,
                format!("invalid config.json: {e}"),
            )
            .with_path(&file)
            .recoverable()
        }),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Config::default()),
        Err(err) => Err(SkillSyncError::io(&err, &file)),
    }
}

/// Save the config atomically (temp file + fsync + rename). Creates the
/// SkillSync home directory if needed — this is SkillSync-owned state, not
/// user skill content.
pub fn save_config(paths: &AppPaths, config: &Config) -> Result<()> {
    let file = paths.config_file();
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).map_err(|e| SkillSyncError::io(&e, parent))?;
    }
    crate::fsutil::atomic_write(
        &file,
        serde_json::to_vec_pretty(config)
            .map_err(|e| SkillSyncError::new(ErrorCode::ConfigInvalid, e.to_string()))?
            .as_slice(),
    )
}

/// Ensure the parent directory of `path` exists (used by writers).
pub fn ensure_parent_dir(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| SkillSyncError::io(&e, parent))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_config_returns_default() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            home: tmp.path().to_path_buf(),
        };
        let config = load_config(&paths).unwrap();
        assert_eq!(config.canonical_skill_root, "~/.agents/skills");
        assert_eq!(config.sync_method, SyncMethod::Auto);
    }

    #[test]
    fn round_trips_camel_case_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            home: tmp.path().to_path_buf(),
        };
        let mut config = Config {
            canonical_skill_root: "~/Developer/agent-skills".into(),
            ..Default::default()
        };
        config.tools.insert(
            "claude".into(),
            ToolOverride {
                enabled: Some(false),
                global_skill_path: Some("~/.claude2/skills".into()),
            },
        );
        save_config(&paths, &config).unwrap();

        let raw = std::fs::read_to_string(paths.config_file()).unwrap();
        assert!(raw.contains("canonicalSkillRoot"));
        let loaded = load_config(&paths).unwrap();
        assert_eq!(loaded, config);
        assert!(!loaded.is_tool_enabled("claude"));
    }

    #[test]
    fn invalid_json_is_structured_error() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = AppPaths {
            home: tmp.path().to_path_buf(),
        };
        std::fs::create_dir_all(paths.home.clone()).unwrap();
        std::fs::write(paths.config_file(), b"{ not json").unwrap();
        let err = load_config(&paths).unwrap_err();
        assert_eq!(err.code, ErrorCode::ConfigInvalid);
        assert!(err.recoverable);
    }
}
