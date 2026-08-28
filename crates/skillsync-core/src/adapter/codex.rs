//! Adapter for **OpenAI Codex**.
//!
//! Documented locations (2026-08): skills live under the Codex home
//! (default `~/.codex`, overridable with `CODEX_HOME`) at `<home>/skills`.
//! Codex follows symlinks that resolve to *directories*, but skips a skill
//! whose `SKILL.md` file itself is a symlink (openai/codex#17344) — so
//! SkillSync must always link whole skill directories, never loose files.

use std::path::{Path, PathBuf};

use crate::config::ToolOverride;
use crate::env::EnvContext;
use crate::error::Result;
use crate::scan::ScannedSkill;
use crate::skill::SkillScope;

use super::{
    override_path, scan_location, LocationKind, ReloadGuidance, SkillLocation, SymlinkSupport,
    ToolAdapter, ToolDetection,
};

pub struct CodexAdapter;

/// Resolve the Codex home directory for this environment.
fn codex_home(env: &EnvContext) -> PathBuf {
    env.var("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| env.home_relative(&[".codex"]))
}

impl ToolAdapter for CodexAdapter {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn display_name(&self) -> &'static str {
        "Codex"
    }

    fn detect(&self, env: &EnvContext) -> ToolDetection {
        let config_dir = codex_home(env);
        if config_dir.is_dir() {
            return ToolDetection {
                installed: true,
                evidence: format!("found config directory {}", config_dir.display()),
                config_dir: Some(config_dir),
            };
        }
        if let Some(bin) = env.which("codex") {
            return ToolDetection {
                installed: true,
                evidence: format!("found `codex` binary at {}", bin.display()),
                config_dir: None,
            };
        }
        ToolDetection {
            installed: false,
            evidence: "no ~/.codex directory (or $CODEX_HOME) and no `codex` binary on PATH".into(),
            config_dir: None,
        }
    }

    fn global_skill_locations(&self, env: &EnvContext, over: &ToolOverride) -> Vec<SkillLocation> {
        if let Some(path) = override_path(over, env) {
            return vec![SkillLocation {
                tool_id: self.id().into(),
                scope: SkillScope::Global,
                path,
                kind: LocationKind::Standard,
                overridden: true,
            }];
        }
        vec![SkillLocation {
            tool_id: self.id().into(),
            scope: SkillScope::Global,
            path: codex_home(env).join("skills"),
            kind: LocationKind::Standard,
            overridden: false,
        }]
    }

    fn scan_skills(
        &self,
        env: &EnvContext,
        over: &ToolOverride,
        canonical_root: &Path,
    ) -> Result<Vec<ScannedSkill>> {
        let mut all = Vec::new();
        for location in self.global_skill_locations(env, over) {
            all.extend(scan_location(self.id(), &location, env, canonical_root)?);
        }
        Ok(all)
    }

    fn symlink_support(&self) -> SymlinkSupport {
        SymlinkSupport::Supported
    }

    fn reload_guidance(&self) -> ReloadGuidance {
        ReloadGuidance {
            summary: "Changes are detected automatically / restart if necessary".into(),
            detail: "Codex discovers skills in its skills directory (default ~/.codex/skills, \
                     or $CODEX_HOME/skills) per session; restart the session if a change is \
                     not picked up. Codex follows symlinks that resolve to directories but \
                     skips skills whose SKILL.md file itself is a symlink, so SkillSync links \
                     whole skill directories only."
                .into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::contract;
    use super::*;

    #[test]
    fn adapter_contract() {
        contract::run_contract_tests(&CodexAdapter);
    }

    #[test]
    fn honors_codex_home_env_var() {
        let tmp = tempfile::tempdir().unwrap();
        let mut env = contract::isolated_env(&tmp.path().join("home"));
        let custom = tmp.path().join("custom-codex-home");
        std::fs::create_dir_all(&custom).unwrap();
        env.env
            .insert("CODEX_HOME".into(), custom.to_string_lossy().into_owned());

        let detection = CodexAdapter.detect(&env);
        assert!(detection.installed);
        assert_eq!(detection.config_dir.as_deref(), Some(custom.as_path()));

        let locations = CodexAdapter.global_skill_locations(&env, &ToolOverride::default());
        assert_eq!(locations[0].path, custom.join("skills"));
    }
}
