//! Canonical store operations (design doc §6, prompt §75 Slice 2).
//!
//! The canonical store is a plain directory of skill directories — usable
//! without SkillSync. These operations are the only writers to it:
//!
//! - `adopt_canonical_root` creates the (empty) root folder on explicit
//!   user action; it never reorganizes existing content.
//! - `plan_import` / `execute_import` copy ONE skill into the store. The
//!   plan is computed first, conflicts are never resolved automatically,
//!   and any replacement of existing content is backed up first (§30, §31).
//!
//! Nothing here touches tool directories — that is the sync engine (Slice 3).

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::AppPaths;
use crate::env::EnvContext;
use crate::error::{ErrorCode, Result, SkillSyncError};
use crate::fingerprint::fingerprint_dir;
use crate::fsutil::{
    ensure_within, remove_dir_verified, validate_no_traversal, validate_not_home_or_root,
};
use crate::scan::inspect_skill_dir;

/// What the user chose to do when the target already exists with
/// different content (§18: never decide automatically).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConflictResolution {
    /// Report the conflict, change nothing.
    Skip,
    /// Import under a suffixed name (`skill-2`).
    KeepBoth,
    /// Back up the existing target, then replace it with the source.
    Replace,
}

/// The action a plan will perform.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum ImportAction {
    /// Target does not exist; the skill will be copied in.
    Create { target: PathBuf },
    /// Target exists with identical content; nothing to do.
    AlreadyPresent { target: PathBuf },
    /// Target exists with different content; import as `<name>-2`.
    KeepBoth { target: PathBuf },
    /// Target exists with different content; back it up, then replace.
    Replace {
        target: PathBuf,
        backup_dir: PathBuf,
    },
    /// A conflict needs a resolution before anything can happen.
    Conflict { target: PathBuf },
}

impl ImportAction {
    pub fn kind_label(&self) -> &'static str {
        match self {
            ImportAction::Create { .. } => "create",
            ImportAction::AlreadyPresent { .. } => "alreadyPresent",
            ImportAction::KeepBoth { .. } => "keepBoth",
            ImportAction::Replace { .. } => "replace",
            ImportAction::Conflict { .. } => "conflict",
        }
    }
}

/// The complete, previewable plan for one import (§58 dry-run model).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPlan {
    pub source: PathBuf,
    /// The canonical root this import targets.
    pub canonical_root: PathBuf,
    pub skill_id: String,
    pub action: ImportAction,
    /// Fingerprint of the source skill.
    pub fingerprint: Option<String>,
    pub notes: Vec<String>,
}

/// Result of executing a plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportOutcome {
    pub action_taken: ImportAction,
    pub target: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    pub dry_run: bool,
}

/// Create the canonical root folder if it does not exist (explicit adopt).
/// Refuses to create anything over an existing non-directory and never
/// operates on `/` or the home directory.
pub fn adopt_canonical_root(env: &EnvContext, canonical_root: &Path) -> Result<PathBuf> {
    validate_not_home_or_root(canonical_root, &env.home)?;
    if canonical_root.exists() {
        if canonical_root.is_dir() {
            return Ok(canonical_root.to_path_buf());
        }
        return Err(SkillSyncError::new(
            ErrorCode::UnsafePath,
            "canonical root exists but is not a directory",
        )
        .with_path(canonical_root)
        .recoverable());
    }
    // Parent boundaries: creating the leaf is enough; deeper missing
    // parents are created too, but the root itself must be inside home
    // or an absolute sensible path (validated above).
    fs::create_dir_all(canonical_root).map_err(|e| SkillSyncError::io(&e, canonical_root))?;
    Ok(canonical_root.to_path_buf())
}

/// Plan an import of `source` (a skill directory) into `canonical_root`.
/// Read-only. Conflicts are surfaced, never auto-resolved.
pub fn plan_import(
    env: &EnvContext,
    paths: &AppPaths,
    source: &Path,
    canonical_root: &Path,
    resolution: ConflictResolution,
) -> Result<ImportPlan> {
    validate_no_traversal(source)?;
    validate_not_home_or_root(source, &env.home)?;

    let skill_id = source
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| {
            SkillSyncError::new(ErrorCode::InvalidSkill, "source path has no file name")
                .with_path(source)
        })?;

    // The source must be a real skill directory (read-only inspection).
    let fingerprint = match inspect_skill_dir(source) {
        Ok((_, _files, fingerprint)) => fingerprint,
        Err(e) => {
            return Err(SkillSyncError::new(
                ErrorCode::InvalidSkill,
                format!("`{skill_id}` is not a valid skill: {}", e.message),
            )
            .with_path(source)
            .with_skill(&skill_id)
            .recoverable())
        }
    };

    let target = canonical_root.join(&skill_id);
    ensure_within_when_root_exists(&target, canonical_root)?;

    let mut notes = Vec::new();
    if !canonical_root.is_dir() {
        notes.push("canonical root does not exist yet; it will be created by this import".into());
    }
    if !target.exists() {
        return Ok(ImportPlan {
            source: source.to_path_buf(),
            canonical_root: canonical_root.to_path_buf(),
            skill_id,
            action: ImportAction::Create { target },
            fingerprint,
            notes,
        });
    }

    // Target exists: compare content, never timestamps (§54).
    let target_fp = fingerprint_dir(&target).ok();
    if target_fp.is_some() && target_fp == fingerprint {
        notes.push("an identical skill already exists in the canonical store".into());
        return Ok(ImportPlan {
            source: source.to_path_buf(),
            canonical_root: canonical_root.to_path_buf(),
            skill_id,
            action: ImportAction::AlreadyPresent { target },
            fingerprint,
            notes,
        });
    }

    match resolution {
        ConflictResolution::Skip => Ok(ImportPlan {
            source: source.to_path_buf(),
            canonical_root: canonical_root.to_path_buf(),
            skill_id,
            action: ImportAction::Conflict { target },
            fingerprint,
            notes,
        }),
        ConflictResolution::KeepBoth => {
            let alt = find_free_name(canonical_root, &skill_id);
            ensure_within_when_root_exists(&canonical_root.join(&alt), canonical_root)?;
            notes.push(format!(
                "`{skill_id}` exists with different content; importing as `{alt}`"
            ));
            Ok(ImportPlan {
                source: source.to_path_buf(),
                canonical_root: canonical_root.to_path_buf(),
                skill_id,
                action: ImportAction::KeepBoth {
                    target: canonical_root.join(alt),
                },
                fingerprint,
                notes,
            })
        }
        ConflictResolution::Replace => {
            let backup_dir = backup_dir_for(paths, "canonical", &skill_id);
            notes.push(format!(
                "`{skill_id}` exists with different content; existing copy will be backed up to {} first",
                backup_dir.display()
            ));
            Ok(ImportPlan {
                source: source.to_path_buf(),
                canonical_root: canonical_root.to_path_buf(),
                skill_id,
                action: ImportAction::Replace { target, backup_dir },
                fingerprint,
                notes,
            })
        }
    }
}

/// Execute a previously computed plan. With `dry_run` nothing is written.
/// The source directory is never modified.
pub fn execute_import(env: &EnvContext, plan: &ImportPlan, dry_run: bool) -> Result<ImportOutcome> {
    let target = match &plan.action {
        ImportAction::Create { target }
        | ImportAction::KeepBoth { target }
        | ImportAction::Replace { target, .. }
        | ImportAction::AlreadyPresent { target } => target.clone(),
        ImportAction::Conflict { .. } => {
            return Err(SkillSyncError::new(
                ErrorCode::TargetConflict,
                "import blocked: canonical skill exists with different content; choose \
                 keepBoth or replace explicitly",
            )
            .with_path(source_target(plan))
            .with_skill(&plan.skill_id)
            .recoverable());
        }
    };

    match &plan.action {
        ImportAction::AlreadyPresent { .. } => Ok(ImportOutcome {
            action_taken: plan.action.clone(),
            target: target.clone(),
            fingerprint: plan.fingerprint.clone(),
            dry_run,
        }),
        ImportAction::Replace { backup_dir, .. } => {
            if dry_run {
                return Ok(ImportOutcome {
                    action_taken: plan.action.clone(),
                    target: target.clone(),
                    fingerprint: plan.fingerprint.clone(),
                    dry_run: true,
                });
            }
            backup_skill_dir(
                &target,
                backup_dir,
                &plan.skill_id,
                "import",
                "canonical",
                env,
            )?;
            // Ownership & boundary verified before the recursive delete:
            // the target is inside the canonical root and was just backed up.
            ensure_within(&target, &plan.canonical_root)?;
            remove_dir_verified(&target, "canonical skill being replaced")?;
            copy_skill_dir(&plan.source, &target)?;
            Ok(ImportOutcome {
                action_taken: ImportAction::Replace {
                    target: target.clone(),
                    backup_dir: backup_dir.clone(),
                },
                target: target.clone(),
                fingerprint: fingerprint_dir(&target).ok(),
                dry_run: false,
            })
        }
        ImportAction::Create { .. } | ImportAction::KeepBoth { .. } => {
            if dry_run {
                return Ok(ImportOutcome {
                    action_taken: plan.action.clone(),
                    target: target.clone(),
                    fingerprint: plan.fingerprint.clone(),
                    dry_run: true,
                });
            }
            if target.exists() {
                return Err(SkillSyncError::new(
                    ErrorCode::TargetConflict,
                    "target appeared after planning; refusing to overwrite",
                )
                .with_path(&target)
                .with_skill(&plan.skill_id));
            }
            // First import may create the canonical root itself (empty dir).
            if !plan.canonical_root.is_dir() {
                fs::create_dir_all(&plan.canonical_root)
                    .map_err(|e| SkillSyncError::io(&e, &plan.canonical_root))?;
            }
            copy_skill_dir(&plan.source, &target)?;
            Ok(ImportOutcome {
                action_taken: plan.action.clone(),
                target: target.clone(),
                fingerprint: fingerprint_dir(&target).ok(),
                dry_run: false,
            })
        }
        ImportAction::Conflict { .. } => unreachable!(),
    }
}

fn source_target(plan: &ImportPlan) -> &Path {
    match &plan.action {
        ImportAction::Conflict { target } => target,
        _ => &plan.source,
    }
}

/// Containment check that tolerates a not-yet-existing canonical root
/// (first import creates it). Falls back to lexical validation, which
/// `validate_no_traversal` makes safe (absolute, no `..`).
fn ensure_within_when_root_exists(candidate: &Path, canonical_root: &Path) -> Result<()> {
    if canonical_root.is_dir() {
        ensure_within(candidate, canonical_root)
    } else {
        validate_no_traversal(candidate)
    }
}

/// Copy one skill directory tree, preserving subdirectories and symlinks.
/// Regular files are copied; symlinks are recreated as symlinks (targets
/// untouched); the walk never follows links, so cycles terminate.
pub fn copy_skill_dir(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target).map_err(|e| SkillSyncError::io(&e, target))?;
    for entry in walkdir::WalkDir::new(source)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let rel = match entry.path().strip_prefix(source) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let dest = target.join(rel);
        let ft = entry.file_type();
        if ft.is_symlink() {
            let link_target =
                fs::read_link(entry.path()).map_err(|e| SkillSyncError::io(&e, entry.path()))?;
            #[cfg(unix)]
            std::os::unix::fs::symlink(&link_target, &dest)
                .map_err(|e| SkillSyncError::io(&e, &dest))?;
            #[cfg(windows)]
            std::os::windows::fs::symlink_file(&link_target, &dest)
                .or_else(|_| std::os::windows::fs::symlink_dir(&link_target, &dest))
                .map_err(|e| SkillSyncError::io(&e, &dest))?;
            #[cfg(not(any(unix, windows)))]
            return Err(SkillSyncError::new(
                ErrorCode::Io,
                "symlinks are not supported on this platform",
            )
            .with_path(&dest));
        } else if ft.is_dir() {
            fs::create_dir_all(&dest).map_err(|e| SkillSyncError::io(&e, &dest))?;
        } else {
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| SkillSyncError::io(&e, parent))?;
            }
            fs::copy(entry.path(), &dest).map_err(|e| SkillSyncError::io(&e, &dest))?;
        }
    }
    Ok(())
}

/// Back up a skill directory before replacing it (§31): copy of the tree +
/// a small metadata file explaining when/what/where-from.
pub(crate) fn backup_skill_dir(
    target: &Path,
    backup_dir: &Path,
    skill_id: &str,
    operation: &str,
    tool_id: &str,
    env: &EnvContext,
) -> Result<()> {
    if backup_dir.exists() {
        return Err(SkillSyncError::new(
            ErrorCode::Io,
            "backup directory already exists; refusing to overwrite a backup",
        )
        .with_path(backup_dir)
        .recoverable());
    }
    if let Some(parent) = backup_dir.parent() {
        fs::create_dir_all(parent).map_err(|e| SkillSyncError::io(&e, parent))?;
    }
    copy_skill_dir(target, backup_dir)?;
    let metadata = serde_json::json!({
        "when": iso_utc_now(),
        "operation": operation,
        "tool": tool_id,
        "skill": skill_id,
        "originalPath": target.to_string_lossy(),
        "home": env.home.to_string_lossy(),
    });
    let metadata_path = backup_dir.with_extension("json");
    fs::write(
        &metadata_path,
        serde_json::to_vec_pretty(&metadata).unwrap_or_else(|_| b"{}".to_vec()),
    )
    .map_err(|e| SkillSyncError::io(&e, &metadata_path))?;
    Ok(())
}

/// SkillSync-owned backup location: `~/.skillsync/backups/<ts>-<op>-…`.
pub fn backup_dir_for(paths: &AppPaths, label: &str, skill_id: &str) -> PathBuf {
    paths
        .backups_dir()
        .join(format!("{ts}-{label}-{skill_id}", ts = timestamp_compact()))
}

/// `YYYYMMDD-HHMMSS` in UTC, no external time dependency.
pub fn timestamp_compact() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, m, d, hh, mm, ss) = civil_utc(secs);
    format!("{y:04}{m:02}{d:02}-{hh:02}{mm:02}{ss:02}")
}

/// ISO 8601 UTC timestamp for backup metadata.
pub fn iso_utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let (y, m, d, hh, mm, ss) = civil_utc(secs);
    format!("{y:04}-{m:02}-{d:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

/// Convert unix seconds to UTC civil time (Howard Hinnant's algorithm).
fn civil_utc(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let days = secs / 86_400;
    let rem = secs % 86_400;
    let (hh, mm, ss) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d, hh, mm, ss)
}

/// First free `name`, `name-2`, `name-3`, ... inside `root`.
fn find_free_name(root: &Path, name: &str) -> String {
    let mut candidate = name.to_string();
    let mut n = 2;
    while root.join(&candidate).exists() {
        candidate = format!("{name}-{n}");
        n += 1;
    }
    candidate
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_skill(root: &Path, name: &str, body: &str) -> PathBuf {
        let dir = root.join(name);
        fs::create_dir_all(dir.join("scripts")).unwrap();
        fs::write(dir.join("SKILL.md"), body).unwrap();
        fs::write(dir.join("scripts/run.sh"), b"echo hi\n").unwrap();
        dir
    }

    const V1: &str = "---\nname: git-commit\ndescription: v1\n---\n# v1\n";
    const V2: &str = "---\nname: git-commit\ndescription: v2\n---\n# v2\n";

    fn sandbox() -> (tempfile::TempDir, EnvContext, AppPaths, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let env = EnvContext::with_home(tmp.path().join("home"));
        let paths = AppPaths {
            home: tmp.path().join("skillsync-home"),
        };
        let canonical = tmp.path().join("store");
        (tmp, env, paths, canonical)
    }

    #[test]
    fn adopt_creates_missing_root_and_is_idempotent() {
        let (_tmp, env, _paths, canonical) = sandbox();
        assert!(!canonical.exists());
        let p = adopt_canonical_root(&env, &canonical).unwrap();
        assert!(p.is_dir());
        // idempotent
        adopt_canonical_root(&env, &canonical).unwrap();
    }

    #[test]
    fn adopt_refuses_file_and_home() {
        let (tmp, env, _paths, _canonical) = sandbox();
        let file = tmp.path().join("afile");
        fs::write(&file, b"x").unwrap();
        let err = adopt_canonical_root(&env, &file).unwrap_err();
        assert_eq!(err.code, ErrorCode::UnsafePath);
        let err = adopt_canonical_root(&env, &env.home).unwrap_err();
        assert_eq!(err.code, ErrorCode::UnsafePath);
    }

    #[test]
    fn import_fresh_creates_full_tree_and_leaves_source_intact() {
        let (tmp, env, paths, canonical) = sandbox();
        fs::create_dir_all(&canonical).unwrap();
        let source = write_skill(
            &tmp.path().join("tools").join("claude-skills"),
            "git-commit",
            V1,
        );
        let source_bytes = fs::read(source.join("SKILL.md")).unwrap();

        let plan =
            plan_import(&env, &paths, &source, &canonical, ConflictResolution::Skip).unwrap();
        assert_eq!(
            plan.action,
            ImportAction::Create {
                target: canonical.join("git-commit")
            }
        );

        let outcome = execute_import(&env, &plan, false).unwrap();
        assert!(!outcome.dry_run);
        let imported = canonical.join("git-commit");
        assert_eq!(fs::read(imported.join("SKILL.md")).unwrap(), source_bytes);
        assert!(imported.join("scripts/run.sh").is_file());
        // fingerprint of the copy matches the source
        assert_eq!(
            fingerprint_dir(&imported).unwrap(),
            fingerprint_dir(&source).unwrap()
        );
        // source untouched
        assert_eq!(fs::read(source.join("SKILL.md")).unwrap(), source_bytes);
    }

    #[test]
    fn import_identical_content_is_a_no_op() {
        let (tmp, env, paths, canonical) = sandbox();
        fs::create_dir_all(&canonical).unwrap();
        let source = write_skill(tmp.path(), "dupe", V1);
        copy_skill_dir(&source, &canonical.join("dupe")).unwrap();

        let plan =
            plan_import(&env, &paths, &source, &canonical, ConflictResolution::Skip).unwrap();
        assert_eq!(
            plan.action,
            ImportAction::AlreadyPresent {
                target: canonical.join("dupe")
            }
        );
        let outcome = execute_import(&env, &plan, false).unwrap();
        assert!(!outcome.dry_run);
        // no extra copy was created
        assert!(!canonical.join("dupe-2").exists());
    }

    #[test]
    fn import_conflicting_content_without_resolution_is_blocked() {
        let (tmp, env, paths, canonical) = sandbox();
        fs::create_dir_all(&canonical).unwrap();
        let source = write_skill(tmp.path(), "git-commit", V2);
        write_skill(&canonical, "git-commit", V1);

        let plan =
            plan_import(&env, &paths, &source, &canonical, ConflictResolution::Skip).unwrap();
        assert!(matches!(plan.action, ImportAction::Conflict { .. }));
        let err = execute_import(&env, &plan, false).unwrap_err();
        assert_eq!(err.code, ErrorCode::TargetConflict);
        // existing content untouched
        assert_eq!(
            fs::read(canonical.join("git-commit").join("SKILL.md")).unwrap(),
            V1.as_bytes()
        );
    }

    #[test]
    fn keep_both_imports_under_suffixed_name() {
        let (tmp, env, paths, canonical) = sandbox();
        fs::create_dir_all(&canonical).unwrap();
        let source = write_skill(tmp.path(), "git-commit", V2);
        write_skill(&canonical, "git-commit", V1);

        let plan = plan_import(
            &env,
            &paths,
            &source,
            &canonical,
            ConflictResolution::KeepBoth,
        )
        .unwrap();
        match &plan.action {
            ImportAction::KeepBoth { target } => {
                assert_eq!(target.file_name().unwrap(), "git-commit-2");
            }
            other => panic!("expected keepBoth, got {other:?}"),
        }
        execute_import(&env, &plan, false).unwrap();
        assert!(canonical.join("git-commit-2").join("SKILL.md").is_file());
        assert_eq!(
            fs::read(canonical.join("git-commit").join("SKILL.md")).unwrap(),
            V1.as_bytes(),
            "original must stay untouched"
        );
    }

    #[test]
    fn replace_backs_up_then_copies() {
        let (tmp, env, paths, canonical) = sandbox();
        fs::create_dir_all(&canonical).unwrap();
        let source = write_skill(tmp.path(), "git-commit", V2);
        write_skill(&canonical, "git-commit", V1);

        let plan = plan_import(
            &env,
            &paths,
            &source,
            &canonical,
            ConflictResolution::Replace,
        )
        .unwrap();
        let outcome = execute_import(&env, &plan, false).unwrap();
        assert!(!outcome.dry_run);

        // New content in place
        assert_eq!(
            fs::read(canonical.join("git-commit").join("SKILL.md")).unwrap(),
            V2.as_bytes()
        );
        // Backup exists with old content + metadata
        let backup_root = paths.backups_dir();
        let backup_dirs: Vec<_> = fs::read_dir(&backup_root)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        assert_eq!(backup_dirs.len(), 1, "one backup directory expected");
        let backup_dir = backup_dirs[0].path();
        assert!(backup_dir.join("SKILL.md").is_file());
        assert_eq!(
            fs::read(backup_dir.join("SKILL.md")).unwrap(),
            V1.as_bytes(),
            "backup holds the replaced content"
        );
        let metadata_path = backup_dir.with_extension("json");
        let metadata: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(metadata_path).unwrap()).unwrap();
        assert_eq!(metadata["tool"], "canonical");
        assert_eq!(metadata["skill"], "git-commit");
        assert!(metadata["originalPath"].as_str().is_some());
    }

    #[test]
    fn dry_run_writes_nothing() {
        let (tmp, env, paths, canonical) = sandbox();
        fs::create_dir_all(&canonical).unwrap();
        let source = write_skill(tmp.path(), "fresh-skill", V1);

        let plan =
            plan_import(&env, &paths, &source, &canonical, ConflictResolution::Skip).unwrap();
        let outcome = execute_import(&env, &plan, true).unwrap();
        assert!(outcome.dry_run);
        assert!(
            !canonical.join("fresh-skill").exists(),
            "dry run must not copy"
        );

        let source_conflict = write_skill(tmp.path(), "git-commit", V2);
        write_skill(&canonical, "git-commit", V1);
        let plan = plan_import(
            &env,
            &paths,
            &source_conflict,
            &canonical,
            ConflictResolution::Replace,
        )
        .unwrap();
        let outcome = execute_import(&env, &plan, true).unwrap();
        assert!(outcome.dry_run);
        assert_eq!(
            fs::read(canonical.join("git-commit").join("SKILL.md")).unwrap(),
            V1.as_bytes(),
            "dry run must not replace"
        );
        assert!(!paths.backups_dir().exists(), "dry run must not back up");
    }

    #[test]
    fn import_refuses_non_skill_source() {
        let (tmp, env, paths, canonical) = sandbox();
        fs::create_dir_all(&canonical).unwrap();
        let not_a_skill = tmp.path().join("random-dir");
        fs::create_dir_all(&not_a_skill).unwrap();
        let err = plan_import(
            &env,
            &paths,
            &not_a_skill,
            &canonical,
            ConflictResolution::Skip,
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidSkill);
    }

    #[test]
    fn civil_utc_known_value() {
        // 2026-08-29T00:00:00Z = 1787961600
        assert_eq!(civil_utc(1_787_961_600), (2026, 8, 29, 0, 0, 0));
        assert_eq!(civil_utc(0), (1970, 1, 1, 0, 0, 0));
    }

    #[test]
    fn first_import_creates_missing_canonical_root() {
        let (tmp, env, paths, canonical) = sandbox();
        assert!(!canonical.exists());
        let source = write_skill(&tmp.path().join("somewhere"), "first-skill", V1);

        let plan =
            plan_import(&env, &paths, &source, &canonical, ConflictResolution::Skip).unwrap();
        assert!(plan.notes.iter().any(|n| n.contains("will be created")));
        execute_import(&env, &plan, false).unwrap();
        assert!(canonical.is_dir());
        assert!(canonical.join("first-skill").join("SKILL.md").is_file());
    }
}
