//! Adapter for **Claude Code**.
//!
//! Documented locations (2026-08): personal skills `~/.claude/skills`,
//! project skills `.claude/skills`. Claude Code follows directory symlinks.

use std::path::Path;

use crate::config::ToolOverride;
use crate::env::EnvContext;
use crate::error::Result;
use crate::scan::ScannedSkill;

use super::{
    override_path, scan_location, LocationKind, ReloadGuidance, SkillLocation, SymlinkSupport,
    ToolAdapter, ToolDetection,
};

pub struct ClaudeAdapter;

impl ToolAdapter for ClaudeAdapter {
    fn id(&self) -> &'static str {
        "claude"
    }

    fn display_name(&self) -> &'static str {
        "Claude Code"
    }

    fn detect(&self, env: &EnvContext) -> ToolDetection {
        let config_dir = env.home_relative(&[".claude"]);
        if config_dir.is_dir() {
            return ToolDetection {
                installed: true,
                evidence: format!("found config directory {}", config_dir.display()),
                config_dir: Some(config_dir),
            };
        }
        if let Some(bin) = env.which("claude") {
            return ToolDetection {
                installed: true,
                evidence: format!("found `claude` binary at {}", bin.display()),
                config_dir: None,
            };
        }
        ToolDetection {
            installed: false,
            evidence: "no ~/.claude directory and no `claude` binary on PATH".into(),
            config_dir: None,
        }
    }

    fn global_skill_locations(&self, env: &EnvContext, over: &ToolOverride) -> Vec<SkillLocation> {
        if let Some(path) = override_path(over, env) {
            return vec![SkillLocation {
                tool_id: self.id().into(),
                scope: crate::skill::SkillScope::Global,
                path,
                kind: LocationKind::Standard,
                overridden: true,
            }];
        }
        vec![SkillLocation {
            tool_id: self.id().into(),
            scope: crate::skill::SkillScope::Global,
            path: env.home_relative(&[".claude", "skills"]),
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
        SymlinkSupport::Preferred
    }

    fn reload_guidance(&self) -> ReloadGuidance {
        ReloadGuidance {
            summary: "Changes are detected automatically".into(),
            detail: "Claude Code reads personal skills (~/.claude/skills) per session and \
                     generally picks up changes on the next prompt; start a new session if a \
                     newly installed skill does not appear."
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
        contract::run_contract_tests(&ClaudeAdapter);
    }

    #[test]
    fn default_location_is_claude_skills() {
        let tmp = tempfile::tempdir().unwrap();
        let env = contract::isolated_env(tmp.path());
        let locations = ClaudeAdapter.global_skill_locations(&env, &ToolOverride::default());
        assert_eq!(locations.len(), 1);
        assert_eq!(locations[0].path, tmp.path().join(".claude").join("skills"));
        assert!(!locations[0].overridden);
    }
}
