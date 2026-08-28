//! Doctor diagnostics (design doc §42): environment, canonical root,
//! tools, symlink capability, git availability, broken symlinks,
//! duplicates, permissions. Read-only except for a throwaway symlink test
//! inside a temp directory.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::adapter::ToolAdapter;
use crate::config::{AppPaths, Config};
use crate::env::EnvContext;
use crate::scan::{scan_skills_root, Managedness};
use crate::skill::{SkillScope, ValidationSeverity};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    Ok,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorCheck {
    pub id: String,
    pub title: String,
    pub status: CheckStatus,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DoctorReport {
    pub os: String,
    pub skillsync_home: String,
    pub checks: Vec<DoctorCheck>,
}

impl DoctorReport {
    pub fn has_errors(&self) -> bool {
        self.checks.iter().any(|c| c.status == CheckStatus::Error)
    }

    pub fn has_warnings(&self) -> bool {
        self.checks.iter().any(|c| c.status == CheckStatus::Warning)
    }
}

fn check(id: &str, title: &str, status: CheckStatus, detail: impl Into<String>) -> DoctorCheck {
    DoctorCheck {
        id: id.to_string(),
        title: title.to_string(),
        status,
        detail: detail.into(),
    }
}

/// Run all diagnostic checks. `env`, `paths` and `config` describe the
/// environment under inspection (tests use synthetic homes).
pub fn run_doctor(
    env: &EnvContext,
    paths: &AppPaths,
    config: &Config,
    adapters: &[std::sync::Arc<dyn ToolAdapter>],
) -> DoctorReport {
    let mut checks = Vec::new();

    checks.push(check(
        "os",
        "Operating system",
        CheckStatus::Ok,
        format!("{}, home {}", env.os, env.home.display()),
    ));

    // SkillSync home
    checks.push(check(
        "skillsync-home",
        "SkillSync home",
        if paths.home.is_dir() {
            CheckStatus::Ok
        } else {
            CheckStatus::Warning
        },
        format!(
            "{} {}",
            paths.home.display(),
            if paths.home.is_dir() {
                "(config, backups and logs live here)"
            } else {
                "does not exist yet; created on first configuration change"
            }
        ),
    ));

    // Canonical skill root
    let canonical_root = config.canonical_root(env);
    if canonical_root.is_dir() {
        let test_file = canonical_root.join(".skillsync-write-test");
        if std::fs::write(&test_file, b"").is_ok() {
            let _ = std::fs::remove_file(&test_file);
            checks.push(check(
                "canonical-root",
                "Canonical skill root",
                CheckStatus::Ok,
                format!("{} is writable", canonical_root.display()),
            ));
        } else {
            checks.push(check(
                "canonical-root",
                "Canonical skill root",
                CheckStatus::Error,
                format!(
                    "{} is not writable (permission denied)",
                    canonical_root.display()
                ),
            ));
        }

        let git_dir = canonical_root.join(".git");
        checks.push(check(
            "canonical-git",
            "Canonical store git repository",
            if git_dir.exists() {
                CheckStatus::Ok
            } else {
                CheckStatus::Warning
            },
            if git_dir.exists() {
                "canonical store is a git repository (machine sync available)".to_string()
            } else {
                "canonical store is not a git repository — optional for machine sync".to_string()
            },
        ));
    } else if canonical_root.exists() {
        checks.push(check(
            "canonical-root",
            "Canonical skill root",
            CheckStatus::Error,
            format!("{} exists but is not a directory", canonical_root.display()),
        ));
    } else {
        checks.push(check(
            "canonical-root",
            "Canonical skill root",
            CheckStatus::Warning,
            format!(
                "{} does not exist yet; it will be created when you adopt your first skill",
                canonical_root.display()
            ),
        ));
    }

    // Symlink capability (probed in a temp dir, never in user locations).
    checks.push(symlink_capability_check(env));

    // Git availability
    checks.push(git_check(env));

    // Per-tool checks + duplicate detection inputs.
    let mut all_skills: Vec<(String, String, Option<String>)> = Vec::new(); // (name, location, fingerprint)
    for adapter in adapters {
        let tool_id = adapter.id();
        let detection = adapter.detect(env);
        let over = config.tool(tool_id).cloned().unwrap_or_default();
        let locations = adapter.global_skill_locations(env, &over);

        let mut details = Vec::new();
        let mut status = CheckStatus::Ok;
        if !detection.installed {
            status = CheckStatus::Warning;
            details.push("tool not detected".to_string());
        } else {
            details.push(format!("detected ({})", detection.evidence));
        }

        for location in &locations {
            if !location.path.exists() {
                details.push(format!("{}: missing", location.path.display()));
                continue;
            }
            match scan_skills_root(
                env,
                tool_id,
                &location.path,
                &config.canonical_root(env),
                SkillScope::Global,
            ) {
                Ok(skills) => {
                    let managed = skills
                        .iter()
                        .filter(|s| {
                            matches!(
                                s.managedness,
                                Managedness::ManagedSymlink { .. } | Managedness::NativeShared
                            )
                        })
                        .count();
                    details.push(format!(
                        "{}: {} skills ({} managed)",
                        location.path.display(),
                        skills.len(),
                        managed
                    ));
                    for s in &skills {
                        all_skills.push((
                            s.display_name.clone(),
                            location.path.display().to_string(),
                            s.fingerprint.clone(),
                        ));
                    }
                }
                Err(err) => {
                    status = CheckStatus::Error;
                    details.push(format!(
                        "{}: scan failed: {}",
                        location.path.display(),
                        err.message
                    ));
                }
            }
            let broken = count_broken_symlinks(&location.path);
            if broken > 0 {
                status = CheckStatus::Error;
                details.push(format!(
                    "{}: {broken} broken symlink(s) in skills directory",
                    location.path.display()
                ));
            }
        }

        checks.push(check(
            &format!("tool-{tool_id}"),
            adapter.display_name(),
            status,
            details.join("; "),
        ));
    }

    // Duplicate skills across locations (§21, §42). Name matches are only
    // warnings; differing fingerprints on the same name are called out.
    let mut by_name: BTreeMap<String, Vec<(String, Option<String>)>> = BTreeMap::new();
    for (name, location, fp) in all_skills {
        by_name.entry(name).or_default().push((location, fp));
    }
    let duplicates: Vec<String> = by_name
        .iter()
        .filter(|(_, entries)| {
            let locations: std::collections::BTreeSet<&String> =
                entries.iter().map(|(l, _)| l).collect();
            locations.len() > 1
        })
        .map(|(name, entries)| {
            let fps: std::collections::BTreeSet<Option<&String>> =
                entries.iter().map(|(_, f)| f.as_ref()).collect();
            if fps.len() > 1 {
                format!("{name} (different content — potential conflict)")
            } else {
                format!("{name} (identical content)")
            }
        })
        .collect();
    checks.push(if duplicates.is_empty() {
        check(
            "duplicates",
            "Duplicate skills",
            CheckStatus::Ok,
            "no skill name appears in more than one tool location",
        )
    } else {
        check(
            "duplicates",
            "Duplicate skills",
            CheckStatus::Warning,
            format!("found in multiple locations: {}", duplicates.join(", ")),
        )
    });

    DoctorReport {
        os: env.os.to_string(),
        skillsync_home: paths.home.display().to_string(),
        checks,
    }
}

fn symlink_capability_check(env: &EnvContext) -> DoctorCheck {
    match crate::fsutil::probe_symlink_capability() {
        Ok(()) => check(
            "symlink-capability",
            "Symlink capability",
            CheckStatus::Ok,
            format!(
                "directory symlinks work on {} (probed in a temp directory)",
                env.os
            ),
        ),
        Err(reason) => check(
            "symlink-capability",
            "Symlink capability",
            CheckStatus::Warning,
            format!("{reason}; SkillSync will fall back to copies in auto mode"),
        ),
    }
}

fn git_check(env: &EnvContext) -> DoctorCheck {
    let git = env.which("git");
    match git {
        Some(path) => {
            let version = std::process::Command::new(&path)
                .arg("--version")
                .output()
                .ok()
                .and_then(|o| String::from_utf8(o.stdout).ok())
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| "git".to_string());
            check("git", "Git availability", CheckStatus::Ok, version)
        }
        None => check(
            "git",
            "Git availability",
            CheckStatus::Warning,
            "git not found on PATH — optional, used for machine sync",
        ),
    }
}

/// Count symlinks in a skills root whose targets do not resolve.
fn count_broken_symlinks(location: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(location) else {
        return 0;
    };
    entries
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.file_type().map(|t| t.is_symlink()).unwrap_or(false)
                && e.path().canonicalize().is_err()
        })
        .count()
}

/// Count validation issues by severity across a report (helper for CLI/GUI).
pub fn count_severity(
    issues: &[crate::skill::ValidationIssue],
    severity: ValidationSeverity,
) -> usize {
    issues.iter().filter(|i| i.severity == severity).count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::registry;
    use crate::config::Config;

    #[test]
    fn doctor_on_empty_environment_has_no_errors() {
        let tmp = tempfile::tempdir().unwrap();
        let env = EnvContext::with_home(tmp.path());
        let paths = AppPaths {
            home: tmp.path().join(".skillsync"),
        };
        let config = Config::default();
        let report = run_doctor(&env, &paths, &config, &registry());
        // Nothing exists yet: warnings are fine, errors are not.
        assert!(!report.has_errors(), "{report:?}");
        assert!(report.has_warnings());
        // Every tool produced a check.
        for id in ["claude", "codex", "cursor", "gemini"] {
            assert!(
                report.checks.iter().any(|c| c.id == format!("tool-{id}")),
                "missing tool check for {id}"
            );
        }
    }

    #[test]
    fn canonical_root_missing_is_warning_with_guidance() {
        let tmp = tempfile::tempdir().unwrap();
        let env = EnvContext::with_home(tmp.path());
        let paths = AppPaths {
            home: tmp.path().join(".skillsync"),
        };
        let report = run_doctor(&env, &paths, &Config::default(), &registry());
        let canonical = report
            .checks
            .iter()
            .find(|c| c.id == "canonical-root")
            .unwrap();
        assert_eq!(canonical.status, CheckStatus::Warning);
        assert!(canonical.detail.contains("does not exist"));
    }

    #[test]
    fn detects_duplicate_skill_names_across_tools() {
        let tmp = tempfile::tempdir().unwrap();
        let env = EnvContext::with_home(tmp.path());
        let canonical = tmp.path().join(".agents").join("skills");
        let claude_skills = tmp.path().join(".claude").join("skills");
        std::fs::create_dir_all(&canonical).unwrap();
        std::fs::create_dir_all(&claude_skills).unwrap();
        let md = b"---\nname: dupe\ndescription: d\n---\nbody";
        std::fs::create_dir_all(canonical.join("dupe")).unwrap();
        std::fs::write(canonical.join("dupe").join("SKILL.md"), md).unwrap();
        std::fs::create_dir_all(claude_skills.join("dupe")).unwrap();
        std::fs::write(claude_skills.join("dupe").join("SKILL.md"), md).unwrap();

        let paths = AppPaths {
            home: tmp.path().join(".skillsync"),
        };
        let report = run_doctor(&env, &paths, &Config::default(), &registry());
        let dup = report.checks.iter().find(|c| c.id == "duplicates").unwrap();
        assert_eq!(dup.status, CheckStatus::Warning, "{:?}", dup.detail);
        assert!(dup.detail.contains("identical content"), "{}", dup.detail);
    }

    #[test]
    fn broken_symlink_in_tool_location_is_error() {
        let tmp = tempfile::tempdir().unwrap();
        let env = EnvContext::with_home(tmp.path());
        let claude_skills = tmp.path().join(".claude").join("skills");
        std::fs::create_dir_all(&claude_skills).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(tmp.path().join("vanished"), claude_skills.join("dead"))
            .unwrap();

        let paths = AppPaths {
            home: tmp.path().join(".skillsync"),
        };
        let report = run_doctor(&env, &paths, &Config::default(), &registry());
        let claude_check = report
            .checks
            .iter()
            .find(|c| c.id == "tool-claude")
            .unwrap();
        assert_eq!(claude_check.status, CheckStatus::Error);
        assert!(claude_check.detail.contains("broken symlink"));
    }
}
