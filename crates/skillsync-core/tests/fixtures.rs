//! Fixture-based integration tests (design doc §72).
//!
//! The shared fixtures under `fixtures/` are scanned read-only — these
//! tests never mutate them and never touch real user skill directories.

use skillsync_core::scan::scan_skills_root;
use skillsync_core::{EnvContext, Managedness, SkillScope, ValidationSeverity};

fn fixtures_dir() -> std::path::PathBuf {
    let mut path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates/
    path.pop(); // repo root
    path.join("fixtures")
}

#[test]
fn scans_all_fixture_skills() {
    let tmp = tempfile::tempdir().unwrap();
    let env = EnvContext::with_home(tmp.path());
    let fixtures = fixtures_dir();
    let skills = scan_skills_root(
        &env,
        "test",
        &fixtures,
        &tmp.path().join("canonical"),
        SkillScope::Global,
    )
    .unwrap();

    let ids: Vec<&str> = skills.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(
        ids,
        vec![
            "basic-skill",
            "codex-metadata-skill",
            "conflicting-skill",
            "invalid-frontmatter",
            "multi-file-skill"
        ]
    );
}

#[test]
fn multi_file_skill_lists_all_files() {
    let tmp = tempfile::tempdir().unwrap();
    let env = EnvContext::with_home(tmp.path());
    let skills = scan_skills_root(
        &env,
        "test",
        &fixtures_dir(),
        &tmp.path().join("canonical"),
        SkillScope::Global,
    )
    .unwrap();

    let multi = skills.iter().find(|s| s.id == "multi-file-skill").unwrap();
    let files: Vec<&str> = multi
        .files
        .iter()
        .map(|f| f.relative_path.as_str())
        .collect();
    assert!(files.contains(&"SKILL.md"));
    assert!(files.contains(&"scripts/run.sh"));
    assert!(files.contains(&"references/api.md"));
    // internal references resolve, so no escape errors
    assert!(!multi
        .validation
        .iter()
        .any(|v| v.severity == ValidationSeverity::Error));
    assert!(multi.fingerprint.is_some());
}

#[test]
fn invalid_frontmatter_reports_error_not_crash() {
    let tmp = tempfile::tempdir().unwrap();
    let env = EnvContext::with_home(tmp.path());
    let skills = scan_skills_root(
        &env,
        "test",
        &fixtures_dir(),
        &tmp.path().join("canonical"),
        SkillScope::Global,
    )
    .unwrap();

    let bad = skills
        .iter()
        .find(|s| s.id == "invalid-frontmatter")
        .expect("invalid skill must still be discovered");
    assert!(bad.has_errors());
}

#[test]
fn conflicting_skill_has_same_name_as_basic_but_different_content() {
    let tmp = tempfile::tempdir().unwrap();
    let env = EnvContext::with_home(tmp.path());
    let skills = scan_skills_root(
        &env,
        "test",
        &fixtures_dir(),
        &tmp.path().join("canonical"),
        SkillScope::Global,
    )
    .unwrap();

    let basic = skills.iter().find(|s| s.id == "basic-skill").unwrap();
    let conflict = skills.iter().find(|s| s.id == "conflicting-skill").unwrap();
    assert_eq!(basic.display_name, "basic-skill");
    assert_eq!(conflict.display_name, "basic-skill");
    assert_ne!(basic.fingerprint, conflict.fingerprint);
}

#[test]
fn codex_metadata_is_preserved() {
    let tmp = tempfile::tempdir().unwrap();
    let env = EnvContext::with_home(tmp.path());
    let skills = scan_skills_root(
        &env,
        "test",
        &fixtures_dir(),
        &tmp.path().join("canonical"),
        SkillScope::Global,
    )
    .unwrap();

    let codex = skills
        .iter()
        .find(|s| s.id == "codex-metadata-skill")
        .unwrap();
    let fm = codex.frontmatter.as_ref().unwrap();
    assert!(fm.raw.get("metadata").is_some(), "raw metadata preserved");
    assert!(codex
        .files
        .iter()
        .any(|f| f.relative_path == "agents/openai.yaml"));
    assert_eq!(codex.managedness, Managedness::Unmanaged);
}
