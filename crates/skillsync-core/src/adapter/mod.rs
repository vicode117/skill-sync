//! Tool adapter boundary (design doc §4 of ARCHITECTURE.md).
//!
//! Every supported coding tool is implemented as a `ToolAdapter`. All
//! tool-specific knowledge — paths, detection, symlink semantics, reload
//! guidance — lives in the adapter module for that tool. Nothing outside
//! `adapter/` may branch on a tool id.

pub mod claude;
pub mod codex;
pub mod cursor;
pub mod gemini;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::ToolOverride;
use crate::env::EnvContext;
use crate::error::Result;
use crate::scan::ScannedSkill;
use crate::skill::{Skill, SkillScope, SkillSource};

/// How the tool handles directory symlinks in its skills root, driving the
/// `Auto` sync method's decision (§13, §44).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SymlinkSupport {
    /// Directory symlinks are first-class for this tool.
    Preferred,
    /// Works, with caveats the adapter documents.
    Supported,
    /// The tool does not follow symlinks during skill discovery — copies
    /// are the safe default (e.g. Gemini CLI).
    Avoided,
}

/// Adapter-owned reload guidance (§45). Never encoded globally.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReloadGuidance {
    /// Short summary, e.g. "Changes detected automatically".
    pub summary: String,
    /// Longer, tool-specific detail for the UI.
    pub detail: String,
}

/// Whether a location is the tool's own conventional directory or the
/// agent-standard `~/.agents/skills` location.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LocationKind {
    /// The tool's own convention (e.g. `~/.claude/skills`).
    Standard,
    /// The cross-tool `.agents/skills` convention.
    AgentStandard,
}

/// A concrete directory from which a tool discovers skills.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillLocation {
    pub tool_id: String,
    pub scope: SkillScope,
    /// Absolute path (already expanded).
    pub path: PathBuf,
    pub kind: LocationKind,
    /// True when the adapter's default was replaced by user config.
    pub overridden: bool,
}

impl SkillLocation {
    pub fn label(&self) -> String {
        match self.kind {
            LocationKind::Standard => self.path.to_string_lossy().into_owned(),
            LocationKind::AgentStandard => {
                format!("{} (agents-standard)", self.path.to_string_lossy())
            }
        }
    }
}

/// Result of `ToolAdapter::detect`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolDetection {
    pub installed: bool,
    /// Human-readable reason (config dir seen, binary on PATH, ...).
    pub evidence: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config_dir: Option<PathBuf>,
}

/// The read-only discovery surface every tool adapter implements.
///
/// Mutation (install/remove with destination-safety validation) is a
/// separate boundary added with Slice 3; nothing here may modify the
/// filesystem.
pub trait ToolAdapter: Send + Sync {
    /// Stable identifier used in config (`tools.<id>`) and CLI flags.
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;

    /// Whether the tool appears installed/configured on this machine.
    fn detect(&self, env: &EnvContext) -> ToolDetection;

    /// Known user-level skill locations (may be more than one, e.g. Cursor
    /// also reads `~/.agents/skills`). Overrides from config win.
    fn global_skill_locations(&self, env: &EnvContext, over: &ToolOverride) -> Vec<SkillLocation>;

    /// Known project-level skill locations for `project_root` (MVP: global
    /// first; adapters may return an empty list until project sync lands).
    fn project_skill_locations(
        &self,
        _env: &EnvContext,
        _over: &ToolOverride,
        _project_root: &Path,
    ) -> Vec<SkillLocation> {
        Vec::new()
    }

    /// Read-only scan of this tool's skill locations.
    fn scan_skills(
        &self,
        env: &EnvContext,
        over: &ToolOverride,
        canonical_root: &Path,
    ) -> Result<Vec<ScannedSkill>>;

    fn symlink_support(&self) -> SymlinkSupport;
    fn reload_guidance(&self) -> ReloadGuidance;
}

/// A skill observed under the tool's directory, re-exposed as a `Skill`
/// with an `Observed` source (helper for overview aggregation).
pub fn scanned_as_skill(scanned: &ScannedSkill) -> Skill {
    Skill {
        id: scanned.id.clone(),
        display_name: scanned.display_name.clone(),
        description: scanned.description.clone(),
        root: scanned.path.clone(),
        scope: scanned.scope,
        source: SkillSource::Observed {
            tool_id: scanned.tool_id.clone(),
        },
        files: scanned.files.clone(),
        fingerprint: scanned.fingerprint.clone(),
        frontmatter: scanned.frontmatter.clone(),
        validation: scanned.validation.clone(),
    }
}

/// The built-in adapter registry. Adding a tool = new module + one line here
/// (plus contract tests) — no other changes anywhere.
pub fn registry() -> Vec<std::sync::Arc<dyn ToolAdapter>> {
    vec![
        std::sync::Arc::new(claude::ClaudeAdapter),
        std::sync::Arc::new(codex::CodexAdapter),
        std::sync::Arc::new(cursor::CursorAdapter),
        std::sync::Arc::new(gemini::GeminiAdapter),
    ]
}

/// Shared helper: expand a possibly-`~`-prefixed override path.
fn override_path(over: &ToolOverride, env: &EnvContext) -> Option<PathBuf> {
    over.global_skill_path
        .as_deref()
        .map(|p| crate::env::expand_home(p, env))
}

/// Shared helper: scan one location and tag results with tool + scope.
fn scan_location(
    adapter_id: &str,
    location: &SkillLocation,
    env: &EnvContext,
    canonical_root: &Path,
) -> Result<Vec<ScannedSkill>> {
    crate::scan::scan_skills_root(
        env,
        adapter_id,
        &location.path,
        canonical_root,
        SkillScope::Global,
    )
}

#[cfg(test)]
pub(crate) mod contract {
    //! Adapter contract tests (design doc §70). Every adapter runs the same
    //! suite over a synthetic home directory.

    use super::*;
    use crate::config::ToolOverride;
    use std::path::{Path, PathBuf};

    pub fn run_contract_tests(adapter: &dyn ToolAdapter) {
        detect_missing_directories_safely(adapter);
        scan_valid_skill(adapter);
        ignore_unrelated_files(adapter);
        identify_managed_symlink(adapter);
        respect_manual_override(adapter);
    }

    fn sandbox(adapter_id: &str) -> (tempfile::TempDir, PathBuf, PathBuf, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        // A neutral canonical store location: intentionally NOT one of the
        // adapters' standard skill directories (Cursor natively reads
        // ~/.agents/skills), so contract assertions stay location-agnostic.
        let canonical = tmp.path().join("canonical-store");
        let tool_dir = home.join(format!(".{adapter_id}"));
        let tool_skills = tool_dir.join("skills");
        std::fs::create_dir_all(&canonical).unwrap();
        std::fs::create_dir_all(&tool_skills).unwrap();
        (tmp, home, canonical, tool_skills)
    }

    fn fixture_skill(dest: &Path, name: &str) -> PathBuf {
        let dir = dest.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Test skill {name}\n---\n# {name}\n"),
        )
        .unwrap();
        dir
    }

    /// A test environment with a synthetic home and an isolated PATH so
    /// host tool installations cannot influence the outcome.
    pub fn isolated_env(home: &Path) -> EnvContext {
        let mut env = EnvContext::with_home(home);
        env.env.insert("PATH".into(), String::new());
        env
    }

    fn detect_missing_directories_safely(adapter: &dyn ToolAdapter) {
        let tmp = tempfile::tempdir().unwrap();
        let env = isolated_env(&tmp.path().join("nothing-here"));
        let detection = adapter.detect(&env);
        // A missing tool directory is "not detected", never an error.
        assert!(
            !detection.installed,
            "{}: empty home should not be detected",
            adapter.id()
        );
        // Scanning a missing location yields no skills and no error.
        let skills = adapter
            .scan_skills(&env, &ToolOverride::default(), &tmp.path().join("c"))
            .unwrap();
        assert!(skills.is_empty(), "{}", adapter.id());
    }

    fn scan_valid_skill(adapter: &dyn ToolAdapter) {
        let (_tmp, home, canonical, tool_skills) = sandbox(adapter.id());
        fixture_skill(&tool_skills, "valid-skill");
        let env = isolated_env(&home);
        let detection = adapter.detect(&env);
        assert!(
            detection.installed,
            "{}: config dir should detect",
            adapter.id()
        );
        let skills = adapter
            .scan_skills(&env, &ToolOverride::default(), &canonical)
            .unwrap();
        assert_eq!(skills.len(), 1, "{}", adapter.id());
        assert_eq!(skills[0].display_name, "valid-skill", "{}", adapter.id());
        assert_eq!(
            skills[0].managedness,
            crate::scan::Managedness::Unmanaged,
            "{}",
            adapter.id()
        );
        assert!(skills[0].frontmatter.is_some(), "{}", adapter.id());
    }

    fn ignore_unrelated_files(adapter: &dyn ToolAdapter) {
        let (_tmp, home, canonical, tool_skills) = sandbox(adapter.id());
        fixture_skill(&tool_skills, "real-skill");
        std::fs::write(tool_skills.join("notes.txt"), b"not a skill").unwrap();
        std::fs::create_dir_all(tool_skills.join("empty-dir")).unwrap();
        std::fs::create_dir_all(tool_skills.join(".hidden")).unwrap();
        let env = isolated_env(&home);
        let skills = adapter
            .scan_skills(&env, &ToolOverride::default(), &canonical)
            .unwrap();
        assert_eq!(skills.len(), 1, "{}", adapter.id());
        assert_eq!(skills[0].id, "real-skill", "{}", adapter.id());
    }

    fn identify_managed_symlink(adapter: &dyn ToolAdapter) {
        let (_tmp, home, canonical, tool_skills) = sandbox(adapter.id());
        let canonical_skill = fixture_skill(&canonical, "managed-skill");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&canonical_skill, tool_skills.join("managed-skill")).unwrap();
        let env = isolated_env(&home);
        let skills = adapter
            .scan_skills(&env, &ToolOverride::default(), &canonical)
            .unwrap();
        assert_eq!(skills.len(), 1, "{}", adapter.id());
        match &skills[0].managedness {
            crate::scan::Managedness::ManagedSymlink { canonical_path } => {
                assert_eq!(
                    canonical_path.canonicalize().ok(),
                    canonical_skill.canonicalize().ok(),
                    "{}",
                    adapter.id()
                );
            }
            other => panic!("{}: expected managed symlink, got {other:?}", adapter.id()),
        }
    }

    fn respect_manual_override(adapter: &dyn ToolAdapter) {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let canonical = tmp.path().join("canonical");
        let custom = tmp.path().join("custom-location");
        std::fs::create_dir_all(&canonical).unwrap();
        std::fs::create_dir_all(&custom).unwrap();
        fixture_skill(&custom, "override-skill");

        let env = isolated_env(&home);
        let over = ToolOverride {
            global_skill_path: Some(custom.to_string_lossy().into_owned()),
            ..Default::default()
        };
        let locations = adapter.global_skill_locations(&env, &over);
        assert!(
            locations.iter().any(|l| l.path == custom && l.overridden),
            "{}: manual override not honored",
            adapter.id()
        );
        let skills = adapter.scan_skills(&env, &over, &canonical).unwrap();
        assert_eq!(skills.len(), 1, "{}", adapter.id());
        assert_eq!(skills[0].id, "override-skill", "{}", adapter.id());
    }
}
