//! Tauri command boundary (design doc §67): a thin, typed layer over
//! `skillsync-core`. No business logic here; commands delegate to the core
//! facade and return its structured error type directly.

use std::sync::Mutex;

use skillsync_core::{Config, DoctorReport, SkillOverview, SkillSync, SkillSyncError};

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
            run_doctor
        ])
        .run(tauri::generate_context!())
        .expect("error while running SkillSync");
}
