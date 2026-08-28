//! Read-only scanning of a skills directory (design doc §19, §20, §22).
//!
//! The scanner is shared by all adapters: each adapter only decides *where*
//! to scan and how the tool treats symlinks. Ownership (`Managedness`) is
//! derived from what we observe — symlink targets and canonical-root
//! identity — never from directory names alone (design doc §28).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::env::EnvContext;
use crate::error::{ErrorCode, Result, SkillSyncError};
use crate::fingerprint::fingerprint_dir;
use crate::frontmatter;
use crate::skill::{
    Skill, SkillFileEntry, SkillFrontmatter, SkillScope, SkillSource, ValidationIssue,
    ValidationSeverity,
};

/// How the observed installation is physically materialized.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum InstallKind {
    /// A real directory on disk.
    Directory,
    /// A symlink pointing somewhere (target may be broken).
    Symlink { target: PathBuf },
}

/// Whether SkillSync owns this observed installation (§28).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum Managedness {
    /// A real directory SkillSync does not manage. Never touched silently.
    Unmanaged,
    /// A symlink resolving into the canonical skill store.
    ManagedSymlink { canonical_path: PathBuf },
    /// A symlink pointing somewhere else (another tool of the user's setup).
    ForeignSymlink { target: PathBuf },
    /// The scanned location *is* the canonical store — nothing to install.
    NativeShared,
    /// A symlink whose target no longer resolves.
    BrokenSymlink,
}

/// A skill observed inside a tool's skill location.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScannedSkill {
    pub tool_id: String,
    /// The skills root this skill was found in.
    pub location_path: PathBuf,
    /// Absolute path of the skill directory (or the symlink itself).
    pub path: PathBuf,
    /// Directory name.
    pub id: String,
    pub display_name: String,
    pub description: Option<String>,
    pub scope: SkillScope,
    pub install: InstallKind,
    pub managedness: Managedness,
    pub files: Vec<SkillFileEntry>,
    pub fingerprint: Option<String>,
    pub frontmatter: Option<SkillFrontmatter>,
    pub validation: Vec<ValidationIssue>,
}

impl ScannedSkill {
    pub fn has_errors(&self) -> bool {
        self.validation
            .iter()
            .any(|v| v.severity == ValidationSeverity::Error)
    }
}

/// Human-readable label for a managedness classification.
pub fn managedness_label(m: &Managedness) -> &'static str {
    match m {
        Managedness::Unmanaged => "unmanaged",
        Managedness::ManagedSymlink { .. } => "managed (symlink)",
        Managedness::ForeignSymlink { .. } => "foreign symlink",
        Managedness::NativeShared => "native (shared canonical store)",
        Managedness::BrokenSymlink => "broken symlink",
    }
}

/// Scan one skills root directory. Read-only: creates nothing, deletes
/// nothing, follows skill symlinks only to classify them.
///
/// `canonical_root` is used to classify `ManagedSymlink` vs
/// `ForeignSymlink` and to mark the location as `NativeShared` when it *is*
/// the canonical store.
pub fn scan_skills_root(
    _env: &EnvContext,
    tool_id: &str,
    location: &Path,
    canonical_root: &Path,
    scope: SkillScope,
) -> Result<Vec<ScannedSkill>> {
    let Ok(location_canonical) = location.canonicalize() else {
        // A missing (or unreadable) location simply has no skills — this is
        // the normal "tool not installed" case, not an error.
        return Ok(Vec::new());
    };
    let canonical_root = canonical_root
        .canonicalize()
        .unwrap_or_else(|_| canonical_root.to_path_buf());
    let native_shared = location_canonical == canonical_root;

    let mut skills = Vec::new();
    let entries = match std::fs::read_dir(location) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::PermissionDenied => {
            return Err(SkillSyncError::io(&err, location)
                .with_tool(tool_id)
                .recoverable())
        }
        Err(err) => return Err(SkillSyncError::io(&err, location).with_tool(tool_id)),
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(err) => return Err(SkillSyncError::io(&err, location)),
        };
        let path = entry.path();
        let Ok(name) = entry.file_name().into_string() else {
            continue; // non-UTF8 entry names are not skill ids
        };
        if name.starts_with('.') {
            continue; // ignore hidden helper files/dirs
        }

        let meta = match std::fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };

        let (install, managedness, effective_root) = if meta.file_type().is_symlink() {
            let target = std::fs::read_link(&path).unwrap_or_else(|_| PathBuf::new());
            match path.canonicalize() {
                Ok(resolved) => {
                    if resolved == canonical_root || resolved.starts_with(&canonical_root) {
                        (
                            InstallKind::Symlink {
                                target: target.clone(),
                            },
                            Managedness::ManagedSymlink {
                                canonical_path: resolved.clone(),
                            },
                            resolved.clone(),
                        )
                    } else {
                        (
                            InstallKind::Symlink {
                                target: target.clone(),
                            },
                            Managedness::ForeignSymlink { target },
                            resolved.clone(),
                        )
                    }
                }
                Err(_) => (
                    InstallKind::Symlink { target },
                    Managedness::BrokenSymlink,
                    path.clone(),
                ),
            }
        } else if meta.is_dir() {
            (
                InstallKind::Directory,
                if native_shared {
                    Managedness::NativeShared
                } else {
                    Managedness::Unmanaged
                },
                path.clone(),
            )
        } else {
            continue; // plain files are not skills
        };

        // Skills must be anchored by SKILL.md; anything else in the skills
        // root is unrelated content and is ignored (adapter contract).
        let skill_md = effective_root.join("SKILL.md");
        if !skill_md.is_file() {
            continue;
        }

        let mut validation = Vec::new();
        let (frontmatter, files, fingerprint) = match inspect_skill_dir(&effective_root) {
            Ok(v) => v,
            Err(err) => {
                validation.push(ValidationIssue::error("unreadable_skill", err.message));
                (None, Vec::new(), None)
            }
        };

        let display_name = frontmatter
            .as_ref()
            .and_then(|f| f.name.clone())
            .unwrap_or_else(|| name.clone());

        if let Some(fm) = &frontmatter {
            if fm.name.is_none() {
                validation.push(ValidationIssue::warning(
                    "missing_name",
                    "SKILL.md frontmatter has no `name`; using the directory name",
                ));
            } else if fm.name.as_deref() != Some(name.as_str()) {
                validation.push(ValidationIssue::note(
                    "name_mismatch",
                    format!(
                        "frontmatter name `{}` differs from directory name `{name}`",
                        fm.name.as_deref().unwrap_or_default()
                    ),
                ));
            }
            if fm.description.is_none() {
                validation.push(ValidationIssue::warning(
                    "missing_description",
                    "SKILL.md frontmatter has no `description`; tools may surface the skill poorly",
                ));
            }
        }

        // Referenced relative resources must not escape the skill dir (§22).
        validation.extend(validate_resource_references(&effective_root, &files));

        skills.push(ScannedSkill {
            tool_id: tool_id.to_string(),
            location_path: location.to_path_buf(),
            path: path.clone(),
            id: name,
            display_name,
            description: frontmatter.as_ref().and_then(|f| f.description.clone()),
            scope,
            install,
            managedness,
            files,
            fingerprint,
            frontmatter,
            validation,
        });
    }

    skills.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(skills)
}

/// Read a skill directory: parse SKILL.md, list files, fingerprint.
/// Returns an error only if the directory cannot be read at all.
pub fn inspect_skill_dir(
    root: &Path,
) -> Result<(
    Option<SkillFrontmatter>,
    Vec<SkillFileEntry>,
    Option<String>,
)> {
    if !root.is_dir() {
        return Err(
            SkillSyncError::new(ErrorCode::InvalidSkill, "skill root is not a directory")
                .with_path(root),
        );
    }

    let skill_md = root.join("SKILL.md");
    let frontmatter = match std::fs::read(&skill_md) {
        Ok(bytes) => frontmatter::parse(&bytes)?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            return Err(
                SkillSyncError::new(ErrorCode::InvalidSkill, "SKILL.md is missing")
                    .with_path(&skill_md),
            )
        }
        Err(err) => return Err(SkillSyncError::io(&err, &skill_md)),
    };

    let files = list_files(root)?;
    let fingerprint = fingerprint_dir(root).ok();

    Ok((frontmatter, files, fingerprint))
}

/// List all files/dirs under `root` as relative, slash-normalized entries.
pub fn list_files(root: &Path) -> Result<Vec<SkillFileEntry>> {
    let mut files = Vec::new();
    for entry in walkdir::WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let rel = match entry.path().strip_prefix(root) {
            Ok(r) => r,
            Err(_) => continue,
        };
        if rel.as_os_str().is_empty() {
            continue;
        }
        let ft = entry.file_type();
        files.push(SkillFileEntry {
            relative_path: rel.to_string_lossy().replace('\\', "/"),
            is_dir: ft.is_dir(),
            is_symlink: ft.is_symlink(),
        });
    }
    files.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(files)
}

/// Check that markdown-relative links in `SKILL.md` resolve inside the
/// skill directory (never above it). Returns issues for escapes; unreadable
/// files are ignored (a note-level concern at most).
pub fn validate_resource_references(root: &Path, files: &[SkillFileEntry]) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    let file_set: BTreeSet<&str> = files.iter().map(|f| f.relative_path.as_str()).collect();

    for entry in files {
        if entry.is_dir || entry.is_symlink || !entry.relative_path.ends_with(".md") {
            continue;
        }
        let abs = root.join(&entry.relative_path);
        let Ok(text) = std::fs::read_to_string(&abs) else {
            continue;
        };
        for target in extract_markdown_links(&text) {
            if target.starts_with('#') || target.contains("://") || target.starts_with('/') {
                continue; // anchors, URLs, absolute paths are out of scope
            }
            let path_part = target.split('#').next().unwrap_or("");
            if path_part.is_empty() {
                continue;
            }
            // Resolve lexically first: a `..` escape must be flagged even
            // when the referenced file does not exist.
            let resolved = normalize_lexical(&root.join(path_part));
            let within_root = resolved.as_ref().is_some_and(|r| r.starts_with(root));
            if !within_root {
                issues.push(
                    ValidationIssue::error(
                        "resource_escape",
                        format!(
                            "`{}` references `{path_part}` which resolves outside the skill directory",
                            entry.relative_path
                        ),
                    )
                    .with_file(entry.relative_path.clone()),
                );
                continue;
            }
            let resolved = resolved.unwrap();
            match std::fs::metadata(&resolved) {
                Ok(_) => {}
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                    issues.push(
                        ValidationIssue::warning(
                            "missing_resource",
                            format!(
                                "`{}` references `{path_part}` which does not exist",
                                entry.relative_path
                            ),
                        )
                        .with_file(entry.relative_path.clone()),
                    );
                }
                Err(_) => {}
            }
            let _ = file_set; // presence is verified via the filesystem itself
        }
    }
    issues
}

/// Lexically resolve `.`/`..` components. Returns `None` when the result
/// would climb above the base path.
fn normalize_lexical(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            std::path::Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    Some(out)
}

/// Extract markdown link/image targets from `text`.
fn extract_markdown_links(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b']' && i + 1 < bytes.len() && bytes[i + 1] == b'(' {
            if let Some(close) = text[i + 2..].find(')') {
                let target = text[i + 2..i + 2 + close].trim();
                // skip title syntax `[text](url "title")`
                let target = target.split_whitespace().next().unwrap_or(target);
                if !target.is_empty() {
                    out.push(target.to_string());
                }
                i += 2 + close;
                continue;
            }
        }
        i += 1;
    }
    out
}

/// Convenience: build the full `Skill` model for a directory (used for the
/// canonical store view and imports later).
pub fn inspect_as_skill(
    _env: &EnvContext,
    root: &Path,
    scope: SkillScope,
    source: SkillSource,
) -> Result<Skill> {
    let id = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .ok_or_else(|| {
            SkillSyncError::new(ErrorCode::InvalidSkill, "path has no file name").with_path(root)
        })?;
    let (frontmatter, files, fingerprint) = inspect_skill_dir(root)?;
    let mut validation = Vec::new();
    if let Some(fm) = &frontmatter {
        if fm.description.is_none() {
            validation.push(ValidationIssue::warning(
                "missing_description",
                "SKILL.md frontmatter has no `description`",
            ));
        }
    }
    validation.extend(validate_resource_references(root, &files));
    Ok(Skill {
        display_name: frontmatter
            .as_ref()
            .and_then(|f| f.name.clone())
            .unwrap_or_else(|| id.clone()),
        description: frontmatter.as_ref().and_then(|f| f.description.clone()),
        id,
        root: root.to_path_buf(),
        scope,
        source,
        files,
        fingerprint,
        frontmatter,
        validation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_skill(root: &Path, name: &str, skill_md: &[u8]) -> PathBuf {
        let dir = root.join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), skill_md).unwrap();
        dir
    }

    const BASIC: &[u8] = b"---\nname: basic-skill\ndescription: A basic skill\n---\n# Basic\n";

    #[test]
    fn scans_real_directory_as_unmanaged() {
        let tmp = tempfile::tempdir().unwrap();
        let canonical = tmp.path().join("canonical");
        let loc = tmp.path().join("loc");
        write_skill(&loc, "alpha", BASIC);
        std::fs::create_dir_all(&canonical).unwrap();

        let env = EnvContext::with_home(tmp.path());
        let skills = scan_skills_root(&env, "test", &loc, &canonical, SkillScope::Global).unwrap();
        assert_eq!(skills.len(), 1);
        let s = &skills[0];
        assert_eq!(s.id, "alpha");
        assert_eq!(s.display_name, "basic-skill");
        assert_eq!(s.managedness, Managedness::Unmanaged);
        assert_eq!(s.install, InstallKind::Directory);
        assert!(s.fingerprint.is_some());
        assert!(
            s.validation
                .iter()
                .all(|v| v.severity == ValidationSeverity::Note),
            "only informational notes expected, got {:?}",
            s.validation
        );
    }

    #[test]
    fn ignores_dirs_without_skill_md_and_hidden_entries() {
        let tmp = tempfile::tempdir().unwrap();
        let canonical = tmp.path().join("canonical");
        let loc = tmp.path().join("loc");
        write_skill(&loc, "alpha", BASIC);
        std::fs::create_dir_all(loc.join("not-a-skill")).unwrap();
        std::fs::create_dir_all(loc.join(".hidden")).unwrap();
        std::fs::write(loc.join("README.txt"), b"noise").unwrap();
        std::fs::write(loc.join(".DS_Store"), b"noise").unwrap();
        std::fs::create_dir_all(&canonical).unwrap();

        let env = EnvContext::with_home(tmp.path());
        let skills = scan_skills_root(&env, "test", &loc, &canonical, SkillScope::Global).unwrap();
        assert_eq!(skills.len(), 1);
    }

    #[test]
    fn missing_location_is_empty_not_error() {
        let tmp = tempfile::tempdir().unwrap();
        let env = EnvContext::with_home(tmp.path());
        let skills = scan_skills_root(
            &env,
            "test",
            &tmp.path().join("does-not-exist"),
            &tmp.path().join("canonical"),
            SkillScope::Global,
        )
        .unwrap();
        assert!(skills.is_empty());
    }

    #[test]
    fn symlink_into_canonical_is_managed() {
        let tmp = tempfile::tempdir().unwrap();
        let canonical = tmp.path().join("canonical");
        let loc = tmp.path().join("loc");
        let skill = write_skill(&canonical, "alpha", BASIC);
        std::fs::create_dir_all(&loc).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&skill, loc.join("alpha")).unwrap();

        let env = EnvContext::with_home(tmp.path());
        let skills = scan_skills_root(&env, "test", &loc, &canonical, SkillScope::Global).unwrap();
        assert_eq!(skills.len(), 1);
        match &skills[0].managedness {
            Managedness::ManagedSymlink { canonical_path } => {
                assert_eq!(
                    canonical_path.canonicalize().ok(),
                    skill.canonicalize().ok()
                );
            }
            other => panic!("expected managed symlink, got {other:?}"),
        }
    }

    #[test]
    fn symlink_outside_canonical_is_foreign() {
        let tmp = tempfile::tempdir().unwrap();
        let other = write_skill(&tmp.path().join("elsewhere"), "alpha", BASIC);
        let canonical = tmp.path().join("canonical");
        let loc = tmp.path().join("loc");
        std::fs::create_dir_all(&loc).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&other, loc.join("alpha")).unwrap();

        let env = EnvContext::with_home(tmp.path());
        let skills = scan_skills_root(&env, "test", &loc, &canonical, SkillScope::Global).unwrap();
        assert_eq!(skills.len(), 1);
        assert!(matches!(
            skills[0].managedness,
            Managedness::ForeignSymlink { .. }
        ));
    }

    #[test]
    fn broken_symlink_is_classified() {
        let tmp = tempfile::tempdir().unwrap();
        let canonical = tmp.path().join("canonical");
        let loc = tmp.path().join("loc");
        std::fs::create_dir_all(&loc).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(tmp.path().join("gone"), loc.join("alpha")).unwrap();

        let env = EnvContext::with_home(tmp.path());
        let skills = scan_skills_root(&env, "test", &loc, &canonical, SkillScope::Global).unwrap();
        // broken symlinks have no SKILL.md behind them, so they are not
        // surfaced as skills at all — doctor reports them separately.
        assert!(skills.is_empty());
    }

    #[test]
    fn native_shared_location_marks_native() {
        let tmp = tempfile::tempdir().unwrap();
        let store = tmp.path().join("canonical");
        let _skill = write_skill(&store, "alpha", BASIC);
        let env = EnvContext::with_home(tmp.path());
        // Scanning the canonical store itself as a tool location.
        let skills = scan_skills_root(&env, "test", &store, &store, SkillScope::Global).unwrap();
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].managedness, Managedness::NativeShared);
    }

    #[test]
    fn invalid_frontmatter_becomes_validation_error() {
        let tmp = tempfile::tempdir().unwrap();
        let loc = tmp.path().join("loc");
        write_skill(&loc, "bad", b"---\nname: [broken\n---\nbody");
        let canonical = tmp.path().join("canonical");
        std::fs::create_dir_all(&canonical).unwrap();

        let env = EnvContext::with_home(tmp.path());
        let skills = scan_skills_root(&env, "test", &loc, &canonical, SkillScope::Global).unwrap();
        assert_eq!(skills.len(), 1);
        assert!(skills[0]
            .validation
            .iter()
            .any(|v| v.code == "invalid_frontmatter" || v.message.contains("YAML")));
    }

    #[test]
    fn resource_escape_is_detected() {
        let tmp = tempfile::tempdir().unwrap();
        let loc = tmp.path().join("loc");
        let dir = write_skill(
            &loc,
            "esc",
            b"---\nname: esc\ndescription: d\n---\n[secret](../../secrets.txt)",
        );
        let canonical = tmp.path().join("canonical");
        std::fs::create_dir_all(&canonical).unwrap();

        let env = EnvContext::with_home(tmp.path());
        let skills = scan_skills_root(&env, "test", &loc, &canonical, SkillScope::Global).unwrap();
        assert_eq!(skills.len(), 1);
        assert!(
            skills[0]
                .validation
                .iter()
                .any(|v| v.code == "resource_escape"),
            "validation: {:?}",
            skills[0].validation
        );
        let _ = dir;
    }

    #[test]
    fn missing_description_is_warning_not_error() {
        let tmp = tempfile::tempdir().unwrap();
        let loc = tmp.path().join("loc");
        write_skill(&loc, "nodesc", b"---\nname: nodesc\n---\nbody");
        let canonical = tmp.path().join("canonical");
        std::fs::create_dir_all(&canonical).unwrap();

        let env = EnvContext::with_home(tmp.path());
        let skills = scan_skills_root(&env, "test", &loc, &canonical, SkillScope::Global).unwrap();
        assert!(skills[0]
            .validation
            .iter()
            .all(|v| v.severity != ValidationSeverity::Error));
        assert!(skills[0]
            .validation
            .iter()
            .any(|v| v.code == "missing_description"));
    }

    #[test]
    fn inspect_as_skill_full_model() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = write_skill(tmp.path(), "full", BASIC);
        std::fs::create_dir_all(dir.join("scripts")).unwrap();
        std::fs::write(dir.join("scripts/run.sh"), b"echo\n").unwrap();

        let env = EnvContext::with_home(tmp.path());
        let skill =
            inspect_as_skill(&env, &dir, SkillScope::Global, SkillSource::Canonical).unwrap();
        assert_eq!(skill.id, "full");
        assert_eq!(skill.source, SkillSource::Canonical);
        assert!(skill
            .files
            .iter()
            .any(|f| f.relative_path == "scripts/run.sh"));
    }
}
