//! SkillSync core — the single implementation of all business logic, shared
//! by the Tauri GUI and the CLI (design doc §41).
//!
//! Slice 1 scope: read-only discovery. Nothing in this crate modifies user
//! skill files; the only writes are SkillSync-owned config under
//! `~/.skillsync/`.

pub mod adapter;
pub mod config;
pub mod conflict;
pub mod doctor;
pub mod env;
pub mod error;
pub mod fingerprint;
pub mod firstimport;
pub mod frontmatter;
pub mod fsutil;
pub mod git;
pub mod managed;
pub mod overview;
pub mod scan;
pub mod skill;
pub mod store;
pub mod sync;
pub mod watcher;

use std::sync::Arc;

pub use config::{AppPaths, Config, SyncMethod, ToolOverride};
pub use conflict::{ConflictReport, DiffEntry, DiffKind, Resolution, ResolutionReport};
pub use doctor::{CheckStatus, DoctorCheck, DoctorReport};
pub use env::EnvContext;
pub use error::{ErrorCode, Result, SkillSyncError};
pub use firstimport::{FirstImportPlan, FirstImportReport, ImportConflict, PlannedImport};
pub use git::{ChangedSkill, GitStatus, SkillChange};
pub use overview::{Installation, LocationInfo, SkillOverview, SkillRow, SyncState, ToolInfo};
pub use scan::{
    managedness_label as scan_managedness_label, InstallKind, Managedness, ScannedSkill,
};
pub use skill::{
    Skill, SkillFileEntry, SkillFrontmatter, SkillScope, SkillSource, ValidationIssue,
    ValidationSeverity,
};
pub use store::{
    adopt_canonical_root as adopt_canonical_root_op, ConflictResolution as ImportResolution,
    ImportAction, ImportOutcome, ImportPlan,
};
pub use sync::{EffectiveMethod, EntryOutcome, PlanAction, PlanEntry, SyncPlan, SyncRunReport};
pub use watcher::{AutoSyncHandle, AutoSyncSink, NullSink};

/// Facade over the environment, config and adapters. Construct once per
/// process (CLI run, GUI session) and call the read-only operations.
pub struct SkillSync {
    env: EnvContext,
    paths: AppPaths,
    config: Config,
    adapters: Vec<Arc<dyn adapter::ToolAdapter>>,
}

impl SkillSync {
    /// Discover the real environment and load config (defaults when
    /// missing). Config parse errors are surfaced, not swallowed.
    pub fn discover() -> Result<Self> {
        let env = EnvContext::discover()?;
        let paths = AppPaths::discover(&env);
        let config = config::load_config(&paths)?;
        Ok(Self {
            env,
            paths,
            config,
            adapters: adapter::registry(),
        })
    }

    /// Build against a synthetic home/environment (tests, sandboxed tools).
    pub fn with_environment(env: EnvContext) -> Self {
        let paths = AppPaths::discover(&env);
        let config = config::load_config(&paths).unwrap_or_default();
        Self {
            env,
            paths,
            config,
            adapters: adapter::registry(),
        }
    }

    pub fn env(&self) -> &EnvContext {
        &self.env
    }

    pub fn paths(&self) -> &AppPaths {
        &self.paths
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn adapters(&self) -> &[Arc<dyn adapter::ToolAdapter>] {
        &self.adapters
    }

    /// Replace and persist the configuration (SkillSync-owned state).
    pub fn save_config(&mut self, config: Config) -> Result<()> {
        config::save_config(&self.paths, &config)?;
        self.config = config;
        Ok(())
    }

    /// Canonical skills currently present in the canonical store. Missing
    /// store yields an empty list (never creates it).
    pub fn canonical_skills(&self) -> Result<Vec<Skill>> {
        let root = self.config.canonical_root(&self.env);
        if !root.is_dir() {
            return Ok(Vec::new());
        }
        let mut skills = Vec::new();
        let entries = std::fs::read_dir(&root).map_err(|e| SkillSyncError::io(&e, &root))?;
        for entry in entries {
            let entry = entry.map_err(|e| SkillSyncError::io(&e, &root))?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || !path.is_dir() {
                continue;
            }
            if !path.join("SKILL.md").is_file() {
                continue; // not a skill directory
            }
            skills.push(scan::inspect_as_skill(
                &self.env,
                &path,
                SkillScope::Global,
                SkillSource::Canonical,
            )?);
        }
        skills.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(skills)
    }

    /// Scan every enabled adapter's skill locations. Undetected tools are
    /// skipped unless the user configured an explicit path override.
    pub fn scan_all(&self) -> Result<Vec<ScannedSkill>> {
        let canonical_root = self.config.canonical_root(&self.env);
        let mut all = Vec::new();
        for adapter in &self.adapters {
            if !self.config.is_tool_enabled(adapter.id()) {
                continue;
            }
            let over = self.config.tool(adapter.id()).cloned().unwrap_or_default();
            let has_override = over.global_skill_path.is_some();
            if !adapter.detect(&self.env).installed && !has_override {
                continue;
            }
            all.extend(adapter.scan_skills(&self.env, &over, &canonical_root)?);
        }
        Ok(all)
    }

    /// The full read-only overview consumed by GUI and CLI.
    pub fn overview(&self) -> Result<SkillOverview> {
        let canonical_root = self.config.canonical_root(&self.env);
        let canonical_skills = self.canonical_skills()?;
        let scanned = self.scan_all()?;

        let builder = overview::OverviewBuilder {
            env: &self.env,
            config: &self.config,
            canonical_root: &canonical_root,
        };
        let tools: Vec<ToolInfo> = self
            .adapters
            .iter()
            .map(|a| {
                let mine: Vec<ScannedSkill> = scanned
                    .iter()
                    .filter(|s| s.tool_id == a.id())
                    .cloned()
                    .collect();
                builder.tool_info(a.as_ref(), &mine)
            })
            .collect();
        let rows = builder.build_rows(&canonical_skills, &scanned, &tools);

        Ok(SkillOverview {
            canonical_root: canonical_root.clone(),
            canonical_root_display: env::abbreviate_home(&canonical_root, &self.env),
            canonical_root_exists: canonical_root.is_dir(),
            tools,
            rows,
        })
    }

    /// Diagnostics (shared by `skillsync doctor` and the GUI).
    pub fn doctor(&self) -> DoctorReport {
        doctor::run_doctor(&self.env, &self.paths, &self.config, &self.adapters)
    }

    /// Create the canonical root folder if missing (explicit user action).
    pub fn adopt_canonical_root(&self) -> Result<std::path::PathBuf> {
        let root = self.config.canonical_root(&self.env);
        store::adopt_canonical_root(&self.env, &root)
    }

    /// Plan an import from a skill directory into the canonical store.
    pub fn plan_import(
        &self,
        source: &std::path::Path,
        resolution: store::ConflictResolution,
    ) -> Result<store::ImportPlan> {
        let root = self.config.canonical_root(&self.env);
        store::plan_import(&self.env, &self.paths, source, &root, resolution)
    }

    /// Execute a previously computed import plan (dry-run capable).
    pub fn execute_import(
        &self,
        plan: &store::ImportPlan,
        dry_run: bool,
    ) -> Result<store::ImportOutcome> {
        store::execute_import(&self.env, plan, dry_run)
    }

    fn sync_context(&self) -> sync::SyncContext<'_> {
        sync::SyncContext {
            env: &self.env,
            paths: &self.paths,
            config: &self.config,
        }
    }

    /// Plan a one-way sync of all canonical skills into one tool (§75
    /// Slice 3). Read-only; preview freely.
    pub fn plan_sync(&self, tool_id: &str) -> Result<SyncPlan> {
        let adapter = self
            .adapters
            .iter()
            .find(|a| a.id() == tool_id)
            .ok_or_else(|| {
                SkillSyncError::new(
                    ErrorCode::ToolNotFound,
                    format!("no adapter for tool `{tool_id}`"),
                )
                .with_tool(tool_id)
            })?;
        if !self.config.is_tool_enabled(tool_id) {
            return Err(SkillSyncError::new(
                ErrorCode::ToolDisabled,
                format!("integration for `{tool_id}` is disabled in the configuration"),
            )
            .with_tool(tool_id)
            .recoverable());
        }
        let canonical_skills = self.canonical_skills()?;
        sync::plan_tool_sync(&self.sync_context(), adapter.as_ref(), &canonical_skills)
    }

    /// Plan and execute a sync (dry-run capable).
    pub fn sync_tool(&self, tool_id: &str, dry_run: bool) -> Result<SyncRunReport> {
        let plan = self.plan_sync(tool_id)?;
        sync::execute_sync(&self.sync_context(), &plan, dry_run)
    }

    /// Sync every detected, enabled tool (§75 Slice 4 multi-tool sync).
    /// Each tool gets its own plan and report; one tool's failure does not
    /// stop the others.
    pub fn sync_all(&self, dry_run: bool) -> Result<Vec<SyncRunReport>> {
        let mut reports = Vec::new();
        for adapter in &self.adapters {
            let id = adapter.id();
            if !self.config.is_tool_enabled(id) || !adapter.detect(&self.env).installed {
                continue;
            }
            reports.push(self.sync_tool(id, dry_run)?);
        }
        Ok(reports)
    }

    /// Detect canonical ⇄ unmanaged-target conflicts (§18). Read-only.
    pub fn conflicts(&self) -> Result<Vec<ConflictReport>> {
        let canonical_skills = self.canonical_skills()?;
        let scanned = self.scan_all()?;
        let tool_names: Vec<(String, String)> = self
            .adapters
            .iter()
            .map(|a| (a.id().to_string(), a.display_name().to_string()))
            .collect();
        Ok(conflict::detect_conflicts(
            &self.env,
            &self.config,
            &canonical_skills,
            &scanned,
            &tool_names,
        ))
    }

    /// Directory-aware diff between a canonical skill and a tool's
    /// unmanaged target (§55). Read-only.
    pub fn diff_skill_tool(&self, skill_id: &str, tool_id: &str) -> Result<Vec<DiffEntry>> {
        let canonical_root = {
            let canonical_skills = self.canonical_skills()?;
            canonical_skills
                .iter()
                .find(|s| s.id == skill_id)
                .ok_or_else(|| {
                    SkillSyncError::new(
                        ErrorCode::InvalidSkill,
                        format!("`{skill_id}` is not in the canonical store"),
                    )
                    .with_skill(skill_id)
                    .recoverable()
                })?
                .root
                .clone()
        };
        let canonical_root_resolved = canonical_root
            .canonicalize()
            .unwrap_or_else(|_| canonical_root.clone());
        let over = self.config.tool(tool_id).cloned().unwrap_or_default();
        let adapter = self
            .adapters
            .iter()
            .find(|a| a.id() == tool_id)
            .ok_or_else(|| {
                SkillSyncError::new(
                    ErrorCode::ToolNotFound,
                    format!("no adapter for `{tool_id}`"),
                )
                .with_tool(tool_id)
            })?;
        for location in adapter.global_skill_locations(&self.env, &over) {
            if location.kind == crate::adapter::LocationKind::AgentStandard && !location.overridden
            {
                continue; // that is the canonical store itself
            }
            let target = location.path.join(skill_id);
            if target.is_dir() {
                return conflict::diff_skill_dirs(&canonical_root_resolved, &target);
            }
        }
        Err(SkillSyncError::new(
            ErrorCode::TargetConflict,
            format!("no installation of `{skill_id}` found for `{tool_id}`"),
        )
        .with_skill(skill_id)
        .with_tool(tool_id)
        .recoverable())
    }

    /// Resolve a conflict (§18). Never destructive without a backup; the
    /// target must still be unmanaged at resolution time.
    pub fn resolve_conflict(
        &mut self,
        skill_id: &str,
        tool_id: &str,
        resolution: Resolution,
        dry_run: bool,
    ) -> Result<ResolutionReport> {
        let conflicts = self.conflicts()?;
        let report = conflicts
            .iter()
            .find(|c| c.skill_id == skill_id && c.tool_id == tool_id && !c.ignored)
            .ok_or_else(|| {
                SkillSyncError::new(
                    ErrorCode::TargetConflict,
                    format!("no active conflict for `{skill_id}` × `{tool_id}`"),
                )
                .with_skill(skill_id)
                .with_tool(tool_id)
                .recoverable()
            })?
            .clone();
        let canonical = self
            .canonical_skills()?
            .into_iter()
            .find(|s| s.id == skill_id)
            .ok_or_else(|| {
                SkillSyncError::new(ErrorCode::InvalidSkill, "canonical skill vanished")
                    .with_skill(skill_id)
            })?;
        let adapter = self
            .adapters
            .iter()
            .find(|a| a.id() == tool_id)
            .ok_or_else(|| {
                SkillSyncError::new(
                    ErrorCode::ToolNotFound,
                    format!("no adapter for `{tool_id}`"),
                )
                .with_tool(tool_id)
            })?
            .clone();
        let ctx = self.sync_context();
        let method = ctx.effective_method(adapter.as_ref(), &mut Vec::new());
        let mut registry = crate::managed::ManagedRegistry::load(&self.env, &self.paths);
        let result = conflict::resolve_conflict(
            &ctx,
            &report,
            &canonical,
            resolution,
            method,
            &mut registry,
            dry_run,
        );
        if result.is_ok() && !dry_run {
            registry.save(&self.paths)?;
        }
        result
    }

    /// Plan the first import (§19/§56/§57): classify every observed skill
    /// as already-canonical, unique-import, or conflict. Read-only.
    pub fn first_import_plan(&self) -> Result<FirstImportPlan> {
        let canonical_skills = self.canonical_skills()?;
        let scanned = self.scan_all()?;
        let canonical_root = self.config.canonical_root(&self.env);
        let tool_names: Vec<(String, String)> = self
            .adapters
            .iter()
            .map(|a| (a.id().to_string(), a.display_name().to_string()))
            .collect();
        Ok(firstimport::plan_first_import(
            &self.env,
            &canonical_root,
            &canonical_skills,
            &scanned,
            &tool_names,
        ))
    }

    /// Apply a first-import plan by reusing the import machinery:
    /// create-only, never overwrites, dry-run capable.
    pub fn apply_first_import(
        &self,
        plan: &FirstImportPlan,
        dry_run: bool,
    ) -> Result<FirstImportReport> {
        firstimport::apply_first_import(self, plan, dry_run)
    }

    /// Git status of the canonical store (machine sync, §34/§35).
    pub fn git_status(&self) -> Result<GitStatus> {
        let root = self.config.canonical_root(&self.env);
        git::status(&self.env, &root)
    }

    /// Explicit `git pull --ff-only` on the canonical store.
    pub fn git_pull(&self) -> Result<String> {
        let root = self.config.canonical_root(&self.env);
        git::pull(&self.env, &root)
    }

    /// Explicit `git add -A` + `git commit` on the canonical store.
    pub fn git_commit(&self, message: &str) -> Result<String> {
        let root = self.config.canonical_root(&self.env);
        git::commit(&self.env, &root, message)
    }

    /// Explicit `git push` on the canonical store.
    pub fn git_push(&self) -> Result<String> {
        let root = self.config.canonical_root(&self.env);
        git::push(&self.env, &root)
    }

    /// Set a Skill×Tool enablement choice and apply it: enabling installs
    /// that one installation, disabling removes only the managed one (§27).
    pub fn set_skill_tool_enabled(
        &mut self,
        skill_id: &str,
        tool_id: &str,
        enabled: bool,
        dry_run: bool,
    ) -> Result<SyncRunReport> {
        // Validate the tool first.
        if !self.adapters.iter().any(|a| a.id() == tool_id) {
            return Err(SkillSyncError::new(
                ErrorCode::ToolNotFound,
                format!("no adapter for tool `{tool_id}`"),
            )
            .with_tool(tool_id));
        }
        // Validate the canonical skill exists.
        let canonical_skills = self.canonical_skills()?;
        if !canonical_skills.iter().any(|s| s.id == skill_id) {
            return Err(SkillSyncError::new(
                ErrorCode::InvalidSkill,
                format!("`{skill_id}` is not in the canonical store"),
            )
            .with_skill(skill_id)
            .recoverable());
        }

        // Persist the desired state (SkillSync-owned config only).
        let mut config = self.config.clone();
        config.set_skill_tool_enabled(skill_id, tool_id, enabled);
        self.save_config(config)?;

        // Plan under the new config, then execute only this skill's entry.
        let mut plan = self.plan_sync(tool_id)?;
        plan.entries.retain(|e| e.skill_id == skill_id);
        if plan.entries.is_empty() {
            return Err(SkillSyncError::new(
                ErrorCode::InvalidSkill,
                format!("plan produced no entry for `{skill_id}`"),
            )
            .with_skill(skill_id));
        }
        sync::execute_sync(&self.sync_context(), &plan, dry_run)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox() -> (tempfile::TempDir, SkillSync) {
        let tmp = tempfile::tempdir().unwrap();
        // Isolate PATH so binaries installed on the host machine cannot
        // influence detection in tests.
        let mut env = EnvContext::with_home(tmp.path());
        env.env.insert("PATH".into(), String::new());
        let app = SkillSync::with_environment(env);
        (tmp, app)
    }

    const BASIC: &[u8] = b"---\nname: basic-skill\ndescription: A basic skill\n---\n# Basic\n";

    #[test]
    fn overview_on_fresh_machine_is_empty_but_complete() {
        let (_tmp, app) = sandbox();
        let overview = app.overview().unwrap();
        assert!(overview.rows.is_empty());
        assert!(!overview.canonical_root_exists);
        assert_eq!(overview.tools.len(), 4);
        for tool in &overview.tools {
            assert!(!tool.detection.installed, "{:?}", tool.id);
            assert_eq!(tool.skill_count, 0);
        }
    }

    #[test]
    fn canonical_skills_are_listed_and_observed_matched_by_fingerprint() {
        let (tmp, mut app) = sandbox();
        let canonical = tmp.path().join(".agents").join("skills");
        let claude_skills = tmp.path().join(".claude").join("skills");
        std::fs::create_dir_all(&canonical).unwrap();
        std::fs::create_dir_all(&claude_skills).unwrap();
        std::fs::create_dir_all(canonical.join("basic-skill")).unwrap();
        std::fs::write(canonical.join("basic-skill").join("SKILL.md"), BASIC).unwrap();
        std::fs::create_dir_all(claude_skills.join("basic-skill")).unwrap();
        std::fs::write(claude_skills.join("basic-skill").join("SKILL.md"), BASIC).unwrap();
        // A tool the user has installed and detected:
        std::fs::create_dir_all(tmp.path().join(".claude")).unwrap();

        app.save_config(Config::default()).unwrap();
        let overview = app.overview().unwrap();
        assert_eq!(overview.rows.len(), 1, "{:?}", overview.rows);
        let row = &overview.rows[0];
        assert!(row.canonical.is_some());
        let claude_install = row
            .installations
            .iter()
            .find(|i| i.tool_id == "claude")
            .unwrap();
        assert_eq!(claude_install.state, SyncState::Unmanaged);
        assert!(
            claude_install.fingerprint.is_some(),
            "fingerprint needed for content match"
        );
        // No managed installation yet: the identical copy in Claude is an
        // import candidate, so the row status stays "not installed".
        assert_eq!(row.status, SyncState::NotInstalled);
    }

    #[test]
    fn managed_symlink_is_recognized_as_synced() {
        let (tmp, mut app) = sandbox();
        let canonical = tmp.path().join(".agents").join("skills");
        let claude_skills = tmp.path().join(".claude").join("skills");
        std::fs::create_dir_all(&canonical).unwrap();
        std::fs::create_dir_all(&claude_skills).unwrap();
        std::fs::create_dir_all(canonical.join("linked")).unwrap();
        std::fs::write(canonical.join("linked").join("SKILL.md"), BASIC).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(canonical.join("linked"), claude_skills.join("linked")).unwrap();
        std::fs::create_dir_all(tmp.path().join(".claude")).unwrap();

        app.save_config(Config::default()).unwrap();
        let overview = app.overview().unwrap();
        assert_eq!(overview.rows.len(), 1);
        assert_eq!(overview.rows[0].status, SyncState::Synced);
        let install = overview.rows[0]
            .installations
            .iter()
            .find(|i| i.tool_id == "claude")
            .unwrap();
        assert_eq!(install.state, SyncState::Synced);
        assert!(matches!(
            install.managedness,
            Managedness::ManagedSymlink { .. }
        ));
    }

    #[test]
    fn same_name_different_content_is_not_merged() {
        let (tmp, mut app) = sandbox();
        let canonical = tmp.path().join(".agents").join("skills");
        let claude_skills = tmp.path().join(".claude").join("skills");
        std::fs::create_dir_all(&canonical).unwrap();
        std::fs::create_dir_all(&claude_skills).unwrap();
        std::fs::create_dir_all(canonical.join("code-review")).unwrap();
        std::fs::write(
            canonical.join("code-review").join("SKILL.md"),
            b"---\nname: code-review\ndescription: canonical review\n---\nA",
        )
        .unwrap();
        std::fs::create_dir_all(claude_skills.join("code-review")).unwrap();
        std::fs::write(
            claude_skills.join("code-review").join("SKILL.md"),
            b"---\nname: code-review\ndescription: totally different workflow\n---\nB",
        )
        .unwrap();
        std::fs::create_dir_all(tmp.path().join(".claude")).unwrap();

        app.save_config(Config::default()).unwrap();
        let overview = app.overview().unwrap();
        assert_eq!(overview.rows.len(), 2, "same name must stay separate rows");
        assert!(overview.rows.iter().any(|r| r.canonical.is_some()));
        assert!(overview
            .rows
            .iter()
            .all(|r| r.canonical.is_none() || r.status == SyncState::NotInstalled));
    }

    #[test]
    fn cursor_reads_canonical_root_natively() {
        let (tmp, mut app) = sandbox();
        let canonical = tmp.path().join(".agents").join("skills");
        std::fs::create_dir_all(&canonical).unwrap();
        std::fs::create_dir_all(canonical.join("native")).unwrap();
        std::fs::write(canonical.join("native").join("SKILL.md"), BASIC).unwrap();
        std::fs::create_dir_all(tmp.path().join(".cursor")).unwrap();

        app.save_config(Config::default()).unwrap();
        let overview = app.overview().unwrap();
        let row = &overview.rows[0];
        let cursor = row
            .installations
            .iter()
            .find(|i| i.tool_id == "cursor")
            .unwrap();
        assert_eq!(cursor.state, SyncState::Native);
        assert!(matches!(cursor.managedness, Managedness::NativeShared));
    }

    #[test]
    fn sync_is_rejected_for_disabled_tools() {
        let (_tmp, mut app) = sandbox();
        let mut config = Config::default();
        config.tools.insert(
            "claude".into(),
            ToolOverride {
                enabled: Some(false),
                ..Default::default()
            },
        );
        app.save_config(config).unwrap();
        let err = app.plan_sync("claude").unwrap_err();
        assert_eq!(err.code, ErrorCode::ToolDisabled);
        assert!(err.recoverable);
    }

    #[test]
    fn sync_unknown_tool_is_tool_not_found() {
        let (_tmp, app) = sandbox();
        let err = app.plan_sync("not-a-tool").unwrap_err();
        assert_eq!(err.code, ErrorCode::ToolNotFound);
    }
    #[test]
    fn config_round_trip_through_facade() {
        let (_tmp, mut app) = sandbox();
        let config = Config {
            canonical_skill_root: "~/Developer/agent-skills".into(),
            ..Default::default()
        };
        app.save_config(config).unwrap();
        assert_eq!(
            app.config().canonical_skill_root,
            "~/Developer/agent-skills"
        );
        // persisted to disk
        let reloaded = config::load_config(app.paths()).unwrap();
        assert_eq!(reloaded.canonical_skill_root, "~/Developer/agent-skills");
    }
}
