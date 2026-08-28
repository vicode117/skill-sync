//! Adapter for **Cursor**.
//!
//! Documented locations (2026-08): user-level skills are read from
//! `~/.cursor/skills` and the agent-standard `~/.agents/skills` (plus the
//! Claude/Codex compatibility directories). SkillSync models the first two;
//! the compatibility locations belong to their own adapters to avoid
//! double-counting the same skills.
//!
//! Because Cursor natively reads `~/.agents/skills`, a canonical store at
//! the default location is recognized as `NativeShared` — nothing to sync.

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

pub struct CursorAdapter;

impl ToolAdapter for CursorAdapter {
    fn id(&self) -> &'static str {
        "cursor"
    }

    fn display_name(&self) -> &'static str {
        "Cursor"
    }

    fn detect(&self, env: &EnvContext) -> ToolDetection {
        let config_dir = env.home_relative(&[".cursor"]);
        if config_dir.is_dir() {
            return ToolDetection {
                installed: true,
                evidence: format!("found config directory {}", config_dir.display()),
                config_dir: Some(config_dir),
            };
        }
        if let Some(bin) = env.which("cursor").or_else(|| env.which("cursor-agent")) {
            return ToolDetection {
                installed: true,
                evidence: format!("found Cursor binary at {}", bin.display()),
                config_dir: None,
            };
        }
        ToolDetection {
            installed: false,
            evidence: "no ~/.cursor directory and no `cursor` binary on PATH".into(),
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
        vec![
            SkillLocation {
                tool_id: self.id().into(),
                scope: SkillScope::Global,
                path: env.home_relative(&[".cursor", "skills"]),
                kind: LocationKind::Standard,
                overridden: false,
            },
            SkillLocation {
                tool_id: self.id().into(),
                scope: SkillScope::Global,
                path: env.home_relative(&[".agents", "skills"]),
                kind: LocationKind::AgentStandard,
                overridden: false,
            },
        ]
    }

    fn scan_skills(
        &self,
        env: &EnvContext,
        over: &ToolOverride,
        canonical_root: &Path,
    ) -> Result<Vec<ScannedSkill>> {
        let mut all = Vec::new();
        let mut seen = std::collections::BTreeSet::new();
        for location in self.global_skill_locations(env, over) {
            // The same directory can be reachable from two configured
            // locations (override pointing at the agents-standard path).
            if !seen.insert(
                location
                    .path
                    .canonicalize()
                    .unwrap_or_else(|_| location.path.clone()),
            ) {
                continue;
            }
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
            detail: "Cursor loads skills from ~/.cursor/skills and ~/.agents/skills when a \
                     session starts; restart the agent session to pick up newly installed \
                     skills."
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
        contract::run_contract_tests(&CursorAdapter);
    }

    #[test]
    fn includes_agents_standard_location() {
        let tmp = tempfile::tempdir().unwrap();
        let env = contract::isolated_env(tmp.path());
        let locations = CursorAdapter.global_skill_locations(&env, &ToolOverride::default());
        assert_eq!(locations.len(), 2);
        assert_eq!(locations[0].path, tmp.path().join(".cursor").join("skills"));
        assert_eq!(locations[0].kind, LocationKind::Standard);
        assert_eq!(locations[1].path, tmp.path().join(".agents").join("skills"));
        assert_eq!(locations[1].kind, LocationKind::AgentStandard);
    }

    #[test]
    fn deduplicates_locations_pointing_at_the_same_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let env = contract::isolated_env(tmp.path());
        let agents = tmp.path().join(".agents").join("skills");
        std::fs::create_dir_all(&agents).unwrap();
        let over = ToolOverride {
            global_skill_path: Some(agents.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let locations = CursorAdapter.global_skill_locations(&env, &over);
        assert_eq!(locations.len(), 1);
        assert!(locations[0].overridden);
    }
}
