//! Automatic synchronization (design doc §7f Slice 6, prompt §32, §33).
//!
//! Watches for changes and — only when `config.autoSync` is enabled —
//! refreshes managed copy targets via the normal sync engine (symlinked
//! targets need no work; copies are updated backup-first). Correctness
//! rules:
//!
//! - The watcher never mutates anything itself; every refresh goes through
//!   `sync_all`, with its plan/skip/backup semantics.
//! - `config.autoSync` is re-read every cycle: toggling it off takes effect
//!   without a restart, and a watcher failure can never lose data — at
//!   worst it stops notifying, and manual Sync Now remains available (§33).
//! - Only SkillSync's own home is watched as the event anchor; events are
//!   just "something changed" triggers, the sync engine re-validates
//!   everything before acting.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use notify::{RecursiveMode, Watcher};

use crate::env::EnvContext;
use crate::error::{Result, SkillSyncError};

/// Receiver for auto-sync run reports (e.g. the GUI event bridge).
pub trait AutoSyncSink: Send + Sync {
    /// Called after each automatic sync pass with per-tool summaries.
    fn on_auto_sync(&self, summaries: Vec<String>);
}

/// A no-op sink (CLI, tests).
pub struct NullSink;
impl AutoSyncSink for NullSink {
    fn on_auto_sync(&self, _summaries: Vec<String>) {}
}

/// Handle to the background watcher. Dropping it signals the thread to
/// stop and joins it.
pub struct AutoSyncHandle {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    runs: Arc<AtomicU64>,
}

impl AutoSyncHandle {
    /// Number of automatic sync passes that actually ran (tests/telemetry).
    pub fn runs(&self) -> u64 {
        self.runs.load(Ordering::Relaxed)
    }
}

impl Drop for AutoSyncHandle {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Spawn the auto-sync watcher thread.
///
/// * `debounce` — quiet period after the last filesystem event before a
///   sync runs (§33: editors fire many events per save).
/// * `min_interval` — lower bound between two automatic passes.
pub fn spawn_auto_sync(
    env: EnvContext,
    sink: Arc<dyn AutoSyncSink>,
    debounce: Duration,
    min_interval: Duration,
) -> Result<AutoSyncHandle> {
    let paths = crate::config::AppPaths::discover(&env);
    let stop = Arc::new(AtomicBool::new(false));
    let runs = Arc::new(AtomicU64::new(0));

    let (tx, rx) = std::sync::mpsc::channel::<notify::Result<notify::Event>>();
    // Watch SkillSync's own home as a stable anchor (the canonical root may
    // not exist yet). Events are triggers only; the engine re-validates.
    let anchor = paths.home.clone();
    std::fs::create_dir_all(&anchor).map_err(|e| SkillSyncError::io(&e, &anchor))?;
    let mut watcher = notify::recommended_watcher(tx)
        .map_err(|e| SkillSyncError::new(crate::error::ErrorCode::Io, e.to_string()))?;
    watcher
        .watch(&anchor, RecursiveMode::NonRecursive)
        .map_err(|e| SkillSyncError::new(crate::error::ErrorCode::Io, e.to_string()))?;
    let watcher = Arc::new(Mutex::new(watcher));

    let stop_flag = Arc::clone(&stop);
    let runs_counter = Arc::clone(&runs);
    let thread = std::thread::Builder::new()
        .name("skillsync-auto-sync".into())
        .spawn(move || {
            let _keep_watcher_alive = watcher;
            let mut last_event: Option<Instant> = None;
            let mut last_run: Option<Instant> = None;
            loop {
                if stop_flag.load(Ordering::Relaxed) {
                    return;
                }
                while let Ok(event) = rx.try_recv() {
                    if event.is_ok() {
                        last_event = Some(Instant::now());
                    }
                }

                if let Some(seen) = last_event {
                    let quiet = seen.elapsed() >= debounce;
                    let spaced = last_run
                        .map(|r| r.elapsed() >= min_interval)
                        .unwrap_or(true);
                    if quiet && spaced {
                        last_event = None;
                        last_run = Some(Instant::now());
                        // Fresh facade each pass: config changes (the
                        // autoSync toggle) apply immediately.
                        let app = crate::SkillSync::with_environment(env.clone());
                        if !app.config().auto_sync {
                            continue;
                        }
                        if let Ok(reports) = app.sync_all(false) {
                            runs_counter.fetch_add(1, Ordering::Relaxed);
                            sink.on_auto_sync(
                                reports
                                    .iter()
                                    .map(|r| format!("{}: {}", r.tool_id, r.summary()))
                                    .collect(),
                            );
                        }
                        // A failed pass logs nothing and retries on the
                        // next event; data can never be lost by watching.
                    }
                }

                std::thread::sleep(Duration::from_millis(250));
            }
        })
        .map_err(|e| SkillSyncError::new(crate::error::ErrorCode::Io, e.to_string()))?;

    Ok(AutoSyncHandle {
        stop,
        thread: Some(thread),
        runs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    struct TxSink(Mutex<mpsc::Sender<()>>);
    impl AutoSyncSink for TxSink {
        fn on_auto_sync(&self, _summaries: Vec<String>) {
            let _ = self.0.lock().unwrap().send(());
        }
    }

    #[test]
    fn disabled_by_default_means_no_runs() {
        // autoSync defaults to false: events never trigger syncs.
        let tmp = tempfile::tempdir().unwrap();
        let mut env = EnvContext::with_home(tmp.path().join("home"));
        env.env.insert("PATH".into(), String::new());
        std::fs::create_dir_all(env.home.join(".skillsync")).unwrap();

        let (tx, rx) = mpsc::channel::<()>();
        let _drain = std::thread::spawn(move || {
            let _ = rx.recv_timeout(Duration::from_secs(1));
        });

        let handle = spawn_auto_sync(
            env.clone(),
            Arc::new(TxSink(Mutex::new(tx))),
            Duration::from_millis(100),
            Duration::from_millis(200),
        )
        .unwrap();

        std::fs::write(env.home.join(".skillsync").join("noise"), b"x").unwrap();
        std::thread::sleep(Duration::from_millis(700));
        assert_eq!(handle.runs(), 0, "autoSync defaults to off");
    }

    #[test]
    fn enabled_config_runs_after_debounce() {
        let tmp = tempfile::tempdir().unwrap();
        let mut env = EnvContext::with_home(tmp.path().join("home"));
        env.env.insert("PATH".into(), String::new());
        std::fs::create_dir_all(env.home.join(".skillsync")).unwrap();
        // Persist autoSync = true and a canonical skill for a fake tool
        // dir so sync_all has something to do (claude not detected here,
        // so likely zero tool reports — the run counter is what matters).
        let paths = crate::config::AppPaths::discover(&env);
        let config = crate::config::Config {
            auto_sync: true,
            ..Default::default()
        };
        crate::config::save_config(&paths, &config).unwrap();

        let (tx, _rx) = mpsc::channel::<()>();
        let handle = spawn_auto_sync(
            env.clone(),
            Arc::new(TxSink(Mutex::new(tx))),
            Duration::from_millis(150),
            Duration::from_millis(100),
        )
        .unwrap();

        std::fs::write(env.home.join(".skillsync").join("noise"), b"x").unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while handle.runs() == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(handle.runs() >= 1, "auto-sync pass should have run");
    }
}
