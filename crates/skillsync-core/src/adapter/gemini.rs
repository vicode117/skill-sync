//! Adapter for **Gemini CLI**.
//!
//! Documented locations (2026-08): personal skills `~/.gemini/skills`,
//! project skills `.gemini/skills`.
//!
//! Important adapter-specific behavior: Gemini CLI does not follow
//! symlinks when discovering skills (google-gemini/gemini-cli#16247), so
//! the `Auto` sync method must fall back to **copy** for this tool.

use std::path::Path;

use crate::config::ToolOverride;
use crate::env::EnvContext;
use crate::error::Result;
use crate::scan::ScannedSkill;
use crate::skill::SkillScope;

use super::{
    override_path, scan_location, LocationKind, ReloadGuidance, SkillLocation, SymlinkSupport,
    ToolAdapter, ToolDetection,
};

pub struct GeminiAdapter;

impl ToolAdapter for GeminiAdapter {
    fn id(&self) -> &'static str {
        "gemini"
    }

    fn display_name(&self) -> &'static str {
        "Gemini CLI"
    }

    fn detect(&self, env: &EnvContext) -> ToolDetection {
        let config_dir = env.home_relative(&[".gemini"]);
        if config_dir.is_dir() {
            return ToolDetection {
                installed: true,
                evidence: format!("found config directory {}", config_dir.display()),
                config_dir: Some(config_dir),
            };
        }
        if let Some(bin) = env.which("gemini") {
            return ToolDetection {
                installed: true,
                evidence: format!("found `gemini` binary at {}", bin.display()),
                config_dir: None,
            };
        }
        ToolDetection {
            installed: false,
            evidence: "no ~/.gemini directory and no `gemini` binary on PATH".into(),
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
            path: env.home_relative(&[".gemini", "skills"]),
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
        SymlinkSupport::Avoided
    }

    fn reload_guidance(&self) -> ReloadGuidance {
        ReloadGuidance {
            summary: "Skills load at session start".into(),
            detail: "Gemini CLI reads personal skills (~/.gemini/skills) at session start; \
                     restart the session to pick up changes. Gemini CLI currently does not \
                     follow symlinks for skill discovery, so SkillSync uses copies for Gemini \
                     and tracks drift by fingerprint."
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
        contract::run_contract_tests(&GeminiAdapter);
    }

    #[test]
    fn prefers_copy_over_symlinks() {
        assert_eq!(GeminiAdapter.symlink_support(), SymlinkSupport::Avoided);
    }

    #[test]
    fn default_location_is_gemini_skills() {
        let tmp = tempfile::tempdir().unwrap();
        let env = contract::isolated_env(tmp.path());
        let locations = GeminiAdapter.global_skill_locations(&env, &ToolOverride::default());
        assert_eq!(locations[0].path, tmp.path().join(".gemini").join("skills"));
    }
}
