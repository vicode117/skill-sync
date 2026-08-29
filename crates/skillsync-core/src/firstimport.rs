//! First import (design doc §7h, prompt §19, §56, §57).
//!
//! When SkillSync is first used, every supported tool's skill directory is
//! scanned and the observed skills are classified by content:
//!
//! - **already canonical** — an identical skill exists in the store;
//! - **imports** — content-unique skills, one proposed entry each (the
//!   tool occurrence is just the source; importing copies into the store
//!   and never modifies tool directories);
//! - **conflicts** — same name, different content (§21: never merged
//!   automatically, listed for explicit resolution).
//!
//! The plan is presented before anything is applied (§56 step 6); applying
//! reuses the Slice-2 import machinery, so existing canonical content is
//! never overwritten and dry-run writes nothing.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::env::{abbreviate_home, EnvContext};
use crate::error::Result;
use crate::scan::ScannedSkill;
use crate::skill::Skill;
use crate::store::ConflictResolution;

/// Aggregated counts for the plan summary (§57).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportCounts {
    pub unique: usize,
    pub exact_duplicates: usize,
    pub conflicts: usize,
    pub already_canonical: usize,
}

/// One proposed import: a content-unique skill with its source occurrence.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedImport {
    pub skill_name: String,
    pub source_tool_id: String,
    pub source_path: PathBuf,
    pub source_display: String,
    pub target: PathBuf,
    pub target_display: String,
    pub fingerprint: Option<String>,
}

/// One same-name/different-content group needing an explicit decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportConflict {
    pub skill_name: String,
    /// Distinct content variants observed (per tool).
    pub occurrences: Vec<ImportConflictOccurrence>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportConflictOccurrence {
    pub tool_id: String,
    pub path: PathBuf,
    pub display_path: String,
    pub fingerprint: Option<String>,
}

/// The complete, previewable first-import plan.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirstImportPlan {
    pub canonical_root: PathBuf,
    pub canonical_root_display: String,
    pub counts: ImportCounts,
    pub imports: Vec<PlannedImport>,
    pub conflicts: Vec<ImportConflict>,
    pub notes: Vec<String>,
}

/// Report of applying the plan (§59: exactly what happened).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FirstImportReport {
    pub dry_run: bool,
    pub imported: Vec<String>,
    /// Entries that could not be imported, with the reason.
    pub skipped: Vec<SkippedImport>,
    pub failed: Vec<FailedImport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedImport {
    pub skill_name: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedImport {
    pub skill_name: String,
    pub error: String,
}

/// Classify all observed skills against the canonical store. Read-only.
pub fn plan_first_import(
    env: &EnvContext,
    canonical_root: &std::path::Path,
    canonical_skills: &[Skill],
    scanned: &[ScannedSkill],
    tool_names: &[(String, String)],
) -> FirstImportPlan {
    let configured_root = canonical_root.to_path_buf();
    let resolved_root = canonical_root
        .canonicalize()
        .unwrap_or_else(|_| configured_root.clone());

    // A canonical skill is already present when content matches (§19).
    let canonical_fps: Vec<Option<&str>> = canonical_skills
        .iter()
        .map(|s| s.fingerprint.as_deref())
        .collect();
    let already_canonical =
        |fp: Option<&str>| fp.is_some() && canonical_fps.iter().any(|c| *c == Some(fp.unwrap()));

    // Group unmanaged observations by content fingerprint (§21: names
    // alone never decide identity).
    let mut groups: std::collections::BTreeMap<String, Vec<&ScannedSkill>> =
        std::collections::BTreeMap::new();
    for scanned_skill in scanned {
        if !matches!(
            scanned_skill.managedness,
            crate::scan::Managedness::Unmanaged
        ) {
            continue;
        }
        let key = scanned_skill
            .fingerprint
            .clone()
            .unwrap_or_else(|| format!("path:{}", scanned_skill.path.display()));
        groups.entry(key).or_default().push(scanned_skill);
    }

    let mut counts = ImportCounts::default();
    let mut imports = Vec::new();
    let mut conflicts: Vec<ImportConflict> = Vec::new();
    let mut notes = Vec::new();

    // Name → distinct fingerprints across groups (for conflict detection).
    let mut names: std::collections::BTreeMap<String, Vec<String>> =
        std::collections::BTreeMap::new();

    let mut group_entries: Vec<(&String, &Vec<&ScannedSkill>)> = groups.iter().collect();
    group_entries.sort_by(|a, b| {
        let an =
            a.1.first()
                .map(|s| s.display_name.to_lowercase())
                .unwrap_or_default();
        let bn =
            b.1.first()
                .map(|s| s.display_name.to_lowercase())
                .unwrap_or_default();
        an.cmp(&bn).then(a.0.cmp(b.0))
    });

    for (key, occurrences) in group_entries {
        let primary = occurrences[0];
        if occurrences.len() > 1 {
            counts.exact_duplicates += occurrences.len() - 1;
        }
        let fp = primary.fingerprint.clone();
        if already_canonical(fp.as_deref()) {
            counts.already_canonical += 1;
            continue;
        }
        names
            .entry(primary.display_name.clone())
            .or_default()
            .push(key.clone());

        let target = configured_root.join(&primary.id);
        let target_free = !target.exists();
        let name_taken_by_other_content = canonical_skills
            .iter()
            .any(|s| s.id == primary.id && s.fingerprint != primary.fingerprint);

        if !target_free || name_taken_by_other_content {
            // Same name, different content: conflict, never auto-import.
            counts.conflicts += 1;
            conflicts.push(ImportConflict {
                skill_name: primary.display_name.clone(),
                occurrences: occurrences
                    .iter()
                    .map(|s| ImportConflictOccurrence {
                        tool_id: s.tool_id.clone(),
                        path: s.path.clone(),
                        display_path: abbreviate_home(&s.path, env),
                        fingerprint: s.fingerprint.clone(),
                    })
                    .chain(
                        canonical_skills
                            .iter()
                            .filter(|s| s.id == primary.id)
                            .map(|s| ImportConflictOccurrence {
                                tool_id: "canonical".into(),
                                path: s.root.clone(),
                                display_path: abbreviate_home(&s.root, env),
                                fingerprint: s.fingerprint.clone(),
                            }),
                    )
                    .collect(),
            });
            continue;
        }

        counts.unique += 1;
        let tool_display = tool_names
            .iter()
            .find(|(id, _)| *id == primary.tool_id)
            .map(|(_, name)| name.clone())
            .unwrap_or_else(|| primary.tool_id.clone());
        notes.push(format!(
            "`{}` found only in {tool_display}; importing will not change the tool directory",
            primary.display_name
        ));
        imports.push(PlannedImport {
            skill_name: primary.display_name.clone(),
            source_tool_id: primary.tool_id.clone(),
            source_path: primary.path.clone(),
            source_display: abbreviate_home(&primary.path, env),
            target: target.clone(),
            target_display: abbreviate_home(&target, env),
            fingerprint: primary.fingerprint.clone(),
        });
    }

    // Names shared by several distinct content groups are conflicts too
    // (§21): drop them from imports and add to the conflict list.
    let multi_name: Vec<String> = names
        .into_iter()
        .filter(|(_, keys)| keys.len() > 1)
        .map(|(name, _)| name)
        .collect();
    for name in &multi_name {
        let before = imports.len();
        imports.retain(|p| p.skill_name != *name);
        counts.unique -= before - imports.len();
        if !conflicts.iter().any(|c| &c.skill_name == name) {
            counts.conflicts += 1;
            let occurrences = scanned
                .iter()
                .filter(|s| {
                    s.display_name == *name
                        && matches!(s.managedness, crate::scan::Managedness::Unmanaged)
                })
                .map(|s| ImportConflictOccurrence {
                    tool_id: s.tool_id.clone(),
                    path: s.path.clone(),
                    display_path: abbreviate_home(&s.path, env),
                    fingerprint: s.fingerprint.clone(),
                })
                .chain(canonical_skills.iter().filter(|s| s.id == *name).map(|s| {
                    ImportConflictOccurrence {
                        tool_id: "canonical".into(),
                        path: s.root.clone(),
                        display_path: abbreviate_home(&s.root, env),
                        fingerprint: s.fingerprint.clone(),
                    }
                }))
                .collect();
            conflicts.push(ImportConflict {
                skill_name: name.clone(),
                occurrences,
            });
        }
    }

    FirstImportPlan {
        canonical_root: resolved_root,
        canonical_root_display: abbreviate_home(&configured_root, env),
        counts,
        imports,
        conflicts,
        notes,
    }
}

/// Apply the plan by reusing the Slice-2 import machinery: every entry is
/// re-planned against the live store (so anything that changed in between
/// is skipped, never overwritten), then executed. Dry-run writes nothing.
pub fn apply_first_import(
    app: &crate::SkillSync,
    plan: &FirstImportPlan,
    dry_run: bool,
) -> Result<FirstImportReport> {
    let mut report = FirstImportReport {
        dry_run,
        ..Default::default()
    };

    for entry in &plan.imports {
        let outcome = app
            .plan_import(&entry.source_path, ConflictResolution::Skip)
            .and_then(|import_plan| app.execute_import(&import_plan, dry_run));
        match outcome {
            Ok(o) => match o.action_taken.kind_label() {
                "create" | "keepBoth" => report.imported.push(entry.skill_name.clone()),
                "alreadyPresent" => report.skipped.push(SkippedImport {
                    skill_name: entry.skill_name.clone(),
                    reason: "identical skill already in the canonical store".into(),
                }),
                kind => report.skipped.push(SkippedImport {
                    skill_name: entry.skill_name.clone(),
                    reason: format!("unexpected plan action `{kind}`"),
                }),
            },
            Err(err) if err.code == crate::error::ErrorCode::TargetConflict => {
                report.skipped.push(SkippedImport {
                    skill_name: entry.skill_name.clone(),
                    reason: "canonical skill appeared with different content; resolve explicitly"
                        .into(),
                });
            }
            Err(err) => report.failed.push(FailedImport {
                skill_name: entry.skill_name.clone(),
                error: err.message,
            }),
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::env::EnvContext;
    use crate::scan::{scan_skills_root, Managedness};
    use crate::skill::{SkillScope, SkillSource};

    const V1: &[u8] = b"---\nname: tdd\ndescription: v1\n---\n# v1\n";
    const V2: &[u8] = b"---\nname: tdd\ndescription: v2\n---\n# v2\n";
    const OTHER: &[u8] = b"---\nname: git-commit\ndescription: other\n---\n# other\n";

    fn write(root: &std::path::Path, tool: &str, name: &str, body: &[u8]) {
        let dir = root.join(format!(".{tool}")).join("skills").join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), body).unwrap();
    }

    fn rig() -> (tempfile::TempDir, EnvContext, crate::config::AppPaths) {
        let tmp = tempfile::tempdir().unwrap();
        let mut env = EnvContext::with_home(tmp.path().join("home"));
        env.env.insert("PATH".into(), String::new());
        let paths = crate::config::AppPaths {
            home: tmp.path().join("sync-home"),
        };
        (tmp, env, paths)
    }

    #[test]
    fn classifies_unique_duplicates_and_conflicts() {
        let (_tmp, env, _paths) = rig();
        let canonical = env.home.join(".agents").join("skills");
        std::fs::create_dir_all(&canonical).unwrap();
        // tdd: identical in claude + codex (exact duplicate, import once);
        // tdd different in gemini (conflict); git-commit: unique in codex.
        write(&env.home, "claude", "tdd", V1);
        write(&env.home, "codex", "tdd", V1);
        write(&env.home, "gemini", "tdd", V2);
        write(&env.home, "codex", "git-commit", OTHER);

        let mut scanned = Vec::new();
        for tool in ["claude", "codex", "gemini"] {
            let dir = env.home.join(format!(".{tool}")).join("skills");
            scanned.extend(
                scan_skills_root(&env, tool, &dir, &canonical, SkillScope::Global).unwrap(),
            );
        }

        let plan = plan_first_import(&env, &canonical, &[], &scanned, &[]);
        assert_eq!(plan.counts.unique, 1, "{:?}", plan);
        assert_eq!(plan.counts.exact_duplicates, 1);
        assert_eq!(plan.counts.conflicts, 1);
        assert_eq!(plan.imports.len(), 1);
        assert_eq!(plan.imports[0].skill_name, "git-commit");
        assert_eq!(plan.imports[0].source_tool_id, "codex");
        assert_eq!(plan.conflicts[0].skill_name, "tdd");
        assert_eq!(plan.conflicts[0].occurrences.len(), 3);
    }

    #[test]
    fn apply_imports_without_touching_tool_directories() {
        let (_tmp, env, _paths) = rig();
        let canonical = env.home.join(".agents").join("skills");
        write(&env.home, "claude", "tdd", V1);
        let mut scanned = scan_skills_root(
            &env,
            "claude",
            &env.home.join(".claude").join("skills"),
            &canonical,
            SkillScope::Global,
        )
        .unwrap();
        assert_eq!(scanned.len(), 1);
        assert_eq!(scanned[0].managedness, Managedness::Unmanaged);
        let _ = &mut scanned;

        let mut app = crate::SkillSync::with_environment(env.clone());
        let config = Config {
            canonical_skill_root: canonical.to_string_lossy().into_owned(),
            ..Default::default()
        };
        app.save_config(config.clone()).unwrap();

        let plan = plan_first_import(&env, &canonical, &[], &scanned, &[]);
        assert_eq!(plan.imports.len(), 1);

        // Dry run: nothing written.
        let report = apply_first_import(&app, &plan, true).unwrap();
        assert!(report.dry_run);
        assert!(!canonical.join("tdd").exists());

        // Real run: skill lands in the store, tool dir untouched.
        let report = apply_first_import(&app, &plan, false).unwrap();
        assert_eq!(report.imported, vec!["tdd".to_string()]);
        assert!(canonical.join("tdd").join("SKILL.md").is_file());
        assert!(env
            .home
            .join(".claude")
            .join("skills")
            .join("tdd")
            .join("SKILL.md")
            .is_file());

        // Re-planning now reports already-canonical instead of duplicating.
        let scanned2 = scan_skills_root(
            &env,
            "claude",
            &env.home.join(".claude").join("skills"),
            &canonical,
            SkillScope::Global,
        )
        .unwrap();
        let canonical_skills = app.canonical_skills().unwrap();
        assert_eq!(canonical_skills.len(), 1);
        let plan2 = plan_first_import(&env, &canonical, &canonical_skills, &scanned2, &[]);
        assert_eq!(plan2.counts.already_canonical, 1, "{plan2:?}");
        assert!(plan2.imports.is_empty());
        let _ = config;
    }

    #[test]
    fn apply_never_overwrites_existing_canonical_content() {
        let (_tmp, env, _paths) = rig();
        let canonical = env.home.join(".agents").join("skills");
        write(&env.home, "claude", "tdd", V2);
        // Canonical already has a DIFFERENT tdd.
        let cdir = canonical.join("tdd");
        std::fs::create_dir_all(&cdir).unwrap();
        std::fs::write(cdir.join("SKILL.md"), V1).unwrap();

        let canonical_skill =
            crate::scan::inspect_as_skill(&env, &cdir, SkillScope::Global, SkillSource::Canonical)
                .unwrap();
        let scanned = scan_skills_root(
            &env,
            "claude",
            &env.home.join(".claude").join("skills"),
            &canonical,
            SkillScope::Global,
        )
        .unwrap();

        let mut app = crate::SkillSync::with_environment(env.clone());
        app.save_config(Config {
            canonical_skill_root: canonical.to_string_lossy().into_owned(),
            ..Default::default()
        })
        .unwrap();

        let plan = plan_first_import(&env, &canonical, &[canonical_skill], &scanned, &[]);
        assert_eq!(plan.counts.conflicts, 1);
        assert!(plan.imports.is_empty(), "{:?}", plan.imports);

        let report = apply_first_import(&app, &plan, false).unwrap();
        assert!(report.imported.is_empty());
        // Canonical content untouched.
        assert_eq!(std::fs::read(cdir.join("SKILL.md")).unwrap(), V1);
    }
}
