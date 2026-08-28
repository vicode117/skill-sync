//! One-way synchronization engine: canonical store → one tool (design doc
//! §7b Slice 3, prompt §13–§15, §58, §59).
//!
//! Model: plan → validate → execute → report. Every plan entry names the
//! exact filesystem action; `dry_run` executes nothing. Removal is only
//! ever applied to *managed* targets: symlinks that resolve into the
//! canonical store, or copies recorded in `managed.json` (§28). Unmanaged
//! content is reported (`Skip`/`Conflict`) and never modified (§30).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::adapter::{LocationKind, SymlinkSupport, ToolAdapter};
use crate::config::{AppPaths, Config, SyncMethod};
use crate::env::abbreviate_home;
use crate::env::EnvContext;
use crate::error::Result;
use crate::fingerprint::fingerprint_dir;
use crate::fsutil::{probe_symlink_capability, remove_dir_verified};
use crate::managed::{ManagedInstall, ManagedRegistry};
use crate::skill::Skill;
use crate::store;

/// How this sync run materializes installations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EffectiveMethod {
    Symlink,
    Copy,
}

/// One planned filesystem action for one skill × tool.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum PlanAction {
    /// Create a directory symlink into the canonical store.
    CreateLink { target: PathBuf, source: PathBuf },
    /// Copy the skill into the tool directory (copy method / fallback).
    CreateCopy { target: PathBuf, source: PathBuf },
    /// Managed copy drifted from canonical: back up, replace, re-record.
    UpdateCopy {
        target: PathBuf,
        source: PathBuf,
        backup_dir: PathBuf,
    },
    /// A dangling (or mis-pointing) managed symlink: recreate it.
    RepairLink { target: PathBuf, source: PathBuf },
    /// Nothing to do (already linked, already native, or up to date).
    NoChange { target: Option<PathBuf> },
    /// The tool reads the canonical store directly at this location.
    Native,
    /// A decision is required (unmanaged target, foreign link, conflict).
    Skip {
        target: Option<PathBuf>,
        reason: String,
    },
    /// Skill is disabled for this tool and nothing is installed.
    Disabled,
    /// Skill disabled for this tool: remove the managed symlink only.
    RemoveManagedLink { target: PathBuf },
    /// Skill disabled for this tool: back up and remove the managed copy.
    RemoveManagedCopy {
        target: PathBuf,
        backup_dir: PathBuf,
    },
}

impl PlanAction {
    pub fn kind_label(&self) -> &'static str {
        match self {
            PlanAction::CreateLink { .. } => "createLink",
            PlanAction::CreateCopy { .. } => "createCopy",
            PlanAction::UpdateCopy { .. } => "updateCopy",
            PlanAction::RepairLink { .. } => "repairLink",
            PlanAction::NoChange { .. } => "noChange",
            PlanAction::Native => "native",
            PlanAction::Skip { .. } => "skip",
            PlanAction::Disabled => "disabled",
            PlanAction::RemoveManagedLink { .. } => "removeManagedLink",
            PlanAction::RemoveManagedCopy { .. } => "removeManagedCopy",
        }
    }
}

/// One entry of a sync plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanEntry {
    pub skill_id: String,
    pub skill_name: String,
    pub action: PlanAction,
    /// `~`-abbreviated target for display.
    pub display_target: String,
    pub notes: Vec<String>,
}

impl PlanEntry {
    pub fn is_mutation(&self) -> bool {
        matches!(
            self.action,
            PlanAction::CreateLink { .. }
                | PlanAction::CreateCopy { .. }
                | PlanAction::UpdateCopy { .. }
                | PlanAction::RepairLink { .. }
                | PlanAction::RemoveManagedLink { .. }
                | PlanAction::RemoveManagedCopy { .. }
        )
    }
}

/// The complete plan for one tool (§59: plan, validate, then execute).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncPlan {
    pub tool_id: String,
    pub tool_display_name: String,
    pub method: EffectiveMethod,
    pub canonical_root: PathBuf,
    pub canonical_root_display: String,
    /// The tool directory installations are created in.
    pub target_dir: Option<PathBuf>,
    pub entries: Vec<PlanEntry>,
}

impl SyncPlan {
    pub fn mutation_count(&self) -> usize {
        self.entries.iter().filter(|e| e.is_mutation()).count()
    }
}

/// Result of executing one entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryOutcome {
    pub skill_id: String,
    pub action_kind: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backup_dir: Option<PathBuf>,
}

/// Report of a sync run (§59: report exactly what succeeded and failed).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncRunReport {
    pub tool_id: String,
    pub method: EffectiveMethod,
    pub dry_run: bool,
    pub succeeded: Vec<EntryOutcome>,
    pub failed: Vec<EntryOutcome>,
}

impl SyncRunReport {
    pub fn summary(&self) -> String {
        format!(
            "{} succeeded, {} failed{}",
            self.succeeded.len(),
            self.failed.len(),
            if self.dry_run {
                " (dry run — no changes)"
            } else {
                ""
            }
        )
    }
}

/// Inputs for planning/execution, taken from the facade.
pub struct SyncContext<'a> {
    pub env: &'a EnvContext,
    pub paths: &'a AppPaths,
    pub config: &'a Config,
}

impl<'a> SyncContext<'a> {
    /// Resolve the configured method with adapter knowledge and a platform
    /// capability probe (§13 Auto, §44 Windows-style fallback).
    pub fn effective_method(
        &self,
        adapter: &dyn ToolAdapter,
        notes: &mut Vec<String>,
    ) -> EffectiveMethod {
        let explicit = match self.config.sync_method {
            SyncMethod::Symlink => Some(EffectiveMethod::Symlink),
            SyncMethod::Copy => Some(EffectiveMethod::Copy),
            SyncMethod::Auto => None,
        };
        let mut chosen = match explicit {
            Some(m) => m,
            None => match adapter.symlink_support() {
                SymlinkSupport::Avoided => EffectiveMethod::Copy,
                _ => EffectiveMethod::Symlink,
            },
        };
        if chosen == EffectiveMethod::Symlink {
            if let Err(reason) = probe_symlink_capability() {
                notes.push(reason);
                chosen = EffectiveMethod::Copy;
            }
        }
        chosen
    }

    /// The tool directory to install into: an explicit override wins, else
    /// the first `Standard` location that is not the canonical store itself
    /// (a tool reading the canonical store natively has nothing to sync,
    /// design doc §14).
    fn target_dir(&self, adapter: &dyn ToolAdapter, canonical_root: &Path) -> Option<PathBuf> {
        let over = self.config.tool(adapter.id()).cloned().unwrap_or_default();
        let locations = adapter.global_skill_locations(self.env, &over);
        if let Some(loc) = locations.iter().find(|l| l.overridden) {
            return Some(loc.path.clone());
        }
        if let Some(loc) = locations
            .iter()
            .find(|l| l.kind == LocationKind::Standard && !is_same_dir(&l.path, canonical_root))
        {
            return Some(loc.path.clone());
        }
        locations.first().map(|l| l.path.clone())
    }
}

fn is_same_dir(a: &Path, b: &Path) -> bool {
    match (a.canonicalize(), b.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => a == b,
    }
}

/// Plan a sync of every canonical skill into `tool_id`.
pub fn plan_tool_sync(
    ctx: &SyncContext,
    adapter: &dyn ToolAdapter,
    canonical_skills: &[Skill],
) -> Result<SyncPlan> {
    // Resolve the store root once (macOS /var -> /private/var and similar
    // symlinked ancestors) so all path comparisons are consistent.
    let configured_root = ctx.config.canonical_root(ctx.env);
    let canonical_root = configured_root
        .canonicalize()
        .unwrap_or_else(|_| configured_root.clone());
    let display_root = configured_root;
    let mut method_notes = Vec::new();
    let method = ctx.effective_method(adapter, &mut method_notes);
    let target_dir = ctx.target_dir(adapter, &canonical_root);
    let registry = ManagedRegistry::load(ctx.env, ctx.paths);

    let mut entries = Vec::new();
    for skill in canonical_skills {
        let (entry_target, action) = match target_dir.as_deref() {
            None => (
                None,
                PlanAction::Skip {
                    target: None,
                    reason: "tool has no known skills directory".into(),
                },
            ),
            Some(dir) => {
                let target = dir.join(&skill.id);
                let action = if is_same_dir(dir, &canonical_root) {
                    // Even natively-shared tools honor enablement: there is
                    // simply nothing to install or remove.
                    if ctx.config.is_skill_tool_enabled(&skill.id, adapter.id()) {
                        PlanAction::Native
                    } else {
                        PlanAction::Disabled
                    }
                } else if !ctx.config.is_skill_tool_enabled(&skill.id, adapter.id()) {
                    plan_removal(
                        ctx,
                        skill,
                        adapter.id(),
                        &canonical_root,
                        &target,
                        &registry,
                    )
                } else {
                    plan_for_target(ctx, skill, &canonical_root, &target, method, &registry)
                };
                (Some(target.clone()), action)
            }
        };

        entries.push(PlanEntry {
            skill_id: skill.id.clone(),
            skill_name: skill.display_name.clone(),
            display_target: entry_target
                .as_deref()
                .map(|t| abbreviate_home(t, ctx.env))
                .unwrap_or_default(),
            notes: if entries.is_empty() {
                method_notes.clone()
            } else {
                Vec::new()
            },
            action,
        });
    }

    Ok(SyncPlan {
        tool_id: adapter.id().to_string(),
        tool_display_name: adapter.display_name().to_string(),
        method,
        canonical_root: canonical_root.clone(),
        canonical_root_display: abbreviate_home(&display_root, ctx.env),
        target_dir,
        entries,
    })
}

/// Classify the removal of a disabled skill's installation. Only managed
/// installations are removed; unmanaged content is reported, never touched
/// (§27, §30).
fn plan_removal(
    ctx: &SyncContext,
    skill: &Skill,
    tool_id: &str,
    canonical_root: &Path,
    target: &Path,
    registry: &ManagedRegistry,
) -> PlanAction {
    let meta = match std::fs::symlink_metadata(target) {
        Ok(m) => m,
        Err(_) => return PlanAction::Disabled, // nothing installed
    };

    if meta.file_type().is_symlink() {
        let managed = match target.canonicalize() {
            Ok(resolved) => resolved.starts_with(canonical_root),
            Err(_) => {
                let raw = std::fs::read_link(target).unwrap_or_default();
                raw == skill.root || raw.starts_with(canonical_root)
            }
        };
        if managed {
            return PlanAction::RemoveManagedLink {
                target: target.to_path_buf(),
            };
        }
        return PlanAction::Skip {
            target: Some(target.to_path_buf()),
            reason: "disabled, but the symlink is not managed by SkillSync".into(),
        };
    }

    if meta.is_dir() {
        if registry.find_by_target(target).is_some() {
            return PlanAction::RemoveManagedCopy {
                target: target.to_path_buf(),
                backup_dir: backup_dir_for_copy(ctx.paths, tool_id, &skill.id),
            };
        }
        return PlanAction::Skip {
            target: Some(target.to_path_buf()),
            reason: "disabled, but the target is an unmanaged directory (never deleted)".into(),
        };
    }

    PlanAction::Skip {
        target: Some(target.to_path_buf()),
        reason: "disabled, but the target is not a managed installation".into(),
    }
}

/// Classify one skill × target path. Read-only.
fn plan_for_target(
    ctx: &SyncContext,
    skill: &Skill,
    canonical_root: &Path,
    target: &Path,
    method: EffectiveMethod,
    registry: &ManagedRegistry,
) -> PlanAction {
    if !target.exists() {
        // A dangling symlink: `exists()` is false. Repair only managed ones
        // (raw target equals the canonical skill or lies inside the store).
        let dangling_link = std::fs::symlink_metadata(target)
            .ok()
            .is_some_and(|m| m.file_type().is_symlink());
        if dangling_link {
            let raw = std::fs::read_link(target).unwrap_or_default();
            if raw == skill.root || raw.starts_with(canonical_root) {
                return PlanAction::RepairLink {
                    target: target.to_path_buf(),
                    source: skill.root.clone(),
                };
            }
            return PlanAction::Skip {
                target: Some(target.to_path_buf()),
                reason: "dangling symlink that does not point into the canonical store".into(),
            };
        }
        return match method {
            EffectiveMethod::Symlink => PlanAction::CreateLink {
                target: target.to_path_buf(),
                source: skill.root.clone(),
            },
            EffectiveMethod::Copy => PlanAction::CreateCopy {
                target: target.to_path_buf(),
                source: skill.root.clone(),
            },
        };
    }

    let meta = match std::fs::symlink_metadata(target) {
        Ok(m) => m,
        Err(e) => {
            return PlanAction::Skip {
                target: Some(target.to_path_buf()),
                reason: format!("unreadable target: {e}"),
            }
        }
    };

    if meta.file_type().is_symlink() {
        let raw = std::fs::read_link(target).unwrap_or_default();
        return match target.canonicalize() {
            Ok(resolved) if resolved == canonical_root.join(&skill.id) => PlanAction::NoChange {
                target: Some(target.to_path_buf()),
            },
            Ok(resolved) if resolved.starts_with(canonical_root) => PlanAction::RepairLink {
                target: target.to_path_buf(),
                source: skill.root.clone(),
            },
            Ok(_) => PlanAction::Skip {
                target: Some(target.to_path_buf()),
                reason: "symlink points outside the canonical store (not managed by SkillSync)"
                    .into(),
            },
            // Unresolvable link: managed only when its raw target clearly
            // belongs to the canonical store.
            Err(_) if raw == skill.root || raw.starts_with(canonical_root) => {
                PlanAction::RepairLink {
                    target: target.to_path_buf(),
                    source: skill.root.clone(),
                }
            }
            Err(_) => PlanAction::Skip {
                target: Some(target.to_path_buf()),
                reason: "dangling symlink that does not point into the canonical store".into(),
            },
        };
    }

    if !meta.is_dir() {
        return PlanAction::Skip {
            target: Some(target.to_path_buf()),
            reason: "target path exists but is not a directory or symlink".into(),
        };
    }

    // Plain directory: only SkillSync-recorded copies may be updated (§28).
    match registry.find_by_target(target) {
        Some(record) => {
            let target_fp = fingerprint_dir(target).ok();
            if target_fp.as_deref() == Some(record.fingerprint.as_str())
                && target_fp.as_deref() == skill.fingerprint.as_deref()
            {
                PlanAction::NoChange {
                    target: Some(target.to_path_buf()),
                }
            } else {
                PlanAction::UpdateCopy {
                    target: target.to_path_buf(),
                    source: skill.root.clone(),
                    backup_dir: backup_dir_for_copy(ctx.paths, record.tool_id.as_str(), &skill.id),
                }
            }
        }
        None => {
            let target_fp = fingerprint_dir(target).ok();
            if target_fp.is_some() && target_fp == skill.fingerprint {
                PlanAction::Skip {
                    target: Some(target.to_path_buf()),
                    reason: "identical unmanaged copy; import it into the store to adopt".into(),
                }
            } else {
                PlanAction::Skip {
                    target: Some(target.to_path_buf()),
                    reason: "unmanaged directory with different content — conflict; resolve \
                             explicitly before syncing (never overwritten)"
                        .into(),
                }
            }
        }
    }
}

fn backup_dir_for_copy(paths: &AppPaths, tool_id: &str, skill_id: &str) -> PathBuf {
    store::backup_dir_for(paths, "sync", &format!("{tool_id}-{skill_id}"))
}

/// Execute a validated plan (§59). Never touches `Skip`/`NoChange`/`Native`
/// entries. Updates the managed-copy registry. Reports exactly what
/// succeeded and failed — nothing is left ambiguous.
pub fn execute_sync(ctx: &SyncContext, plan: &SyncPlan, dry_run: bool) -> Result<SyncRunReport> {
    let mut registry = ManagedRegistry::load(ctx.env, ctx.paths);
    let mut registry_dirty = false;
    let mut succeeded = Vec::new();
    let mut failed = Vec::new();

    for entry in &plan.entries {
        let outcome = |ok: bool, error: Option<String>, backup: Option<PathBuf>| EntryOutcome {
            skill_id: entry.skill_id.clone(),
            action_kind: entry.action.kind_label().to_string(),
            ok,
            error,
            backup_dir: backup,
        };

        match &entry.action {
            PlanAction::NoChange { .. }
            | PlanAction::Native
            | PlanAction::Skip { .. }
            | PlanAction::Disabled => continue,
            PlanAction::RemoveManagedLink { target } => {
                if dry_run {
                    succeeded.push(outcome(true, None, None));
                    continue;
                }
                // Re-verify ownership at execution time: the symlink must
                // resolve into the canonical store before it is removed.
                let canonical_root = &plan.canonical_root;
                let owned = std::fs::symlink_metadata(target)
                    .map(|m| m.file_type().is_symlink())
                    .unwrap_or(false)
                    && (target
                        .canonicalize()
                        .map(|r| r.starts_with(canonical_root))
                        .unwrap_or(false)
                        || std::fs::read_link(target)
                            .map(|raw| raw.starts_with(canonical_root))
                            .unwrap_or(false));
                if !owned {
                    failed.push(outcome(
                        false,
                        Some("link no longer resolves into the canonical store; refusing".into()),
                        None,
                    ));
                    continue;
                }
                match std::fs::remove_file(target) {
                    Ok(()) => succeeded.push(outcome(true, None, None)),
                    Err(e) => failed.push(outcome(false, Some(e.to_string()), None)),
                }
            }
            PlanAction::RemoveManagedCopy { target, backup_dir } => {
                if dry_run {
                    succeeded.push(outcome(true, None, Some(backup_dir.clone())));
                    continue;
                }
                if registry.find_by_target(target).is_none() {
                    failed.push(outcome(
                        false,
                        Some("managed record missing; refusing to remove".into()),
                        None,
                    ));
                    continue;
                }
                let backup = match store::backup_skill_dir(
                    target,
                    backup_dir,
                    &entry.skill_id,
                    "disable",
                    plan.tool_id.as_str(),
                    ctx.env,
                ) {
                    Ok(()) => backup_dir.clone(),
                    Err(e) => {
                        failed.push(outcome(false, Some(e.message), None));
                        continue;
                    }
                };
                match remove_dir_verified(target, "managed copy (disabled)") {
                    Ok(()) => {
                        registry.remove_by_target(target);
                        registry_dirty = true;
                        succeeded.push(outcome(true, None, Some(backup)));
                    }
                    Err(e) => failed.push(outcome(false, Some(e.message), Some(backup))),
                }
            }
            PlanAction::CreateLink { target, source } => {
                if dry_run {
                    succeeded.push(outcome(true, None, None));
                    continue;
                }
                if let Err(e) = ensure_target_parent(target) {
                    failed.push(outcome(false, Some(e), None));
                    continue;
                }
                match make_symlink(source, target) {
                    Ok(()) => succeeded.push(outcome(true, None, None)),
                    Err(e) => failed.push(outcome(false, Some(e), None)),
                }
            }
            PlanAction::RepairLink { target, source } => {
                if dry_run {
                    succeeded.push(outcome(true, None, None));
                    continue;
                }
                // Removing the link only: a symlink is never user data.
                match std::fs::remove_file(target) {
                    Ok(()) => match make_symlink(source, target) {
                        Ok(()) => succeeded.push(outcome(true, None, None)),
                        Err(e) => failed.push(outcome(false, Some(e), None)),
                    },
                    Err(e) => failed.push(outcome(false, Some(e.to_string()), None)),
                }
            }
            PlanAction::CreateCopy { target, source } => {
                if dry_run {
                    succeeded.push(outcome(true, None, None));
                    continue;
                }
                match install_copy(plan, source, target, &entry.skill_id, &mut registry) {
                    Ok(()) => {
                        registry_dirty = true;
                        succeeded.push(outcome(true, None, None));
                    }
                    Err(e) => failed.push(outcome(false, Some(e.message), None)),
                }
            }
            PlanAction::UpdateCopy {
                target,
                source,
                backup_dir,
            } => {
                if dry_run {
                    succeeded.push(outcome(true, None, Some(backup_dir.clone())));
                    continue;
                }
                // Ownership: recorded in the registry — otherwise refuse.
                if registry.find_by_target(target).is_none() {
                    failed.push(outcome(
                        false,
                        Some("managed record disappeared; refusing to remove".into()),
                        None,
                    ));
                    continue;
                }
                // Back up the drifted managed copy before replacing (§31).
                let backup = match store::backup_skill_dir(
                    target,
                    backup_dir,
                    &entry.skill_id,
                    "sync",
                    plan.tool_id.as_str(),
                    ctx.env,
                ) {
                    Ok(()) => backup_dir.clone(),
                    Err(e) => {
                        failed.push(outcome(false, Some(e.message), None));
                        continue;
                    }
                };
                match remove_dir_verified(target, "managed copy") {
                    Ok(()) => {
                        match install_copy(plan, source, target, &entry.skill_id, &mut registry) {
                            Ok(()) => {
                                registry_dirty = true;
                                succeeded.push(outcome(true, None, Some(backup)));
                            }
                            Err(e) => failed.push(outcome(false, Some(e.message), Some(backup))),
                        }
                    }
                    Err(e) => failed.push(outcome(false, Some(e.message), Some(backup))),
                }
            }
        }
    }

    if registry_dirty && !dry_run {
        registry.save(ctx.paths)?;
    }

    Ok(SyncRunReport {
        tool_id: plan.tool_id.clone(),
        method: plan.method,
        dry_run,
        succeeded,
        failed,
    })
}

fn make_symlink(source: &Path, target: &Path) -> std::result::Result<(), String> {
    #[cfg(unix)]
    let result = std::os::unix::fs::symlink(source, target);
    #[cfg(windows)]
    let result = std::os::windows::fs::symlink_dir(source, target);
    #[cfg(not(any(unix, windows)))]
    let result: std::io::Result<()> = Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "symlinks unsupported",
    ));
    result.map_err(|e| e.to_string())
}

/// Copy a skill into place and record it as managed (§28).
fn install_copy(
    plan: &SyncPlan,
    source: &Path,
    target: &Path,
    skill_id: &str,
    registry: &mut ManagedRegistry,
) -> Result<()> {
    store::copy_skill_dir(source, target)?;
    let fingerprint = fingerprint_dir(target).unwrap_or_default();
    registry.upsert(ManagedInstall {
        tool_id: plan.tool_id.clone(),
        skill_id: skill_id.to_string(),
        target: target.to_path_buf(),
        fingerprint,
        installed_at: store::iso_utc_now(),
    });
    Ok(())
}

fn ensure_target_parent(target: &Path) -> std::result::Result<(), String> {
    if let Some(parent) = target.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("could not create tool skills directory: {e}"))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapter::claude::ClaudeAdapter;
    use crate::adapter::gemini::GeminiAdapter;
    use crate::skill::{SkillScope, SkillSource};

    fn write_skill(root: &Path, name: &str, body: &str) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        std::fs::write(dir.join("SKILL.md"), body).unwrap();
        std::fs::write(dir.join("scripts/run.sh"), "echo hi\n").unwrap();
        dir
    }

    const V1: &str = "---\nname: tdd\ndescription: v1\n---\n# v1\n";
    const V2: &str = "---\nname: tdd\ndescription: v2\n---\n# v2\n";

    struct Sandbox {
        tmp: tempfile::TempDir,
        env: EnvContext,
        paths: AppPaths,
        config: Config,
        canonical: PathBuf,
    }

    fn sandbox(method: SyncMethod) -> Sandbox {
        let tmp = tempfile::tempdir().unwrap();
        let mut env = EnvContext::with_home(tmp.path().join("home"));
        env.env.insert("PATH".into(), String::new());
        let paths = AppPaths {
            home: tmp.path().join("sync-home"),
        };
        let canonical = env.home.join(".agents").join("skills");
        let config = Config {
            sync_method: method,
            canonical_skill_root: canonical.to_string_lossy().into_owned(),
            ..Default::default()
        };
        Sandbox {
            tmp,
            env,
            paths,
            config,
            canonical,
        }
    }

    impl Sandbox {
        fn ctx(&self) -> SyncContext<'_> {
            SyncContext {
                env: &self.env,
                paths: &self.paths,
                config: &self.config,
            }
        }

        fn canonical_skill(&self, name: &str, body: &str) -> Skill {
            let dir = write_skill(&self.canonical, name, body);
            crate::scan::inspect_as_skill(
                &self.env,
                &dir,
                SkillScope::Global,
                SkillSource::Canonical,
            )
            .unwrap()
        }
    }

    #[test]
    fn symlink_install_then_no_change() {
        let sb = sandbox(SyncMethod::Auto);
        std::fs::create_dir_all(&sb.canonical).unwrap();
        let skill = sb.canonical_skill("tdd", V1);

        let plan = plan_tool_sync(&sb.ctx(), &ClaudeAdapter, std::slice::from_ref(&skill)).unwrap();
        assert_eq!(plan.method, EffectiveMethod::Symlink);
        let target = sb.env.home.join(".claude").join("skills").join("tdd");
        assert_eq!(
            plan.entries[0].action,
            PlanAction::CreateLink {
                target: target.clone(),
                source: skill.root.clone()
            }
        );

        let report = execute_sync(&sb.ctx(), &plan, false).unwrap();
        assert_eq!(report.succeeded.len(), 1, "{report:?}");
        assert_eq!(
            std::fs::read_link(&target).unwrap(),
            skill.root,
            "link points at the canonical skill"
        );

        // Second plan: nothing to do.
        let plan2 = plan_tool_sync(&sb.ctx(), &ClaudeAdapter, &[skill]).unwrap();
        assert!(matches!(
            plan2.entries[0].action,
            PlanAction::NoChange { .. }
        ));
    }

    #[test]
    fn dry_run_creates_nothing() {
        let sb = sandbox(SyncMethod::Auto);
        std::fs::create_dir_all(&sb.canonical).unwrap();
        let skill = sb.canonical_skill("tdd", V1);

        let plan = plan_tool_sync(&sb.ctx(), &ClaudeAdapter, &[skill]).unwrap();
        let report = execute_sync(&sb.ctx(), &plan, true).unwrap();
        assert!(report.dry_run);
        assert_eq!(report.succeeded.len(), 1);
        assert!(
            !sb.env
                .home
                .join(".claude")
                .join("skills")
                .join("tdd")
                .exists(),
            "dry run must not create the link"
        );
    }

    #[test]
    fn copy_install_records_ownership_and_updates_with_backup() {
        let sb = sandbox(SyncMethod::Copy);
        std::fs::create_dir_all(&sb.canonical).unwrap();
        let skill = sb.canonical_skill("tdd", V1);

        // Gemini avoids symlinks: even Auto must choose Copy (§45 adapter
        // knowledge driving the method).
        let plan = plan_tool_sync(&sb.ctx(), &GeminiAdapter, std::slice::from_ref(&skill)).unwrap();
        assert_eq!(plan.method, EffectiveMethod::Copy);
        let target = sb.env.home.join(".gemini").join("skills").join("tdd");
        assert!(matches!(
            plan.entries[0].action,
            PlanAction::CreateCopy { .. }
        ));
        execute_sync(&sb.ctx(), &plan, false).unwrap();
        assert_eq!(
            std::fs::read(target.join("SKILL.md")).unwrap(),
            V1.as_bytes()
        );

        // Recorded as managed (§28)
        let registry = ManagedRegistry::load(&sb.env, &sb.paths);
        assert!(registry.find_by_target(&target).is_some());

        // Up to date: no change.
        let plan2 =
            plan_tool_sync(&sb.ctx(), &GeminiAdapter, std::slice::from_ref(&skill)).unwrap();
        assert!(matches!(
            plan2.entries[0].action,
            PlanAction::NoChange { .. }
        ));

        // Canonical drifts → UpdateCopy, executed with a backup (§31).
        std::fs::write(skill.root.join("SKILL.md"), V2).unwrap();
        let skill_v2 = crate::scan::inspect_as_skill(
            &sb.env,
            &skill.root,
            SkillScope::Global,
            SkillSource::Canonical,
        )
        .unwrap();
        let plan3 = plan_tool_sync(&sb.ctx(), &GeminiAdapter, &[skill_v2]).unwrap();
        match &plan3.entries[0].action {
            PlanAction::UpdateCopy { backup_dir, .. } => {
                assert!(backup_dir.to_string_lossy().contains("sync-gemini-tdd"));
            }
            other => panic!("expected updateCopy, got {other:?}"),
        }
        let report = execute_sync(&sb.ctx(), &plan3, false).unwrap();
        assert_eq!(report.succeeded.len(), 1);
        assert_eq!(
            std::fs::read(target.join("SKILL.md")).unwrap(),
            V2.as_bytes()
        );
        // Old content preserved in the backup
        let backups: Vec<_> = std::fs::read_dir(sb.paths.backups_dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_dir())
            .collect();
        assert_eq!(backups.len(), 1);
        assert_eq!(
            std::fs::read(backups[0].path().join("SKILL.md")).unwrap(),
            V1.as_bytes()
        );
    }

    #[test]
    fn unmanaged_conflicting_target_is_skipped_never_overwritten() {
        let sb = sandbox(SyncMethod::Auto);
        std::fs::create_dir_all(&sb.canonical).unwrap();
        let skill = sb.canonical_skill("tdd", V2);
        let target = sb.env.home.join(".claude").join("skills").join("tdd");
        write_skill(target.parent().unwrap(), "tdd", V1);

        let plan = plan_tool_sync(&sb.ctx(), &ClaudeAdapter, &[skill]).unwrap();
        match &plan.entries[0].action {
            PlanAction::Skip { reason, .. } => {
                assert!(reason.contains("conflict"), "{reason}");
            }
            other => panic!("expected skip, got {other:?}"),
        }
        execute_sync(&sb.ctx(), &plan, false).unwrap();
        // User content untouched
        assert_eq!(
            std::fs::read(target.join("SKILL.md")).unwrap(),
            V1.as_bytes()
        );
    }

    #[test]
    fn identical_unmanaged_copy_is_reported_not_duplicated() {
        let sb = sandbox(SyncMethod::Copy);
        std::fs::create_dir_all(&sb.canonical).unwrap();
        let skill = sb.canonical_skill("tdd", V1);
        let target_dir = sb.env.home.join(".gemini").join("skills");
        let target = write_skill(&target_dir, "tdd", V1);

        let plan = plan_tool_sync(&sb.ctx(), &GeminiAdapter, &[skill]).unwrap();
        match &plan.entries[0].action {
            PlanAction::Skip { reason, .. } => assert!(reason.contains("identical")),
            other => panic!("expected skip, got {other:?}"),
        }
        execute_sync(&sb.ctx(), &plan, false).unwrap();
        // Still a plain directory (not linked, not re-copied, not recorded)
        let registry = ManagedRegistry::load(&sb.env, &sb.paths);
        assert!(registry.find_by_target(&target).is_none());
    }

    #[test]
    fn foreign_symlink_is_left_alone() {
        let sb = sandbox(SyncMethod::Auto);
        std::fs::create_dir_all(&sb.canonical).unwrap();
        let skill = sb.canonical_skill("tdd", V1);
        let elsewhere = write_skill(sb.tmp.path(), "elsewhere-tdd", V1);
        let tool_dir = sb.env.home.join(".claude").join("skills");
        std::fs::create_dir_all(&tool_dir).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&elsewhere, tool_dir.join("tdd")).unwrap();

        let plan = plan_tool_sync(&sb.ctx(), &ClaudeAdapter, &[skill]).unwrap();
        match &plan.entries[0].action {
            PlanAction::Skip { reason, .. } => {
                assert!(reason.contains("outside the canonical store"));
            }
            other => panic!("expected skip, got {other:?}"),
        }
        execute_sync(&sb.ctx(), &plan, false).unwrap();
        assert_eq!(
            std::fs::read_link(tool_dir.join("tdd")).unwrap(),
            elsewhere,
            "foreign link must survive"
        );
    }

    #[test]
    fn dangling_managed_link_is_repaired() {
        let sb = sandbox(SyncMethod::Auto);
        std::fs::create_dir_all(&sb.canonical).unwrap();
        let tool_dir = sb.env.home.join(".claude").join("skills");
        std::fs::create_dir_all(&tool_dir).unwrap();

        // A managed link whose canonical skill vanished (e.g. interrupted
        // store operation): raw target points into the store, target gone.
        let missing_root = sb.canonical.join("tdd");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&missing_root, tool_dir.join("tdd")).unwrap();

        let skill = Skill {
            id: "tdd".into(),
            display_name: "tdd".into(),
            description: None,
            root: missing_root.clone(),
            scope: SkillScope::Global,
            source: SkillSource::Canonical,
            files: vec![],
            fingerprint: None,
            frontmatter: None,
            validation: vec![],
        };

        let plan = plan_tool_sync(&sb.ctx(), &ClaudeAdapter, &[skill]).unwrap();
        assert!(matches!(
            plan.entries[0].action,
            PlanAction::RepairLink { .. }
        ));
        let report = execute_sync(&sb.ctx(), &plan, false).unwrap();
        assert_eq!(report.succeeded.len(), 1);
        assert_eq!(
            std::fs::read_link(tool_dir.join("tdd")).unwrap(),
            missing_root,
            "link recreated, pointing into the store"
        );
    }

    #[test]
    fn dangling_foreign_link_is_skipped() {
        let sb = sandbox(SyncMethod::Auto);
        std::fs::create_dir_all(&sb.canonical).unwrap();
        let skill = sb.canonical_skill("tdd", V1);
        let tool_dir = sb.env.home.join(".claude").join("skills");
        std::fs::create_dir_all(&tool_dir).unwrap();
        let gone = sb.tmp.path().join("vanished-skill");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&gone, tool_dir.join("tdd")).unwrap();

        let plan = plan_tool_sync(&sb.ctx(), &ClaudeAdapter, &[skill]).unwrap();
        match &plan.entries[0].action {
            PlanAction::Skip { reason, .. } => {
                assert!(reason.contains("dangling"), "{reason}");
            }
            other => panic!("expected skip, got {other:?}"),
        }
        // Untouched
        assert_eq!(std::fs::read_link(tool_dir.join("tdd")).unwrap(), gone);
    }

    #[test]
    fn native_location_produces_no_mutations() {
        let mut sb = sandbox(SyncMethod::Auto);
        std::fs::create_dir_all(&sb.canonical).unwrap();
        let skill = sb.canonical_skill("tdd", V1);

        // Override the tool's location to the canonical store itself: the
        // tool reads it natively (§14) — nothing to install.
        sb.config.tools.insert(
            "claude".into(),
            crate::config::ToolOverride {
                global_skill_path: Some(sb.canonical.to_string_lossy().into_owned()),
                ..Default::default()
            },
        );

        let plan = plan_tool_sync(&sb.ctx(), &ClaudeAdapter, &[skill]).unwrap();
        assert!(matches!(plan.entries[0].action, PlanAction::Native));
        assert_eq!(plan.mutation_count(), 0);
        execute_sync(&sb.ctx(), &plan, false).unwrap();
    }

    #[test]
    fn missing_canonical_store_yields_empty_plan() {
        let sb = sandbox(SyncMethod::Auto);
        // canonical root never created
        let plan = plan_tool_sync(&sb.ctx(), &ClaudeAdapter, &[]).unwrap();
        assert!(plan.entries.is_empty());
        let report = execute_sync(&sb.ctx(), &plan, false).unwrap();
        assert_eq!(report.summary(), "0 succeeded, 0 failed");
    }
}

#[cfg(test)]
mod enablement_tests {
    use super::*;
    use crate::adapter::claude::ClaudeAdapter;
    use crate::config::ToolOverride;
    use crate::error::ErrorCode;
    use crate::skill::{SkillScope, SkillSource};

    const V1: &str = "---\nname: tdd\ndescription: v1\n---\n# v1\n";

    fn write_skill(root: &Path, name: &str, body: &str) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), body).unwrap();
        dir
    }

    /// Full sandbox mirroring the facade (needed for config persistence).
    fn facade() -> (tempfile::TempDir, crate::SkillSync) {
        let tmp = tempfile::tempdir().unwrap();
        let mut env = EnvContext::with_home(tmp.path().join("home"));
        env.env.insert("PATH".into(), String::new());
        let app = crate::SkillSync::with_environment(env);
        (tmp, app)
    }

    #[test]
    fn disabled_skill_without_installation_plans_disabled() {
        let tmp = tempfile::tempdir().unwrap();
        let mut env = EnvContext::with_home(tmp.path().join("home"));
        env.env.insert("PATH".into(), String::new());
        let paths = AppPaths {
            home: tmp.path().join("sync-home"),
        };
        let canonical = env.home.join(".agents").join("skills");
        std::fs::create_dir_all(&canonical).unwrap();
        write_skill(&canonical, "tdd", V1);
        let mut config = Config {
            canonical_skill_root: canonical.to_string_lossy().into_owned(),
            ..Default::default()
        };
        config.set_skill_tool_enabled("tdd", "claude", false);
        let ctx = SyncContext {
            env: &env,
            paths: &paths,
            config: &config,
        };
        let skill = crate::scan::inspect_as_skill(
            &env,
            &canonical.join("tdd"),
            SkillScope::Global,
            SkillSource::Canonical,
        )
        .unwrap();

        let plan = plan_tool_sync(&ctx, &ClaudeAdapter, &[skill]).unwrap();
        assert!(matches!(plan.entries[0].action, PlanAction::Disabled));
        assert_eq!(plan.mutation_count(), 0);
    }

    #[test]
    fn disabling_removes_only_the_managed_link() {
        let (_tmp, mut app) = facade();
        let home = app.env().home.clone();
        let canonical = home.join(".agents").join("skills");
        write_skill(&canonical, "tdd", V1);
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        app.save_config(Config::default()).unwrap();

        // Install via sync (Slice 3 behavior).
        let report = app.sync_tool("claude", false).unwrap();
        assert_eq!(report.succeeded.len(), 1);
        let link = home.join(".claude").join("skills").join("tdd");
        assert!(link.exists());

        // Disable: the managed link is removed, canonical stays.
        let report = app
            .set_skill_tool_enabled("tdd", "claude", false, false)
            .unwrap();
        assert_eq!(report.succeeded.len(), 1);
        assert_eq!(report.succeeded[0].action_kind, "removeManagedLink");
        assert!(!link.exists());
        assert!(canonical.join("tdd").join("SKILL.md").is_file());

        // Config records the choice.
        assert!(!app.config().is_skill_tool_enabled("tdd", "claude"));
        // Overview shows the disabled state.
        let overview = app.overview().unwrap();
        let row = &overview.rows[0];
        let installation = row
            .installations
            .iter()
            .find(|i| i.tool_id == "claude")
            .unwrap();
        assert_eq!(installation.state, crate::SyncState::Disabled);

        // Re-enable: installs again.
        let report = app
            .set_skill_tool_enabled("tdd", "claude", true, false)
            .unwrap();
        assert_eq!(report.succeeded[0].action_kind, "createLink");
        assert!(link.exists());
    }

    #[test]
    fn disabling_never_touches_unmanaged_targets() {
        let (_tmp, mut app) = facade();
        let home = app.env().home.clone();
        let canonical = home.join(".agents").join("skills");
        write_skill(&canonical, "tdd", V1);
        // User already has their own copy in the tool dir.
        let tool_dir = home.join(".claude").join("skills");
        write_skill(
            &tool_dir,
            "tdd",
            "---\nname: tdd\ndescription: mine\n---\nmine",
        );
        std::fs::create_dir_all(home.join(".claude")).unwrap();
        app.save_config(Config::default()).unwrap();

        let report = app
            .set_skill_tool_enabled("tdd", "claude", false, false)
            .unwrap();
        // A skip is reported in the plan, not executed: nothing changed.
        assert!(report.succeeded.is_empty() && report.failed.is_empty());
        // User content untouched.
        assert!(tool_dir.join("tdd").join("SKILL.md").is_file());
    }

    #[test]
    fn enablement_rejects_unknown_skill_and_tool() {
        let (_tmp, mut app) = facade();
        let canonical = app.env().home.join(".agents").join("skills");
        write_skill(&canonical, "tdd", V1);
        app.save_config(Config::default()).unwrap();

        let err = app
            .set_skill_tool_enabled("nope", "claude", true, false)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidSkill);
        let err = app
            .set_skill_tool_enabled("tdd", "not-a-tool", true, false)
            .unwrap_err();
        assert_eq!(err.code, ErrorCode::ToolNotFound);
    }

    #[test]
    fn config_records_and_persists_enablement() {
        let (_tmp, mut app) = facade();
        app.save_config(Config::default()).unwrap();
        assert!(app.config().is_skill_tool_enabled("x", "claude"));
        let mut config = app.config().clone();
        config.set_skill_tool_enabled("x", "claude", false);
        app.save_config(config).unwrap();
        assert!(!app.config().is_skill_tool_enabled("x", "claude"));
        let reloaded = crate::config::load_config(app.paths()).unwrap();
        assert!(!reloaded.is_skill_tool_enabled("x", "claude"));
        // Other combos keep the enabled default.
        assert!(reloaded.is_skill_tool_enabled("x", "codex"));
        let _ = ToolOverride::default();
    }
}
