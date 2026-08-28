//! Aggregated overview: merges canonical-store skills with observed
//! installations into per-skill rows with sync states (design doc §17,
//! §25, §60 observed-vs-desired model).
//!
//! This is the single implementation used by both the CLI and the GUI.
//! Slice 1 derives only what read-only discovery can know: `Native`,
//! `Synced`, `NotInstalled`, `Unmanaged`, `Unavailable`. The `Modified`,
//! `Conflict` and `Disabled` states require the canonical store and
//! enablement model (Slices 2–5) and are defined but not yet produced.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::adapter::{LocationKind, SkillLocation, SymlinkSupport, ToolAdapter, ToolDetection};
use crate::config::Config;
use crate::env::{abbreviate_home, EnvContext};
use crate::scan::{Managedness, ScannedSkill};
use crate::skill::{SkillSource, ValidationIssue};

/// Per Skill×Tool state (§17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SyncState {
    /// The tool reads the canonical store directly — nothing to install.
    Native,
    /// A managed installation is present and current.
    Synced,
    /// Desired for this tool but not installed (derived once enablement
    /// exists; in Slice 1: canonical skill, detected tool, no installation).
    NotInstalled,
    /// Disabled by the user for this tool.
    Disabled,
    /// A managed copy drifted from the canonical skill (copy mode).
    Modified,
    /// Canonical and target content both exist and differ.
    Conflict,
    /// Present in the tool directory but not controlled by SkillSync.
    Unmanaged,
    /// A managed installation is broken (e.g. dead symlink).
    Unavailable,
}

/// One observed (or absent-but-expected) installation of a skill in a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Installation {
    pub tool_id: String,
    pub tool_display_name: String,
    pub path: PathBuf,
    /// `~`-abbreviated path for display.
    pub display_path: String,
    pub state: SyncState,
    pub managedness: Managedness,
    pub fingerprint: Option<String>,
    pub validation: Vec<ValidationIssue>,
}

/// The canonical side of a row, when the skill has been adopted.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CanonicalInfo {
    pub path: PathBuf,
    pub display_path: String,
    pub fingerprint: Option<String>,
    pub validation: Vec<ValidationIssue>,
}

/// One row in the skill list / matrix: one skill, all its installations.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillRow {
    /// Stable key: canonical dir name, or fingerprint, or synthetic id.
    pub key: String,
    pub name: String,
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub canonical: Option<CanonicalInfo>,
    pub installations: Vec<Installation>,
    /// Aggregated row status (worst relevant state, see module docs).
    pub status: SyncState,
}

/// Per-location detail for the tools page (§53).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationInfo {
    pub path: PathBuf,
    pub display_path: String,
    pub kind: LocationKind,
    pub overridden: bool,
    pub exists: bool,
    /// The location is the canonical store itself.
    pub native_canonical: bool,
    pub skill_count: usize,
    pub managed_count: usize,
    pub unmanaged_count: usize,
}

/// A detected tool and its capabilities (§53).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolInfo {
    pub id: String,
    pub display_name: String,
    pub detection: ToolDetection,
    pub enabled: bool,
    pub locations: Vec<LocationInfo>,
    pub symlink_support: SymlinkSupport,
    pub reload_guidance: crate::adapter::ReloadGuidance,
    pub skill_count: usize,
    pub managed_count: usize,
}

/// The complete read-only overview produced by one scan pass.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillOverview {
    pub canonical_root: PathBuf,
    pub canonical_root_display: String,
    pub canonical_root_exists: bool,
    pub tools: Vec<ToolInfo>,
    pub rows: Vec<SkillRow>,
}

pub struct OverviewBuilder<'a> {
    pub env: &'a EnvContext,
    pub config: &'a Config,
    pub canonical_root: &'a Path,
}

impl<'a> OverviewBuilder<'a> {
    /// Build tool info for one adapter. `scanned` is the adapter's scan
    /// result (may already have been produced once and is reused).
    pub fn tool_info(&self, adapter: &dyn ToolAdapter, scanned: &[ScannedSkill]) -> ToolInfo {
        let over = self.config.tool(adapter.id()).cloned().unwrap_or_default();
        let locations = adapter.global_skill_locations(self.env, &over);
        let canonical_canonical = self.canonical_root.canonicalize().ok();

        let infos = locations
            .iter()
            .map(|loc: &SkillLocation| {
                let loc_canonical = loc.path.canonicalize().ok();
                let native_canonical = matches!(
                    (&loc_canonical, &canonical_canonical),
                    (Some(l), Some(c)) if l == c
                );
                let in_loc: Vec<&ScannedSkill> = scanned
                    .iter()
                    .filter(|s| s.location_path == loc.path)
                    .collect();
                let managed = in_loc
                    .iter()
                    .filter(|s| {
                        matches!(
                            s.managedness,
                            Managedness::ManagedSymlink { .. } | Managedness::NativeShared
                        )
                    })
                    .count();
                LocationInfo {
                    display_path: abbreviate_home(&loc.path, self.env),
                    exists: loc.path.is_dir(),
                    skill_count: in_loc.len(),
                    managed_count: managed,
                    unmanaged_count: in_loc.len() - managed,
                    native_canonical,
                    path: loc.path.clone(),
                    kind: loc.kind,
                    overridden: loc.overridden,
                }
            })
            .collect();

        let detection = adapter.detect(self.env);
        ToolInfo {
            id: adapter.id().to_string(),
            display_name: adapter.display_name().to_string(),
            enabled: self.config.is_tool_enabled(adapter.id()),
            detection,
            locations: infos,
            symlink_support: adapter.symlink_support(),
            reload_guidance: adapter.reload_guidance(),
            skill_count: scanned.len(),
            managed_count: scanned
                .iter()
                .filter(|s| {
                    matches!(
                        s.managedness,
                        Managedness::ManagedSymlink { .. } | Managedness::NativeShared
                    )
                })
                .count(),
        }
    }

    /// Merge canonical skills and observed installations into rows.
    pub fn build_rows(
        &self,
        canonical_skills: &[crate::skill::Skill],
        scanned: &[ScannedSkill],
        tools: &[ToolInfo],
    ) -> Vec<SkillRow> {
        let mut rows: Vec<SkillRow> = Vec::new();
        let mut by_key: BTreeMap<String, usize> = BTreeMap::new();

        // Canonical skills first — they are the source of truth.
        for skill in canonical_skills {
            let key = skill.id.clone();
            by_key.insert(key.clone(), rows.len());
            rows.push(SkillRow {
                key: key.clone(),
                name: skill.display_name.clone(),
                description: skill.description.clone(),
                canonical: Some(CanonicalInfo {
                    display_path: abbreviate_home(&skill.root, self.env),
                    path: skill.root.clone(),
                    fingerprint: skill.fingerprint.clone(),
                    validation: skill.validation.clone(),
                }),
                installations: Vec::new(),
                status: SyncState::NotInstalled,
            });
        }

        let canonical_by_id: BTreeMap<&str, &crate::skill::Skill> = canonical_skills
            .iter()
            .map(|s| (s.id.as_str(), s))
            .collect();
        let canonical_by_fingerprint: BTreeMap<&str, &crate::skill::Skill> = canonical_skills
            .iter()
            .filter_map(|s| s.fingerprint.as_deref().map(|f| (f, s)))
            .collect();

        // Match observed installations to rows (§21: never merge by name
        // alone — matching uses ownership signals and content fingerprints).
        let mut unmatched: Vec<&ScannedSkill> = Vec::new();
        for scanned_skill in scanned {
            let installation_for = |state: SyncState| Installation {
                tool_id: scanned_skill.tool_id.clone(),
                tool_display_name: tools
                    .iter()
                    .find(|t| t.id == scanned_skill.tool_id)
                    .map(|t| t.display_name.clone())
                    .unwrap_or_else(|| scanned_skill.tool_id.clone()),
                path: scanned_skill.path.clone(),
                display_path: abbreviate_home(&scanned_skill.path, self.env),
                state,
                managedness: scanned_skill.managedness.clone(),
                fingerprint: scanned_skill.fingerprint.clone(),
                validation: scanned_skill.validation.clone(),
            };

            match &scanned_skill.managedness {
                Managedness::ManagedSymlink { canonical_path } => {
                    // Ownership signal: the link resolves into the canonical
                    // store; match by the linked directory.
                    let target_id = canonical_path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned());
                    let by_id = target_id.as_deref().and_then(|id| canonical_by_id.get(id));
                    let matched = match by_id {
                        Some(skill) => {
                            let row_idx = by_key[&skill.id.clone()];
                            rows[row_idx]
                                .installations
                                .push(installation_for(SyncState::Synced));
                            true
                        }
                        None => {
                            // Fall back to content match against the link.
                            let by_fp = scanned_skill
                                .fingerprint
                                .as_deref()
                                .and_then(|f| canonical_by_fingerprint.get(f));
                            if let Some(skill) = by_fp {
                                let row_idx = by_key[&skill.id.clone()];
                                rows[row_idx]
                                    .installations
                                    .push(installation_for(SyncState::Synced));
                                true
                            } else {
                                false
                            }
                        }
                    };
                    if !matched {
                        unmatched.push(scanned_skill);
                    }
                }
                Managedness::NativeShared => {
                    // The scanned location IS the canonical store: match by
                    // exact path, then fingerprint.
                    let by_path = canonical_skills
                        .iter()
                        .find(|s| s.root == scanned_skill.path);
                    let matched = match by_path {
                        Some(skill) => {
                            let row_idx = by_key[&skill.id.clone()];
                            rows[row_idx]
                                .installations
                                .push(installation_for(SyncState::Native));
                            true
                        }
                        None => {
                            let by_fp = scanned_skill
                                .fingerprint
                                .as_deref()
                                .and_then(|f| canonical_by_fingerprint.get(f));
                            if let Some(skill) = by_fp {
                                let row_idx = by_key[&skill.id.clone()];
                                rows[row_idx]
                                    .installations
                                    .push(installation_for(SyncState::Native));
                                true
                            } else {
                                false
                            }
                        }
                    };
                    if !matched {
                        unmatched.push(scanned_skill);
                    }
                }
                Managedness::Unmanaged | Managedness::ForeignSymlink { .. } => {
                    // Content-identical duplicates belong to the canonical
                    // row (import candidates); different content is its own
                    // skill even when the name matches (§21).
                    let by_fp = scanned_skill
                        .fingerprint
                        .as_deref()
                        .and_then(|f| canonical_by_fingerprint.get(f));
                    if let Some(skill) = by_fp {
                        let row_idx = by_key[&skill.id.clone()];
                        rows[row_idx]
                            .installations
                            .push(installation_for(SyncState::Unmanaged));
                    } else {
                        unmatched.push(scanned_skill);
                    }
                }
                Managedness::BrokenSymlink => unreachable!("broken symlinks are never surfaced"),
            }
        }

        // Unmatched observed skills: group by fingerprint so the same skill
        // installed identically in several tools shows as one row.
        let mut fp_groups: BTreeMap<String, usize> = BTreeMap::new();
        for skill in unmatched {
            let key = match &skill.fingerprint {
                Some(fp) => format!("fp:{fp}"),
                None => format!("path:{}", skill.path.display()),
            };
            let idx = match fp_groups.get(&key) {
                Some(i) => *i,
                None => {
                    rows.push(SkillRow {
                        key: key.clone(),
                        name: skill.display_name.clone(),
                        description: skill.description.clone(),
                        canonical: None,
                        installations: Vec::new(),
                        status: SyncState::Unmanaged,
                    });
                    let new_idx = rows.len() - 1;
                    fp_groups.insert(key, new_idx);
                    new_idx
                }
            };
            let tool_name = tools
                .iter()
                .find(|t| t.id == skill.tool_id)
                .map(|t| t.display_name.clone())
                .unwrap_or_else(|| skill.tool_id.clone());
            rows[idx].installations.push(Installation {
                tool_id: skill.tool_id.clone(),
                tool_display_name: tool_name,
                path: skill.path.clone(),
                display_path: abbreviate_home(&skill.path, self.env),
                state: SyncState::Unmanaged,
                managedness: skill.managedness.clone(),
                fingerprint: skill.fingerprint.clone(),
                validation: skill.validation.clone(),
            });
        }

        // Add NotInstalled placeholders for detected, enabled tools that
        // have no installation of a canonical skill (matrix `-`).
        for row in &mut rows {
            if row.canonical.is_none() {
                continue;
            }
            for tool in tools {
                if !tool.enabled || !tool.detection.installed {
                    continue;
                }
                if !row.installations.iter().any(|i| i.tool_id == tool.id) {
                    row.installations.push(Installation {
                        tool_id: tool.id.clone(),
                        tool_display_name: tool.display_name.clone(),
                        path: PathBuf::new(),
                        display_path: String::new(),
                        state: SyncState::NotInstalled,
                        managedness: Managedness::Unmanaged, // placeholder; state is NotInstalled
                        fingerprint: None,
                        validation: Vec::new(),
                    });
                }
            }
        }

        // Aggregate row status (see module docs for the Slice 1 rule).
        for row in &mut rows {
            row.status = aggregate_status(row);
            row.installations.sort_by(|a, b| a.tool_id.cmp(&b.tool_id));
        }
        rows.sort_by_key(|r| r.name.to_lowercase());
        rows
    }
}

fn aggregate_status(row: &SkillRow) -> SyncState {
    let states: Vec<SyncState> = row.installations.iter().map(|i| i.state).collect();
    if row.canonical.is_some() {
        // Health of the canonical → tools relationship.
        if states.contains(&SyncState::Unavailable) {
            SyncState::Unavailable
        } else if states.iter().any(|s| {
            matches!(
                s,
                SyncState::Synced | SyncState::Native | SyncState::Modified
            )
        }) {
            SyncState::Synced
        } else {
            SyncState::NotInstalled
        }
    } else {
        SyncState::Unmanaged
    }
}

/// The canonical source of a skill row for display purposes.
pub fn row_source_label(row: &SkillRow) -> String {
    match &row.canonical {
        Some(c) => c.display_path.clone(),
        None => row
            .installations
            .first()
            .map(|i| i.display_path.clone())
            .unwrap_or_default(),
    }
}

/// Convenience so callers can iterate `SkillSource` in UIs.
pub fn skill_source_label(source: &SkillSource) -> String {
    match source {
        SkillSource::Canonical => "canonical".into(),
        SkillSource::Observed { tool_id } => format!("observed ({tool_id})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env::EnvContext;

    fn installation(tool: &str, state: SyncState) -> Installation {
        Installation {
            tool_id: tool.into(),
            tool_display_name: tool.into(),
            path: PathBuf::from("/tmp/x"),
            display_path: "/tmp/x".into(),
            state,
            managedness: Managedness::Unmanaged,
            fingerprint: None,
            validation: Vec::new(),
        }
    }

    fn row(canonical: bool, installations: Vec<Installation>) -> SkillRow {
        SkillRow {
            key: "k".into(),
            name: "n".into(),
            description: None,
            canonical: canonical.then(|| CanonicalInfo {
                path: PathBuf::from("/tmp/c"),
                display_path: "/tmp/c".into(),
                fingerprint: None,
                validation: Vec::new(),
            }),
            installations,
            status: SyncState::NotInstalled,
        }
    }

    #[test]
    fn canonical_with_no_installations_is_not_installed() {
        let mut r = row(true, vec![]);
        r.status = aggregate_status(&r);
        assert_eq!(r.status, SyncState::NotInstalled);
    }

    #[test]
    fn canonical_with_synced_installation_is_synced() {
        let mut r = row(true, vec![installation("claude", SyncState::Synced)]);
        r.status = aggregate_status(&r);
        assert_eq!(r.status, SyncState::Synced);
    }

    #[test]
    fn broken_managed_installation_is_unavailable() {
        let mut r = row(true, vec![installation("claude", SyncState::Unavailable)]);
        r.status = aggregate_status(&r);
        assert_eq!(r.status, SyncState::Unavailable);
    }

    #[test]
    fn non_canonical_rows_are_unmanaged() {
        let mut r = row(false, vec![installation("codex", SyncState::Unmanaged)]);
        r.status = aggregate_status(&r);
        assert_eq!(r.status, SyncState::Unmanaged);
    }

    #[test]
    fn env_context_used_for_abbreviations() {
        let env = EnvContext::with_home("/Users/tester");
        assert_eq!(
            abbreviate_home(Path::new("/Users/tester/.claude/skills"), &env),
            "~/.claude/skills"
        );
    }
}
