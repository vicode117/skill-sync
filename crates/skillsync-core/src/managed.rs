//! Ownership registry for SkillSync-managed *copies* (design doc §28).
//!
//! Symlink ownership is derivable from the link target, but a copied
//! installation needs minimal metadata kept OUTSIDE the skill directories.
//! This registry lives in `~/.skillsync/managed.json` and is the only
//! proof that a plain directory in a tool's skills folder was installed by
//! SkillSync. Absence of a record ⇒ unmanaged ⇒ never touched.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::AppPaths;
use crate::env::EnvContext;
use crate::error::Result;
use crate::fsutil::atomic_write;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedInstall {
    pub tool_id: String,
    pub skill_id: String,
    /// Absolute path of the installed copy (inside a tool skills dir).
    pub target: PathBuf,
    /// Fingerprint of the canonical skill at install/update time.
    pub fingerprint: String,
    pub installed_at: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ManagedRegistry {
    installs: Vec<ManagedInstall>,
}

impl ManagedRegistry {
    /// Load `~/.skillsync/managed.json`; missing file ⇒ empty registry.
    pub fn load(env: &EnvContext, paths: &AppPaths) -> Self {
        let file = paths.managed_file();
        match std::fs::read(&file) {
            Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
            Err(_) => {
                let _ = env;
                Self::default()
            }
        }
    }

    pub fn save(&self, paths: &AppPaths) -> Result<()> {
        let file = paths.managed_file();
        atomic_write(
            &file,
            serde_json::to_vec_pretty(self)
                .map_err(|e| {
                    crate::error::SkillSyncError::new(crate::error::ErrorCode::Io, e.to_string())
                })?
                .as_slice(),
        )
    }

    pub fn installs(&self) -> &[ManagedInstall] {
        &self.installs
    }

    pub fn find_by_target(&self, target: &Path) -> Option<&ManagedInstall> {
        self.installs.iter().find(|i| i.target == target)
    }

    pub fn upsert(&mut self, install: ManagedInstall) {
        self.installs.retain(|i| i.target != install.target);
        self.installs.push(install);
    }

    pub fn remove_by_target(&mut self, target: &Path) -> bool {
        let before = self.installs.len();
        self.installs.retain(|i| i.target != target);
        before != self.installs.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_through_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let env = EnvContext::with_home(tmp.path().join("home"));
        let paths = AppPaths {
            home: tmp.path().join("sync-home"),
        };
        let mut reg = ManagedRegistry::load(&env, &paths);
        assert!(reg.installs().is_empty(), "missing file means empty");

        reg.upsert(ManagedInstall {
            tool_id: "gemini".into(),
            skill_id: "tdd".into(),
            target: tmp.path().join("gemini-skills/tdd"),
            fingerprint: "abc".into(),
            installed_at: "2026-08-29T00:00:00Z".into(),
        });
        reg.save(&paths).unwrap();

        let reloaded = ManagedRegistry::load(&env, &paths);
        assert_eq!(reloaded.installs().len(), 1);
        assert!(reloaded
            .find_by_target(&tmp.path().join("gemini-skills/tdd"))
            .is_some());

        let mut reloaded = reloaded;
        assert!(reloaded.remove_by_target(&tmp.path().join("gemini-skills/tdd")));
        reloaded.save(&paths).unwrap();
        assert!(ManagedRegistry::load(&env, &paths).installs().is_empty());
    }

    #[test]
    fn upsert_replaces_by_target() {
        let mut reg = ManagedRegistry::default();
        let target = PathBuf::from("/t/x");
        reg.upsert(ManagedInstall {
            tool_id: "claude".into(),
            skill_id: "x".into(),
            target: target.clone(),
            fingerprint: "1".into(),
            installed_at: "a".into(),
        });
        reg.upsert(ManagedInstall {
            tool_id: "claude".into(),
            skill_id: "x".into(),
            target: target.clone(),
            fingerprint: "2".into(),
            installed_at: "b".into(),
        });
        assert_eq!(reg.installs().len(), 1);
        assert_eq!(reg.installs()[0].fingerprint, "2");
    }
}
