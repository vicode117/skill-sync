//! Tauri command boundary (design doc §67): a thin, typed layer over
//! `skillsync-core`. No business logic here; commands delegate to the core
//! facade and return its structured error type directly.

use std::sync::{Arc, Mutex};

use skillsync_core::{
    Config, ConflictReport, DiffEntry, DoctorReport, FirstImportPlan, FirstImportReport, GitStatus,
    ImportOutcome, ImportPlan, Resolution, ResolutionReport, SkillOverview, SkillSync,
    SkillSyncError, SyncPlan, SyncRunReport,
};

/// Shared application state: one core facade instance per session.
pub struct AppState {
    app: Mutex<SkillSync>,
}

#[tauri::command]
fn get_config(state: tauri::State<AppState>) -> Result<Config, SkillSyncError> {
    let app = state.app.lock().map_err(|_| poisoned())?;
    Ok(app.config().clone())
}

#[tauri::command]
fn save_config(state: tauri::State<AppState>, config: Config) -> Result<Config, SkillSyncError> {
    let mut app = state.app.lock().map_err(|_| poisoned())?;
    app.save_config(config)?;
    Ok(app.config().clone())
}

#[tauri::command]
fn scan_overview(state: tauri::State<AppState>) -> Result<SkillOverview, SkillSyncError> {
    let app = state.app.lock().map_err(|_| poisoned())?;
    app.overview()
}

#[tauri::command]
fn run_doctor(state: tauri::State<AppState>) -> Result<DoctorReport, SkillSyncError> {
    let app = state.app.lock().map_err(|_| poisoned())?;
    Ok(app.doctor())
}

#[tauri::command]
fn adopt_canonical_root(
    state: tauri::State<AppState>,
) -> Result<std::path::PathBuf, SkillSyncError> {
    let app = state.app.lock().map_err(|_| poisoned())?;
    app.adopt_canonical_root()
}

#[tauri::command]
fn plan_import(
    state: tauri::State<AppState>,
    source_path: String,
) -> Result<ImportPlan, SkillSyncError> {
    let app = state.app.lock().map_err(|_| poisoned())?;
    app.plan_import(
        std::path::Path::new(&source_path),
        skillsync_core::ImportResolution::Skip,
    )
}

#[tauri::command]
fn plan_sync(state: tauri::State<AppState>, tool_id: String) -> Result<SyncPlan, SkillSyncError> {
    let app = state.app.lock().map_err(|_| poisoned())?;
    app.plan_sync(&tool_id)
}

/// Execute a sync (dry-run capable).
#[tauri::command]
fn sync_tool(
    state: tauri::State<AppState>,
    tool_id: String,
    dry_run: Option<bool>,
) -> Result<SyncRunReport, SkillSyncError> {
    let app = state.app.lock().map_err(|_| poisoned())?;
    app.sync_tool(&tool_id, dry_run.unwrap_or(false))
}

/// Set a Skill×Tool enablement choice and apply it (install/remove the
/// managed installation only).
#[tauri::command]
fn set_skill_tool_enabled(
    state: tauri::State<AppState>,
    skill_id: String,
    tool_id: String,
    enabled: bool,
    dry_run: Option<bool>,
) -> Result<SyncRunReport, SkillSyncError> {
    let mut app = state.app.lock().map_err(|_| poisoned())?;
    app.set_skill_tool_enabled(&skill_id, &tool_id, enabled, dry_run.unwrap_or(false))
}

/// Sync every detected, enabled tool.
#[tauri::command]
fn sync_all(
    state: tauri::State<AppState>,
    dry_run: Option<bool>,
) -> Result<Vec<SyncRunReport>, SkillSyncError> {
    let app = state.app.lock().map_err(|_| poisoned())?;
    app.sync_all(dry_run.unwrap_or(false))
}

/// List canonical vs unmanaged-target conflicts (§18).
#[tauri::command]
fn list_conflicts(state: tauri::State<AppState>) -> Result<Vec<ConflictReport>, SkillSyncError> {
    let app = state.app.lock().map_err(|_| poisoned())?;
    app.conflicts()
}

/// Diff a canonical skill against a tool's unmanaged target (§55).
#[tauri::command]
fn diff_skill(
    state: tauri::State<AppState>,
    skill_id: String,
    tool_id: String,
) -> Result<Vec<DiffEntry>, SkillSyncError> {
    let app = state.app.lock().map_err(|_| poisoned())?;
    app.diff_skill_tool(&skill_id, &tool_id)
}

/// Resolve a conflict (explicit user choice; backups always created).
#[tauri::command]
fn resolve_conflict(
    state: tauri::State<AppState>,
    skill_id: String,
    tool_id: String,
    resolution: String,
    dry_run: Option<bool>,
) -> Result<ResolutionReport, SkillSyncError> {
    let resolution = match resolution.as_str() {
        "useCanonical" => Resolution::UseCanonical,
        "importTarget" => Resolution::ImportTarget,
        "keepBoth" => Resolution::KeepBoth,
        other => {
            return Err(SkillSyncError::new(
                skillsync_core::ErrorCode::ConfigInvalid,
                format!("unknown resolution `{other}`"),
            ))
        }
    };
    let mut app = state.app.lock().map_err(|_| poisoned())?;
    app.resolve_conflict(&skill_id, &tool_id, resolution, dry_run.unwrap_or(false))
}

/// Ignore (or unignore) a conflict.
#[tauri::command]
fn set_conflict_ignored(
    state: tauri::State<AppState>,
    skill_id: String,
    tool_id: String,
    ignored: bool,
) -> Result<Config, SkillSyncError> {
    let mut app = state.app.lock().map_err(|_| poisoned())?;
    let mut config = app.config().clone();
    config.set_conflict_ignored(&skill_id, &tool_id, ignored);
    app.save_config(config)?;
    Ok(app.config().clone())
}

/// First-import plan (§19/§56/§57).
#[tauri::command]
fn first_import_plan(state: tauri::State<AppState>) -> Result<FirstImportPlan, SkillSyncError> {
    let app = state.app.lock().map_err(|_| poisoned())?;
    app.first_import_plan()
}

/// Apply a first-import plan (create-only; dry-run capable).
#[tauri::command]
fn apply_first_import(
    state: tauri::State<AppState>,
    plan: FirstImportPlan,
    dry_run: Option<bool>,
) -> Result<FirstImportReport, SkillSyncError> {
    let app = state.app.lock().map_err(|_| poisoned())?;
    app.apply_first_import(&plan, dry_run.unwrap_or(false))
}

/// Read a text file from an allowed skill location for preview (§26).
#[tauri::command]
fn read_skill_file(state: tauri::State<AppState>, path: String) -> Result<String, SkillSyncError> {
    let app = state.app.lock().map_err(|_| poisoned())?;
    app.read_skill_file(std::path::Path::new(&path))
}

/// Open a skill directory in the OS file explorer (§26).
#[tauri::command]
fn open_in_explorer(state: tauri::State<AppState>, path: String) -> Result<(), SkillSyncError> {
    let app = state.app.lock().map_err(|_| poisoned())?;
    app.open_skill_dir(std::path::Path::new(&path))
}

/// Git status of the canonical store (machine sync, §35).
#[tauri::command]
fn git_status(state: tauri::State<AppState>) -> Result<GitStatus, SkillSyncError> {
    let app = state.app.lock().map_err(|_| poisoned())?;
    app.git_status()
}

/// Explicit git operations — never automatic (§35).
#[tauri::command]
fn git_pull(state: tauri::State<AppState>) -> Result<String, SkillSyncError> {
    let app = state.app.lock().map_err(|_| poisoned())?;
    app.git_pull()
}

#[tauri::command]
fn git_commit(state: tauri::State<AppState>, message: String) -> Result<String, SkillSyncError> {
    let app = state.app.lock().map_err(|_| poisoned())?;
    app.git_commit(&message)
}

#[tauri::command]
fn git_push(state: tauri::State<AppState>) -> Result<String, SkillSyncError> {
    let app = state.app.lock().map_err(|_| poisoned())?;
    app.git_push()
}

/// Execute an import. `resolution` is one of `skip`, `keepBoth`, `replace`;
/// `skip` (the default) never overwrites existing canonical content.
#[tauri::command]
fn import_skill(
    state: tauri::State<AppState>,
    source_path: String,
    resolution: Option<String>,
    dry_run: Option<bool>,
) -> Result<ImportOutcome, SkillSyncError> {
    let resolution = match resolution.as_deref() {
        Some("keepBoth") => skillsync_core::ImportResolution::KeepBoth,
        Some("replace") => skillsync_core::ImportResolution::Replace,
        _ => skillsync_core::ImportResolution::Skip,
    };
    let app = state.app.lock().map_err(|_| poisoned())?;
    let plan = app.plan_import(std::path::Path::new(&source_path), resolution)?;
    app.execute_import(&plan, dry_run.unwrap_or(false))
}

fn poisoned() -> SkillSyncError {
    SkillSyncError::new(
        skillsync_core::ErrorCode::Io,
        "internal state lock poisoned",
    )
}

/// Bridges auto-sync passes to the frontend (design doc §7f).
struct AutoSyncEventSink {
    handle: tauri::AppHandle,
}

impl skillsync_core::AutoSyncSink for AutoSyncEventSink {
    fn on_auto_sync(&self, summaries: Vec<String>) {
        use tauri::Emitter;
        let _ = self.handle.emit("auto-sync-ran", summaries);
    }
}

/// Keeps the watcher thread alive for the whole session.
struct AutoSyncState {
    _handle: Mutex<Option<skillsync_core::AutoSyncHandle>>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = SkillSync::discover().unwrap_or_else(|err| {
        // Fall back to a defaults-only facade; the UI surfaces the error
        // through config reads instead of failing to launch.
        eprintln!("warning: falling back to default environment: {err}");
        SkillSync::with_environment(skillsync_core::EnvContext::with_home(
            std::env::temp_dir().join("skillsync-fallback-home"),
        ))
    });

    tauri::Builder::default()
        .manage(AppState {
            app: Mutex::new(app),
        })
        .setup(|app_handle| {
            // Automatic synchronization (§32): off unless config.autoSync;
            // the watcher re-reads config every cycle, so toggling applies
            // immediately. A watcher that fails to start never blocks the
            // app — manual Sync Now always remains available (§33).
            use tauri::Manager;
            if let Ok(env) = skillsync_core::EnvContext::discover() {
                let sink = Arc::new(AutoSyncEventSink {
                    handle: app_handle.handle().clone(),
                });
                if let Ok(handle) = skillsync_core::watcher::spawn_auto_sync(
                    env,
                    sink,
                    std::time::Duration::from_secs(2),
                    std::time::Duration::from_secs(5),
                ) {
                    app_handle.handle().manage(AutoSyncState {
                        _handle: Mutex::new(Some(handle)),
                    });
                }
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            scan_overview,
            run_doctor,
            adopt_canonical_root,
            plan_import,
            import_skill,
            plan_sync,
            sync_tool,
            sync_all,
            set_skill_tool_enabled,
            list_conflicts,
            diff_skill,
            resolve_conflict,
            set_conflict_ignored,
            git_status,
            git_pull,
            git_commit,
            git_push,
            first_import_plan,
            apply_first_import,
            read_skill_file,
            open_in_explorer
        ])
        .run(tauri::generate_context!())
        .expect("error while running SkillSync");
}
