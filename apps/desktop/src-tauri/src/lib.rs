//! Tauri command boundary (design doc §67): a thin, typed layer over
//! `skillsync-core`. No business logic here; commands delegate to the core
//! facade and return its structured error type directly.

use std::sync::Mutex;

use skillsync_core::{
    Config, DoctorReport, ImportOutcome, ImportPlan, SkillOverview, SkillSync, SkillSyncError,
    SyncPlan, SyncRunReport,
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
        skillsync_core::ConflictResolution::Skip,
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

/// Execute an import. `resolution` is one of `skip`, `keepBoth`, `replace`;
/// `skip` (the default) never overwrites existing canonical content.
#[tauri::command]
fn import_skill(
    state: tauri::State<AppState>,
    source_path: String,
    resolution: Option<String>,
    dry_run: Option<bool>,
) -> Result<ImportOutcome, SkillSyncError> {
    use skillsync_core::ConflictResolution as R;
    let resolution = match resolution.as_deref() {
        Some("keepBoth") => R::KeepBoth,
        Some("replace") => R::Replace,
        _ => R::Skip,
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
        .invoke_handler(tauri::generate_handler![
            get_config,
            save_config,
            scan_overview,
            run_doctor,
            adopt_canonical_root,
            plan_import,
            import_skill,
            plan_sync,
            sync_tool
        ])
        .run(tauri::generate_context!())
        .expect("error while running SkillSync");
}
