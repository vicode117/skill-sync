//! Conflict management (design doc §7e Slice 5, prompt §18, §54, §55).
//!
//! A conflict exists when a canonical skill and an unmanaged target with
//! the same name hold different content (§21: names alone never merge).
//! Conflicts are detected, reported, compared — and resolved only by an
//! explicit user choice. Every resolution backs up whatever it replaces
//! (§31); timestamps never decide (§54).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::env::{abbreviate_home, EnvContext};
use crate::error::{ErrorCode, Result, SkillSyncError};
use crate::fingerprint::fingerprint_dir;
use crate::fsutil::remove_dir_verified;
use crate::managed::ManagedRegistry;
use crate::scan::ScannedSkill;
use crate::skill::Skill;
use crate::store;
use crate::sync::{EffectiveMethod, SyncContext};

/// What the user chose to do with a conflict (§18).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Resolution {
    /// Replace the target with the canonical version (target backed up).
    UseCanonical,
    /// Replace the canonical skill with the target version (canonical
    /// backed up), then install the managed version into the tool.
    ImportTarget,
    /// Import the target version into the store under a new name; keep
    /// both sides untouched.
    KeepBoth,
}

/// One differing file between two skill directories (§55).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum DiffKind {
    Added,
    Removed,
    Modified,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffEntry {
    /// Slash-separated path relative to the skill root.
    pub relative_path: String,
    pub kind: DiffKind,
    /// For modified text files: a line-based textual diff.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_diff: Option<String>,
}

/// A detected canonical ⇄ target conflict.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictReport {
    pub skill_id: String,
    pub skill_name: String,
    pub tool_id: String,
    pub tool_display_name: String,
    pub canonical_path: PathBuf,
    pub canonical_display: String,
    pub target_path: PathBuf,
    pub target_display: String,
    pub canonical_fingerprint: Option<String>,
    pub target_fingerprint: Option<String>,
    pub ignored: bool,
}

/// Report of a resolution run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolutionReport {
    pub skill_id: String,
    pub tool_id: String,
    pub resolution: Resolution,
    pub dry_run: bool,
    pub backups: Vec<PathBuf>,
    pub installed: bool,
    pub notes: Vec<String>,
}

/// Detect canonical ⇄ unmanaged-target conflicts from one scan pass.
/// Purely read-only; honors the user's Ignore choices.
pub fn detect_conflicts(
    env: &EnvContext,
    config: &Config,
    canonical_skills: &[Skill],
    scanned: &[ScannedSkill],
    tool_names: &[(String, String)],
) -> Vec<ConflictReport> {
    let mut conflicts = Vec::new();
    for scanned_skill in scanned {
        // Only plain, unmanaged directories can conflict.
        if !matches!(
            scanned_skill.managedness,
            crate::scan::Managedness::Unmanaged
        ) {
            continue;
        }
        let Some(canonical) = canonical_skills.iter().find(|s| s.id == scanned_skill.id) else {
            continue;
        };
        // Different content only (identical copies are import candidates).
        if canonical.fingerprint.is_some() && canonical.fingerprint == scanned_skill.fingerprint {
            continue;
        }
        let tool_display = tool_names
            .iter()
            .find(|(id, _)| *id == scanned_skill.tool_id)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| scanned_skill.tool_id.clone());
        conflicts.push(ConflictReport {
            skill_id: canonical.id.clone(),
            skill_name: canonical.display_name.clone(),
            tool_id: scanned_skill.tool_id.clone(),
            tool_display_name: tool_display,
            canonical_path: canonical.root.clone(),
            canonical_display: abbreviate_home(&canonical.root, env),
            target_path: scanned_skill.path.clone(),
            target_display: abbreviate_home(&scanned_skill.path, env),
            canonical_fingerprint: canonical.fingerprint.clone(),
            target_fingerprint: scanned_skill.fingerprint.clone(),
            ignored: config.is_conflict_ignored(&canonical.id, &scanned_skill.tool_id),
        });
    }
    conflicts.sort_by(|a, b| (&a.skill_id, &a.tool_id).cmp(&(&b.skill_id, &b.tool_id)));
    conflicts
}

/// Directory-aware diff of two skill trees (§55): added/removed/modified
/// files; textual line diffs for modified text files. Binary files are
/// reported without a text diff.
pub fn diff_skill_dirs(canonical: &Path, target: &Path) -> Result<Vec<DiffEntry>> {
    let collect = |root: &Path| -> Result<std::collections::BTreeMap<String, PathBuf>> {
        let mut map = std::collections::BTreeMap::new();
        for entry in walkdir::WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let rel = match entry.path().strip_prefix(root) {
                Ok(r) => r,
                Err(_) => continue,
            };
            if rel.as_os_str().is_empty() || entry.file_type().is_dir() {
                continue;
            }
            map.insert(
                rel.to_string_lossy().replace('\\', "/"),
                entry.path().to_path_buf(),
            );
        }
        Ok(map)
    };

    let canonical_files = collect(canonical)?;
    let target_files = collect(target)?;
    let mut entries = Vec::new();

    for (path, target_path) in &target_files {
        match canonical_files.get(path) {
            None => entries.push(DiffEntry {
                relative_path: path.clone(),
                kind: DiffKind::Added,
                text_diff: None,
            }),
            Some(canonical_path) => {
                let a = std::fs::read(canonical_path)
                    .map_err(|e| SkillSyncError::io(&e, canonical_path))?;
                let b =
                    std::fs::read(target_path).map_err(|e| SkillSyncError::io(&e, target_path))?;
                if a != b {
                    let text_diff = if let (Some(a), Some(b)) = (as_text(&a), as_text(&b)) {
                        Some(line_diff(&a, &b))
                    } else {
                        None
                    };
                    entries.push(DiffEntry {
                        relative_path: path.clone(),
                        kind: DiffKind::Modified,
                        text_diff,
                    });
                }
            }
        }
    }
    for (path, canonical_path) in &canonical_files {
        if !target_files.contains_key(path) {
            entries.push(DiffEntry {
                relative_path: path.clone(),
                kind: DiffKind::Removed,
                text_diff: None,
            });
            let _ = canonical_path;
        }
    }
    Ok(entries)
}

const MAX_DIFF_BYTES: usize = 256 * 1024;

/// UTF-8 text within a size cap is diffable; anything else is binary.
fn as_text(bytes: &[u8]) -> Option<Vec<String>> {
    if bytes.len() > MAX_DIFF_BYTES {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    Some(text.lines().map(|l| l.to_string()).collect())
}

/// Line-based diff using LCS; output lists removed (`<`) and added (`>`)
/// lines with line numbers. Deliberately simple — this is a review aid,
/// not a patch format.
fn line_diff(a: &[String], b: &[String]) -> String {
    let n = a.len();
    let m = b.len();
    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }
    let mut out = String::new();
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            out.push_str(&format!("{}   < {}\n", i + 1, a[i]));
            i += 1;
        } else {
            out.push_str(&format!("    {} > {}\n", j + 1, b[j]));
            j += 1;
        }
    }
    while i < n {
        out.push_str(&format!("{}   < {}\n", i + 1, a[i]));
        i += 1;
    }
    while j < m {
        out.push_str(&format!("    {} > {}\n", j + 1, b[j]));
        j += 1;
    }
    out.trim_end().to_string()
}

/// Apply a conflict resolution. `method` decides how the managed
/// installation is (re)created after UseCanonical / ImportTarget.
pub fn resolve_conflict(
    ctx: &SyncContext,
    report: &ConflictReport,
    canonical: &Skill,
    resolution: Resolution,
    method: EffectiveMethod,
    registry: &mut ManagedRegistry,
    dry_run: bool,
) -> Result<ResolutionReport> {
    if report.ignored {
        return Err(SkillSyncError::new(
            ErrorCode::TargetConflict,
            "this conflict was ignored; unignore it before resolving",
        )
        .with_skill(&report.skill_id)
        .with_tool(&report.tool_id)
        .recoverable());
    }
    let env = ctx.env;
    let mut backups = Vec::new();
    let mut notes = Vec::new();

    match resolution {
        Resolution::UseCanonical => {
            ensure_unmanaged(ctx, report, registry)?;
            let backup = store::backup_dir_for(
                ctx.paths,
                "conflict",
                &format!("{}-{}", report.tool_id, report.skill_id),
            );
            notes.push(format!(
                "target backed up to {}",
                abbreviate_home(&backup, env)
            ));
            if !dry_run {
                store::backup_skill_dir(
                    &report.target_path,
                    &backup,
                    &report.skill_id,
                    "conflict",
                    &report.tool_id,
                    env,
                )?;
                remove_dir_verified(&report.target_path, "conflicting unmanaged target")?;
                install_canonical(ctx, report, canonical, method, registry)?;
            }
            backups.push(backup);
        }
        Resolution::ImportTarget => {
            ensure_unmanaged(ctx, report, registry)?;
            let canonical_backup = store::backup_dir_for(ctx.paths, "import", &report.skill_id);
            let target_backup = store::backup_dir_for(
                ctx.paths,
                "conflict",
                &format!("{}-{}", report.tool_id, report.skill_id),
            );
            notes.push(format!(
                "canonical backed up to {}",
                abbreviate_home(&canonical_backup, env)
            ));
            if !dry_run {
                // Canonical adopts the target content (canonical backed up).
                store::backup_skill_dir(
                    &report.canonical_path,
                    &canonical_backup,
                    &report.skill_id,
                    "import",
                    "canonical",
                    env,
                )?;
                remove_dir_verified(&report.canonical_path, "canonical skill (import)")?;
                store::copy_skill_dir(&report.target_path, &report.canonical_path)?;
                // Target becomes managed: back up, remove, install.
                store::backup_skill_dir(
                    &report.target_path,
                    &target_backup,
                    &report.skill_id,
                    "conflict",
                    &report.tool_id,
                    env,
                )?;
                remove_dir_verified(&report.target_path, "conflicting unmanaged target")?;
                install_canonical(ctx, report, canonical, method, registry)?;
            }
            backups.push(canonical_backup);
            backups.push(target_backup);
        }
        Resolution::KeepBoth => {
            // Import the target version under a fresh name; both sides stay.
            let parent = canonical.root.parent().ok_or_else(|| {
                SkillSyncError::new(ErrorCode::InvalidSkill, "canonical skill has no parent")
                    .with_path(&canonical.root)
            })?;
            let alt = {
                let mut candidate = format!("{}-2", report.skill_id);
                let mut n = 2;
                while parent.join(&candidate).exists() {
                    n += 1;
                    candidate = format!("{}-{n}", report.skill_id);
                }
                candidate
            };
            let dest = parent.join(&alt);
            notes.push(format!("target imported as `{alt}` in the canonical store"));
            if !dry_run {
                store::copy_skill_dir(&report.target_path, &dest)?;
            }
        }
    }

    Ok(ResolutionReport {
        skill_id: report.skill_id.clone(),
        tool_id: report.tool_id.clone(),
        resolution,
        dry_run,
        backups,
        installed: !dry_run && resolution != Resolution::KeepBoth,
        notes,
    })
}

/// The target must still be an unmanaged directory at resolution time.
fn ensure_unmanaged(
    _ctx: &SyncContext,
    report: &ConflictReport,
    registry: &ManagedRegistry,
) -> Result<()> {
    let meta = std::fs::symlink_metadata(&report.target_path)
        .map_err(|e| SkillSyncError::io(&e, &report.target_path))?;
    if meta.file_type().is_symlink() {
        return Err(SkillSyncError::new(
            ErrorCode::TargetConflict,
            "target is now a symlink; re-scan before resolving",
        )
        .with_path(&report.target_path)
        .recoverable());
    }
    if !meta.is_dir() {
        return Err(SkillSyncError::new(
            ErrorCode::TargetConflict,
            "target is no longer a directory; re-scan before resolving",
        )
        .with_path(&report.target_path)
        .recoverable());
    }
    if registry.find_by_target(&report.target_path).is_some() {
        return Err(SkillSyncError::new(
            ErrorCode::TargetConflict,
            "target is a managed copy; sync instead of resolving a conflict",
        )
        .with_path(&report.target_path)
        .recoverable());
    }
    Ok(())
}

/// Install the canonical skill into the tool directory (link or copy).
fn install_canonical(
    _ctx: &SyncContext,
    report: &ConflictReport,
    canonical: &Skill,
    method: EffectiveMethod,
    registry: &mut ManagedRegistry,
) -> Result<()> {
    let target = &report.target_path;
    match method {
        EffectiveMethod::Symlink => {
            #[cfg(unix)]
            let result = std::os::unix::fs::symlink(&canonical.root, target);
            #[cfg(windows)]
            let result = std::os::windows::fs::symlink_dir(&canonical.root, target);
            #[cfg(not(any(unix, windows)))]
            let result: std::io::Result<()> = Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "symlinks unsupported",
            ));
            result.map_err(|e| SkillSyncError::io(&e, target))?;
        }
        EffectiveMethod::Copy => {
            store::copy_skill_dir(&canonical.root, target)?;
            registry.upsert(crate::managed::ManagedInstall {
                tool_id: report.tool_id.clone(),
                skill_id: report.skill_id.clone(),
                target: target.to_path_buf(),
                fingerprint: fingerprint_dir(target).unwrap_or_default(),
                installed_at: store::iso_utc_now(),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppPaths;

    fn write(root: &Path, rel: &str, bytes: &[u8]) {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }

    #[test]
    fn diff_reports_added_removed_modified_with_text() {
        let tmp = tempfile::tempdir().unwrap();
        let a = tmp.path().join("canonical");
        let b = tmp.path().join("target");
        write(&a, "SKILL.md", b"# same\n");
        write(&a, "old.md", b"old file\n");
        write(&a, "changed.md", b"line1\nline2\n");
        write(&b, "SKILL.md", b"# same\n");
        write(&b, "changed.md", b"line1\nline2 changed\n");
        write(&b, "new.md", b"brand new\n");
        write(&b, "blob.bin", b"\x00\x01\x02");

        let diff = diff_skill_dirs(&a, &b).unwrap();
        let paths: Vec<(&str, &str)> = diff
            .iter()
            .map(|e| (e.relative_path.as_str(), e.kind.kind_label()))
            .collect();
        assert!(paths.contains(&("old.md", "removed")));
        assert!(paths.contains(&("new.md", "added")));
        assert!(paths.contains(&("blob.bin", "added")));
        let changed = diff
            .iter()
            .find(|e| e.relative_path == "changed.md")
            .unwrap();
        assert_eq!(changed.kind.kind_label(), "modified");
        let text = changed.text_diff.as_ref().unwrap();
        assert!(text.contains("< line2"));
        assert!(text.contains("> line2 changed"));
    }

    #[test]
    fn detect_pairs_same_name_different_content() {
        let tmp = tempfile::tempdir().unwrap();
        let mut env = EnvContext::with_home(tmp.path().join("home"));
        env.env.insert("PATH".into(), String::new());
        let _paths = AppPaths {
            home: tmp.path().join("sync-home"),
        };
        let canonical_dir = env.home.join(".agents").join("skills");
        let tool_dir = env.home.join(".claude").join("skills");
        write(
            &canonical_dir,
            "tdd/SKILL.md",
            b"---\nname: tdd\ndescription: c\n---\ncanonical",
        );
        write(
            &tool_dir,
            "tdd/SKILL.md",
            b"---\nname: tdd\ndescription: t\n---\ntarget",
        );

        let canonical = crate::scan::inspect_as_skill(
            &env,
            &canonical_dir.join("tdd"),
            crate::skill::SkillScope::Global,
            crate::skill::SkillSource::Canonical,
        )
        .unwrap();
        let config = Config::default();
        let scanned = crate::scan::scan_skills_root(
            &env,
            "claude",
            &tool_dir,
            &canonical_dir,
            crate::skill::SkillScope::Global,
        )
        .unwrap();

        let conflicts = detect_conflicts(
            &env,
            &config,
            std::slice::from_ref(&canonical),
            &scanned,
            &[("claude".into(), "Claude Code".into())],
        );
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].skill_id, "tdd");
        assert_eq!(conflicts[0].tool_id, "claude");
        assert!(!conflicts[0].ignored);

        // Ignore choice suppresses it (flagged, not dropped — UI decides).
        let mut config = config;
        config.set_conflict_ignored("tdd", "claude", true);
        let conflicts = detect_conflicts(
            &env,
            &config,
            &[canonical],
            &scanned,
            &[("claude".into(), "Claude Code".into())],
        );
        assert!(conflicts[0].ignored);
    }
}

impl DiffKind {
    pub fn kind_label(&self) -> &'static str {
        match self {
            DiffKind::Added => "added",
            DiffKind::Removed => "removed",
            DiffKind::Modified => "modified",
        }
    }
}

#[cfg(test)]
mod resolution_tests {
    use super::*;
    use crate::adapter::claude::ClaudeAdapter;
    use crate::adapter::gemini::GeminiAdapter;
    use crate::config::AppPaths;
    use crate::skill::{SkillScope, SkillSource};

    const CANONICAL: &[u8] = b"---\nname: tdd\ndescription: canonical\n---\ncanonical body\n";
    const TARGET: &[u8] = b"---\nname: tdd\ndescription: user edit\n---\nuser edited body\n";

    struct Rig {
        #[allow(dead_code)] // keeps the temp dir alive for the whole test
        tmp: tempfile::TempDir,
        env: EnvContext,
        paths: AppPaths,
        config: Config,
        canonical_root: PathBuf,
        tool_dir: PathBuf,
    }

    fn rig(copy_method: bool) -> Rig {
        let tmp = tempfile::tempdir().unwrap();
        let mut env = EnvContext::with_home(tmp.path().join("home"));
        env.env.insert("PATH".into(), String::new());
        let paths = AppPaths {
            home: tmp.path().join("sync-home"),
        };
        let canonical_root = env.home.join(".agents").join("skills");
        let tool_dir = env.home.join(".claude").join("skills");
        let mut config = Config {
            canonical_skill_root: canonical_root.to_string_lossy().into_owned(),
            ..Default::default()
        };
        if copy_method {
            config.sync_method = crate::config::SyncMethod::Copy;
        }
        std::fs::create_dir_all(&canonical_root).unwrap();
        std::fs::create_dir_all(&tool_dir).unwrap();
        Rig {
            tmp,
            env,
            paths,
            config,
            canonical_root,
            tool_dir,
        }
    }

    impl Rig {
        fn ctx(&self) -> SyncContext<'_> {
            SyncContext {
                env: &self.env,
                paths: &self.paths,
                config: &self.config,
            }
        }

        fn canonical(&self) -> Skill {
            crate::scan::inspect_as_skill(
                &self.env,
                &self.canonical_root.join("tdd"),
                SkillScope::Global,
                SkillSource::Canonical,
            )
            .unwrap()
        }

        fn conflicting_report(&self) -> ConflictReport {
            let canonical = self.canonical();
            ConflictReport {
                skill_id: "tdd".into(),
                skill_name: "tdd".into(),
                tool_id: "claude".into(),
                tool_display_name: "Claude Code".into(),
                canonical_path: canonical.root.clone(),
                canonical_display: canonical.root.display().to_string(),
                target_path: self.tool_dir.join("tdd"),
                target_display: self.tool_dir.join("tdd").display().to_string(),
                canonical_fingerprint: canonical.fingerprint.clone(),
                target_fingerprint: Some("stale".into()),
                ignored: false,
            }
        }
    }

    fn seeded(rig: &Rig) -> (Skill, ManagedRegistry) {
        write_tree(&rig.canonical_root.join("tdd"), CANONICAL);
        write_tree(&rig.tool_dir.join("tdd"), TARGET);
        (rig.canonical(), ManagedRegistry::default())
    }

    fn write_tree(root: &Path, skill_md: &[u8]) {
        std::fs::create_dir_all(root).unwrap();
        std::fs::write(root.join("SKILL.md"), skill_md).unwrap();
    }

    #[test]
    fn use_canonical_replaces_target_with_link_and_backup() {
        let rig = rig(false);
        let (canonical, mut registry) = seeded(&rig);
        let report = rig.conflicting_report();

        // Dry run: nothing changes.
        let outcome = resolve_conflict(
            &rig.ctx(),
            &report,
            &canonical,
            Resolution::UseCanonical,
            EffectiveMethod::Symlink,
            &mut registry,
            true,
        )
        .unwrap();
        assert!(outcome.dry_run);
        assert_eq!(
            std::fs::read(report.target_path.join("SKILL.md")).unwrap(),
            TARGET
        );

        let outcome = resolve_conflict(
            &rig.ctx(),
            &report,
            &canonical,
            Resolution::UseCanonical,
            EffectiveMethod::Symlink,
            &mut registry,
            false,
        )
        .unwrap();
        assert_eq!(outcome.backups.len(), 1);
        // The user's version is in the backup.
        let backup_md = std::fs::read(outcome.backups[0].join("SKILL.md")).unwrap();
        assert_eq!(backup_md, TARGET);
        // The target is now a symlink to the canonical skill.
        assert_eq!(
            std::fs::read_link(&report.target_path).unwrap(),
            canonical.root
        );
        // Metadata explains what happened (§31).
        let metadata = std::fs::read_to_string(outcome.backups[0].with_extension("json")).unwrap();
        assert!(metadata.contains("conflict"));
    }

    #[test]
    fn import_target_makes_canonical_match_and_installs_copy() {
        let rig = rig(true); // copy method (recorded in the registry)
        let (canonical, mut registry) = seeded(&rig);
        let report = rig.conflicting_report();

        let outcome = resolve_conflict(
            &rig.ctx(),
            &report,
            &canonical,
            Resolution::ImportTarget,
            EffectiveMethod::Copy,
            &mut registry,
            false,
        )
        .unwrap();
        assert_eq!(outcome.backups.len(), 2);
        // Canonical now holds the user's content.
        assert_eq!(
            std::fs::read(report.canonical_path.join("SKILL.md")).unwrap(),
            TARGET
        );
        // The old canonical content is backed up.
        assert_eq!(
            std::fs::read(outcome.backups[0].join("SKILL.md")).unwrap(),
            CANONICAL
        );
        // The target became a managed copy.
        assert!(registry.find_by_target(&report.target_path).is_some());
        assert_eq!(
            std::fs::read(report.target_path.join("SKILL.md")).unwrap(),
            TARGET
        );
    }

    #[test]
    fn keep_both_imports_under_a_fresh_name() {
        let rig = rig(false);
        let (canonical, mut registry) = seeded(&rig);
        let report = rig.conflicting_report();

        let outcome = resolve_conflict(
            &rig.ctx(),
            &report,
            &canonical,
            Resolution::KeepBoth,
            EffectiveMethod::Symlink,
            &mut registry,
            false,
        )
        .unwrap();
        assert!(!outcome.installed);
        assert!(outcome.backups.is_empty(), "nothing was replaced");
        // New canonical skill holds the user's content.
        let imported = rig.canonical_root.join("tdd-2");
        assert_eq!(std::fs::read(imported.join("SKILL.md")).unwrap(), TARGET);
        // Both originals untouched.
        assert_eq!(
            std::fs::read(report.canonical_path.join("SKILL.md")).unwrap(),
            CANONICAL
        );
        assert_eq!(
            std::fs::read(report.target_path.join("SKILL.md")).unwrap(),
            TARGET
        );
        let _ = GeminiAdapter; // silence unused in some cfgs
        let _ = ClaudeAdapter;
    }

    #[test]
    fn resolution_refuses_managed_targets() {
        let rig = rig(true);
        let (canonical, mut registry) = seeded(&rig);
        // Record the target as a managed copy: no longer a conflict case.
        registry.upsert(crate::managed::ManagedInstall {
            tool_id: "claude".into(),
            skill_id: "tdd".into(),
            target: rig.tool_dir.join("tdd"),
            fingerprint: "x".into(),
            installed_at: "t".into(),
        });
        let report = rig.conflicting_report();
        let err = resolve_conflict(
            &rig.ctx(),
            &report,
            &canonical,
            Resolution::UseCanonical,
            EffectiveMethod::Copy,
            &mut registry,
            false,
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::TargetConflict);
        // Target untouched.
        assert_eq!(
            std::fs::read(report.target_path.join("SKILL.md")).unwrap(),
            TARGET
        );
    }

    #[test]
    fn ignored_conflicts_are_rejected() {
        let rig = rig(false);
        let (canonical, mut registry) = seeded(&rig);
        let mut report = rig.conflicting_report();
        report.ignored = true;
        let err = resolve_conflict(
            &rig.ctx(),
            &report,
            &canonical,
            Resolution::UseCanonical,
            EffectiveMethod::Symlink,
            &mut registry,
            false,
        )
        .unwrap_err();
        assert_eq!(err.code, ErrorCode::TargetConflict);
        assert!(err.recoverable);
    }
}
