//! Structured operation logging (prompt §50).
//!
//! JSONL under `~/.skillsync/logs/skillsync.log`. Entries record operation,
//! tool, skill, path, status and error codes — never skill file contents
//! and never environment secrets. Logging is best-effort: a logging
//! failure is silently ignored so it can never break a user operation.

use std::path::PathBuf;

use serde::Serialize;

use crate::env::EnvContext;

const MAX_LOG_BYTES: u64 = 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// ISO-8601 UTC timestamp.
    pub ts: String,
    /// e.g. `import`, `sync`, `resolve`, `git-pull`.
    pub operation: String,
    /// `ok` or `error`.
    pub status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_code: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub struct LogEntryBuilder {
    entry: LogEntry,
}

impl LogEntryBuilder {
    pub fn new(operation: &str, status: &'static str) -> Self {
        Self {
            entry: LogEntry {
                ts: crate::store::iso_utc_now(),
                operation: operation.to_string(),
                status,
                tool: None,
                skill: None,
                path: None,
                error_code: None,
                message: None,
            },
        }
    }

    pub fn tool(mut self, tool: impl Into<String>) -> Self {
        self.entry.tool = Some(tool.into());
        self
    }

    pub fn skill(mut self, skill: impl Into<String>) -> Self {
        self.entry.skill = Some(skill.into());
        self
    }

    pub fn path(mut self, path: PathBuf) -> Self {
        self.entry.path = Some(path);
        self
    }

    /// Set the path only when present (convenience for optional targets).
    pub fn path_opt(mut self, path: Option<PathBuf>) -> Self {
        self.entry.path = path;
        self
    }

    pub fn error(mut self, code: &str, message: &str) -> Self {
        self.entry.error_code = Some(code.to_string());
        self.entry.message = Some(redact_message(message));
        self
    }

    pub fn emit(self, env: &EnvContext) {
        write_entry(env, &self.entry);
    }
}

/// Start an `ok` entry.
pub fn ok(operation: &str) -> LogEntryBuilder {
    LogEntryBuilder::new(operation, "ok")
}

/// Start an `error` entry with code + (redacted) message.
pub fn error(operation: &str, code: &str, message: &str) -> LogEntryBuilder {
    LogEntryBuilder::new(operation, "error").error(code, message)
}

/// Defense in depth for §49/§50: strip anything that looks like an
/// inline assignment of a credential from free-form messages.
fn redact_message(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    for line in message.lines() {
        let lower = line.to_lowercase();
        if (lower.contains("password")
            || lower.contains("token")
            || lower.contains("secret")
            || lower.contains("api_key")
            || lower.contains("apikey"))
            && lower.contains('=')
        {
            out.push_str("<redacted line>");
        } else {
            out.push_str(line);
        }
        out.push('\n');
    }
    out.trim_end().to_string()
}

fn write_entry(env: &EnvContext, entry: &LogEntry) {
    let paths = crate::config::AppPaths::discover(env);
    let logs_dir = paths.logs_dir();
    let file = logs_dir.join("skillsync.log");
    // One rotation step; never an enterprise system (§31 spirit).
    if let Ok(meta) = std::fs::metadata(&file) {
        if meta.len() > MAX_LOG_BYTES {
            let rotated = logs_dir.join("skillsync.log.1");
            let _ = std::fs::rename(&file, rotated);
        }
    }
    if let Ok(json) = serde_json::to_string(entry) {
        if std::fs::create_dir_all(&logs_dir).is_ok() {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&file)
            {
                let _ = writeln!(f, "{json}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_jsonl_with_expected_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let env = EnvContext::with_home(tmp.path().join("home"));
        ok("import")
            .tool("claude")
            .skill("tdd")
            .path(PathBuf::from("/tools/tdd"))
            .emit(&env);
        error("sync", "TARGET_CONFLICT", "unmanaged directory differs")
            .tool("claude")
            .skill("legacy")
            .emit(&env);

        let paths = crate::config::AppPaths::discover(&env);
        let raw = std::fs::read_to_string(paths.logs_dir().join("skillsync.log")).unwrap();
        let lines: Vec<&str> = raw.lines().collect();
        assert_eq!(lines.len(), 2);
        let first: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(first["operation"], "import");
        assert_eq!(first["status"], "ok");
        assert_eq!(first["tool"], "claude");
        assert_eq!(first["skill"], "tdd");
        assert!(first["ts"].as_str().is_some());
        let second: serde_json::Value = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(second["errorCode"], "TARGET_CONFLICT");
        assert_eq!(second["status"], "error");
    }

    #[test]
    fn rotates_when_log_exceeds_cap() {
        let tmp = tempfile::tempdir().unwrap();
        let env = EnvContext::with_home(tmp.path().join("home"));
        let paths = crate::config::AppPaths::discover(&env);
        std::fs::create_dir_all(paths.logs_dir()).unwrap();
        let file = paths.logs_dir().join("skillsync.log");
        std::fs::write(&file, vec![b'x'; 1024 * 1024 + 1]).unwrap();

        ok("sync").emit(&env);
        assert!(!file.exists() || std::fs::metadata(&file).unwrap().len() < MAX_LOG_BYTES);
        assert!(paths.logs_dir().join("skillsync.log.1").exists());
    }

    #[test]
    fn redacts_credential_like_lines() {
        let redacted = redact_message("push failed\npassword=hunter2\ndone");
        assert!(!redacted.contains("hunter2"));
        assert!(redacted.contains("<redacted line>"));
        assert!(redacted.contains("push failed"));
        assert!(redacted.contains("done"));
    }
}
