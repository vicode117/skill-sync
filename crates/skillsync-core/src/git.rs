//! Git integration for the canonical store (design doc §7g Slice 7,
//! prompt §34–§36).
//!
//! Machine Sync (store ⇄ git remotes) is a separate concept from Tool Sync
//! (store → tools). This module only drives the **system git** binary with
//! explicit argument lists — never a shell, never an embedded git, never
//! anything automatic: pull/commit/push run only when the user asks.
//! Commands run with `-C <canonical root>` and never inherit surprise
//! state; output is captured, not streamed, and nothing from skill file
//! *contents* is ever logged (§49/§50).

use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::env::EnvContext;
use crate::error::{ErrorCode, Result, SkillSyncError};
use crate::skill::ValidationSeverity;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum SkillChange {
    Modified,
    Added,
    Deleted,
    Renamed,
}

/// One skill directory with pending changes (paths grouped by first path
/// segment — the skill directory name).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChangedSkill {
    pub skill_id: String,
    pub change: SkillChange,
    pub files: Vec<String>,
}

/// Repository status for the canonical store.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatus {
    /// The canonical root is inside a git work tree.
    pub is_repo: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    /// True when the branch has an upstream configured.
    pub has_upstream: bool,
    pub changed_skills: Vec<ChangedSkill>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl GitStatus {
    pub fn has_changes(&self) -> bool {
        !self.changed_skills.is_empty()
    }

    /// The most severe change kind, for badges.
    pub fn severity(&self) -> ValidationSeverity {
        if self
            .changed_skills
            .iter()
            .any(|c| matches!(c.change, SkillChange::Deleted))
        {
            ValidationSeverity::Warning
        } else {
            ValidationSeverity::Note
        }
    }
}

/// Whether the system git binary is available.
pub fn git_available(env: &EnvContext) -> bool {
    env.which("git").is_some()
}

fn run_git(env: &EnvContext, root: &Path, args: &[&str]) -> Result<String> {
    let git = env.which("git").ok_or_else(|| {
        SkillSyncError::new(ErrorCode::GitNotFound, "git binary not found on PATH").recoverable()
    })?;
    let output = Command::new(&git)
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .map_err(|e| SkillSyncError::io(&e, root))?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        return Err(SkillSyncError::new(
            ErrorCode::GitNotFound,
            format!(
                "git {} failed: {}",
                args.join(" "),
                combine(&stderr, &stdout)
            ),
        )
        .with_path(root)
        .recoverable());
    }
    Ok(if stdout.trim().is_empty() {
        stderr
    } else {
        stdout
    })
}

fn combine(stderr: &str, stdout: &str) -> String {
    let mut s = stderr.trim().to_string();
    if s.is_empty() {
        s = stdout.trim().to_string();
    }
    s
}

/// Ensure the canonical root exists and is a git work tree.
fn require_repo(env: &EnvContext, canonical_root: &Path) -> Result<()> {
    if !canonical_root.is_dir() {
        return Err(SkillSyncError::new(
            ErrorCode::GitNotFound,
            "canonical skill root does not exist",
        )
        .with_path(canonical_root)
        .recoverable());
    }
    let output = Command::new(env.which("git").ok_or_else(|| {
        SkillSyncError::new(ErrorCode::GitNotFound, "git binary not found on PATH")
    })?)
    .arg("-C")
    .arg(canonical_root)
    .args(["rev-parse", "--is-inside-work-tree"])
    .output()
    .map_err(|e| SkillSyncError::io(&e, canonical_root))?;
    if !output.status.success() {
        return Err(SkillSyncError::new(
            ErrorCode::GitNotFound,
            "canonical skill root is not a git repository",
        )
        .with_path(canonical_root)
        .recoverable());
    }
    Ok(())
}

/// Read-only repository status: branch, ahead/behind, changed skills.
pub fn status(env: &EnvContext, canonical_root: &Path) -> Result<GitStatus> {
    if !canonical_root.is_dir() {
        return Ok(GitStatus {
            is_repo: false,
            branch: None,
            ahead: 0,
            behind: 0,
            has_upstream: false,
            changed_skills: Vec::new(),
            error: Some("canonical skill root does not exist".into()),
        });
    }
    if let Err(err) = require_repo(env, canonical_root) {
        // Non-repo roots are a normal state (§35: git is optional).
        return Ok(GitStatus {
            is_repo: false,
            branch: None,
            ahead: 0,
            behind: 0,
            has_upstream: false,
            changed_skills: Vec::new(),
            error: Some(err.message),
        });
    }
    let raw = run_git(env, canonical_root, &["status", "--porcelain=v1", "-b"])?;

    let mut branch = None;
    let mut ahead = 0;
    let mut behind = 0;
    let mut has_upstream = false;
    let mut changed: Vec<(String, SkillChange, Vec<String>)> = Vec::new();

    for line in raw.lines() {
        if let Some(head) = line.strip_prefix("## ") {
            // e.g. `main...origin/main [ahead 1, behind 2]` or `main`
            let head = head.split('(').next().unwrap_or(head).trim();
            let mut parts = head.splitn(2, "...");
            let name = parts.next().unwrap_or("").trim().to_string();
            if !name.is_empty() && name != "HEAD (no branch)" {
                branch = Some(name);
            }
            if let Some(upstream) = parts.next() {
                has_upstream = true;
                if let Some(idx) = upstream.find("ahead ") {
                    ahead = upstream[idx + 6..]
                        .split(|c: char| !c.is_ascii_digit())
                        .next()
                        .and_then(|d| d.parse().ok())
                        .unwrap_or(0);
                }
                if let Some(idx) = upstream.find("behind ") {
                    behind = upstream[idx + 7..]
                        .split(|c: char| !c.is_ascii_digit())
                        .next()
                        .and_then(|d| d.parse().ok())
                        .unwrap_or(0);
                }
            }
            continue;
        }
        if line.len() < 4 {
            continue;
        }
        let (xy, path) = line.split_at(2);
        let path = path.trim_start().to_string();
        if path.starts_with('"') {
            continue; // quoted exotic paths are skipped (names with spaces)
        }
        let skill_id = match path.split('/').next() {
            Some(seg) if !seg.is_empty() => seg.to_string(),
            _ => continue, // root-level files are not skills
        };
        let change = match (xy.get(0..1), xy.get(1..2)) {
            (Some("?"), _) => SkillChange::Added,
            (Some("D"), _) | (_, Some("D")) => SkillChange::Deleted,
            (Some("R"), _) => SkillChange::Renamed,
            _ => SkillChange::Modified,
        };
        match changed.iter_mut().find(|(id, _, _)| *id == skill_id) {
            Some(entry) => {
                entry.2.push(path);
                // Deletion dominates; then rename; else modified/added.
                if matches!(change, SkillChange::Deleted) {
                    entry.1 = change;
                }
            }
            None => changed.push((skill_id.clone(), change, vec![path])),
        }
    }

    let changed_skills = changed
        .into_iter()
        .map(|(skill_id, change, mut files)| {
            files.sort();
            ChangedSkill {
                skill_id,
                change,
                files,
            }
        })
        .collect();

    Ok(GitStatus {
        is_repo: true,
        branch,
        ahead,
        behind,
        has_upstream,
        changed_skills,
        error: None,
    })
}

/// Pull with fast-forward only: never creates merge commits or overwrites
/// local state silently; conflicts surface as errors for the user.
pub fn pull(env: &EnvContext, canonical_root: &Path) -> Result<String> {
    require_repo(env, canonical_root)?;
    run_git(env, canonical_root, &["pull", "--ff-only"])
}

/// Stage all changes in the store and commit with an explicit message.
pub fn commit(env: &EnvContext, canonical_root: &Path, message: &str) -> Result<String> {
    require_repo(env, canonical_root)?;
    if message.trim().is_empty() {
        return Err(SkillSyncError::new(
            ErrorCode::ConfigInvalid,
            "commit message must not be empty",
        )
        .recoverable());
    }
    run_git(env, canonical_root, &["add", "-A"])?;
    run_git(env, canonical_root, &["commit", "-m", message])
}

/// Push to the configured upstream. Always explicit.
pub fn push(env: &EnvContext, canonical_root: &Path) -> Result<String> {
    require_repo(env, canonical_root)?;
    run_git(env, canonical_root, &["push"])
}

/// Where the skill lives relative to the store (helper for UI labels).
pub fn skill_paths(changed: &[ChangedSkill]) -> Vec<PathBuf> {
    changed.iter().map(|_| PathBuf::new()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Repo {
        tmp: tempfile::TempDir,
        root: PathBuf,
        env: EnvContext,
    }

    fn git(root: &Path, args: &[&str]) {
        let out = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(args)
            .env("GIT_AUTHOR_NAME", "Test")
            .env("GIT_AUTHOR_EMAIL", "test@example.invalid")
            .env("GIT_COMMITTER_NAME", "Test")
            .env("GIT_COMMITTER_EMAIL", "test@example.invalid")
            .output()
            .expect("git should be available for these tests");
        assert!(
            out.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        );
    }

    /// Skip gracefully when no system git is available.
    fn repo() -> Option<Repo> {
        let available = Command::new("git")
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if !available {
            eprintln!("skipping: git not available");
            return None;
        }
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("store");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        let env = EnvContext::with_home(tmp.path().join("home"));
        Some(Repo { tmp, root, env })
    }

    fn write_skill(root: &Path, name: &str, body: &str) {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), body).unwrap();
    }

    #[test]
    fn status_tracks_changed_skills() {
        let Some(repo) = repo() else { return };
        write_skill(&repo.root, "alpha", "---\nname: alpha\n---\nv1\n");
        git(&repo.root, &["add", "-A"]);
        git(&repo.root, &["commit", "-q", "-m", "init"]);

        let clean = status(&repo.env, &repo.root).unwrap();
        assert!(clean.is_repo);
        assert_eq!(clean.branch.as_deref(), Some("main"));
        assert!(!clean.has_changes());

        write_skill(&repo.root, "alpha", "---\nname: alpha\n---\nv2\n");
        write_skill(&repo.root, "beta", "---\nname: beta\n---\nnew\n");
        std::fs::remove_dir_all(repo.root.join("alpha")).ok();

        let st = status(&repo.env, &repo.root).unwrap();
        let ids: Vec<&str> = st
            .changed_skills
            .iter()
            .map(|c| c.skill_id.as_str())
            .collect();
        assert!(ids.contains(&"alpha"), "{:?}", st.changed_skills);
        assert!(ids.contains(&"beta"));
    }

    #[test]
    fn commit_is_explicit_and_reported() {
        let Some(repo) = repo() else { return };
        write_skill(&repo.root, "alpha", "---\nname: alpha\n---\nv1\n");
        commit(&repo.env, &repo.root, "add alpha").unwrap();
        let st = status(&repo.env, &repo.root).unwrap();
        assert!(!st.has_changes());
        assert!(commit(&repo.env, &repo.root, "   ").is_err());
    }

    #[test]
    fn push_pull_round_trip_through_local_remote() {
        let Some(repo) = repo() else { return };
        let remote = repo.tmp.path().join("remote.git");
        std::fs::create_dir_all(&remote).unwrap();
        git(&remote, &["init", "-q", "--bare", "-b", "main"]);

        write_skill(&repo.root, "alpha", "---\nname: alpha\n---\nv1\n");
        git(&repo.root, &["add", "-A"]);
        git(&repo.root, &["commit", "-q", "-m", "init"]);
        git(
            &repo.root,
            &["remote", "add", "origin", remote.to_str().unwrap()],
        );
        git(&repo.root, &["push", "-q", "-u", "origin", "main"]);

        let st = status(&repo.env, &repo.root).unwrap();
        assert!(st.has_upstream);
        assert_eq!(st.ahead, 0);

        let clone = repo.tmp.path().join("clone");
        let out = Command::new("git")
            .args([
                "clone",
                "-q",
                remote.to_str().unwrap(),
                clone.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(out.status.success());
        assert!(clone.join("alpha").join("SKILL.md").is_file());

        write_skill(&repo.root, "beta", "---\nname: beta\n---\nnew\n");
        commit(&repo.env, &repo.root, "add beta").unwrap();
        let st = status(&repo.env, &repo.root).unwrap();
        assert_eq!(st.ahead, 1);
        push(&repo.env, &repo.root).unwrap();
        let _pulled = pull(&repo.env, &clone).unwrap();
        assert!(
            clone.join("beta").join("SKILL.md").is_file(),
            "pull brought beta"
        );
    }

    #[test]
    fn non_repo_roots_are_reported_not_fatal() {
        let tmp = tempfile::tempdir().unwrap();
        let env = EnvContext::with_home(tmp.path().join("home"));
        let root = tmp.path().join("plain");
        std::fs::create_dir_all(&root).unwrap();
        let st = status(&env, &root).unwrap();
        assert!(!st.is_repo);
        assert!(pull(&env, &root).is_err());
        assert!(push(&env, &root).is_err());
        assert!(commit(&env, &root, "msg").is_err());
    }
}
