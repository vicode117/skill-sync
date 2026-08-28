//! Execution environment: home directory, OS, and tool-relevant env vars.
//!
//! Adapters must take all environmental facts from `EnvContext` so behavior
//! is testable with synthetic homes/environments.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Macos,
    Linux,
    Windows,
    Other,
}

impl std::fmt::Display for Os {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Os::Macos => "macOS",
            Os::Linux => "Linux",
            Os::Windows => "Windows",
            Os::Other => "other",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone)]
pub struct EnvContext {
    pub home: PathBuf,
    pub os: Os,
    /// Environment variable overrides (e.g. `CODEX_HOME`). Values here win
    /// over the process environment; explicit config always wins over both.
    pub env: BTreeMap<String, String>,
}

impl EnvContext {
    /// Discover the real environment. Returns an error only when no home
    /// directory can be resolved at all.
    pub fn discover() -> crate::error::Result<Self> {
        let home = dirs::home_dir().ok_or_else(|| {
            crate::error::SkillSyncError::new(
                crate::error::ErrorCode::Io,
                "could not resolve the user home directory",
            )
        })?;
        Ok(Self::with_home(home))
    }

    pub fn with_home(home: impl Into<PathBuf>) -> Self {
        let home = home.into();
        let os = if cfg!(target_os = "macos") {
            Os::Macos
        } else if cfg!(target_os = "windows") {
            Os::Windows
        } else if cfg!(target_os = "linux") {
            Os::Linux
        } else {
            Os::Other
        };
        Self {
            home,
            os,
            env: BTreeMap::new(),
        }
    }

    /// Look up an environment variable: injected overrides first, then the
    /// process environment.
    pub fn var(&self, key: &str) -> Option<String> {
        if let Some(v) = self.env.get(key) {
            return Some(v.clone());
        }
        std::env::var(key).ok()
    }

    pub fn home_relative(&self, segments: &[&str]) -> PathBuf {
        let mut p = self.home.clone();
        for s in segments {
            p = p.join(s);
        }
        p
    }

    /// Search `PATH` (from injected env or process env) for an executable.
    pub fn which(&self, name: &str) -> Option<PathBuf> {
        let path_var = self.var("PATH")?;
        let ext = if self.os == Os::Windows { ".exe" } else { "" };
        for dir in std::env::split_paths(&path_var) {
            let candidate = dir.join(format!("{name}{ext}"));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
        None
    }
}

/// Expand a leading `~` (and `~user` is not supported) using the env home.
pub fn expand_home(path: &str, env: &EnvContext) -> PathBuf {
    let trimmed = path.trim();
    if trimmed == "~" {
        return env.home.clone();
    }
    if let Some(rest) = trimmed.strip_prefix("~/").or(trimmed.strip_prefix("~\\")) {
        return env.home.join(rest);
    }
    PathBuf::from(trimmed)
}

/// Lexically normalize a path for display: keep `~` abbreviation when the
/// path is inside the home directory. Never resolves symlinks.
pub fn abbreviate_home(path: &Path, env: &EnvContext) -> String {
    match path.strip_prefix(&env.home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_string(),
        Ok(rest) => format!("~/{}", rest.to_string_lossy()),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_home_handles_tilde() {
        let env = EnvContext::with_home("/tmp/home");
        assert_eq!(expand_home("~", &env), PathBuf::from("/tmp/home"));
        assert_eq!(
            expand_home("~/.agents/skills", &env),
            PathBuf::from("/tmp/home/.agents/skills")
        );
        assert_eq!(
            expand_home("/absolute/path", &env),
            PathBuf::from("/absolute/path")
        );
    }
}
