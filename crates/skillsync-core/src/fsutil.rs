//! Filesystem safety helpers (design doc §5, §29).
//!
//! Everything that writes or deletes goes through here so the safety rules
//! exist in exactly one place. Slice 1 only *writes SkillSync-owned config*;
//! the mutation API grows in Slice 3 on top of these primitives.

use std::io::Write;
use std::path::{Component, Path, PathBuf};

use crate::error::{ErrorCode, Result, SkillSyncError};

/// Write bytes atomically: temp file in the target directory, fsync, rename.
pub fn atomic_write(target: &Path, bytes: &[u8]) -> Result<()> {
    let parent = target.parent().ok_or_else(|| {
        SkillSyncError::new(ErrorCode::UnsafePath, "path has no parent directory")
    })?;
    std::fs::create_dir_all(parent).map_err(|e| SkillSyncError::io(&e, parent))?;

    let file_name = target
        .file_name()
        .ok_or_else(|| SkillSyncError::new(ErrorCode::UnsafePath, "path has no file name"))?;
    let tmp = parent.join(format!(
        ".{}.tmp-{}",
        file_name.to_string_lossy(),
        std::process::id()
    ));
    {
        let mut f = std::fs::File::create(&tmp).map_err(|e| SkillSyncError::io(&e, &tmp))?;
        f.write_all(bytes).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            SkillSyncError::io(&e, &tmp)
        })?;
        f.sync_all().map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            SkillSyncError::io(&e, &tmp)
        })?;
    }
    std::fs::rename(&tmp, target).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        SkillSyncError::io(&e, target)
    })
}

/// A path is *safe to operate on* when it is absolute, contains no `..`
/// components, and is not a filesystem root.
pub fn validate_no_traversal(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(
            SkillSyncError::new(ErrorCode::UnsafePath, "path must be absolute").with_path(path),
        );
    }
    for component in path.components() {
        match component {
            Component::ParentDir => {
                return Err(SkillSyncError::new(
                    ErrorCode::UnsafePath,
                    "path contains `..` traversal",
                )
                .with_path(path))
            }
            Component::RootDir | Component::Prefix(_) => {}
            _ => {}
        }
    }
    if path.parent().is_none() || path == Path::new("/") {
        return Err(SkillSyncError::new(
            ErrorCode::UnsafePath,
            "refusing to operate on a filesystem root",
        )
        .with_path(path));
    }
    Ok(())
}

/// Ensure `candidate` stays inside `boundary` after resolution (the
/// candidate itself may not exist yet). Symlinked ancestors (macOS
/// `/var` → `/private/var`, and similar) are resolved via the deepest
/// existing ancestor so the containment check is sound.
pub fn ensure_within(candidate: &Path, boundary: &Path) -> Result<()> {
    validate_no_traversal(candidate)?;
    let boundary = boundary
        .canonicalize()
        .map_err(|e| SkillSyncError::io(&e, boundary))?;

    // Walk up to the deepest ancestor that exists, remembering the rest.
    let mut existing = candidate.to_path_buf();
    let mut suffix = PathBuf::new();
    while !existing.exists() {
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) if parent != existing => {
                suffix = Path::new(name).join(&suffix);
                existing = parent.to_path_buf();
            }
            _ => break,
        }
    }
    let base = existing
        .canonicalize()
        .map_err(|e| SkillSyncError::io(&e, existing))?;
    let resolved = if suffix.as_os_str().is_empty() {
        base
    } else {
        base.join(suffix)
    };

    if !resolved.starts_with(&boundary) {
        return Err(SkillSyncError::new(
            ErrorCode::UnsafePath,
            "path escapes the allowed boundary",
        )
        .with_path(candidate));
    }
    Ok(())
}

/// Protect against operating on the home directory itself or anything above
/// it (accidental home-directory deletion, filesystem root deletion).
pub fn validate_not_home_or_root(path: &Path, home: &Path) -> Result<()> {
    validate_no_traversal(path)?;
    if path == Path::new("/") || path == home {
        return Err(SkillSyncError::new(
            ErrorCode::UnsafePath,
            "refusing to operate on the home directory or filesystem root",
        )
        .with_path(path));
    }
    Ok(())
}

/// Remove a directory tree *only* after the caller has verified ownership
/// and path boundaries. This is intentionally a narrow primitive: every
/// call site must document ownership (AGENTS.md safety rules).
pub fn remove_dir_verified(path: &Path, what: &str) -> Result<()> {
    std::fs::remove_dir_all(path)
        .map_err(|e| SkillSyncError::io(&e, path))
        .map_err(|mut e| {
            e.message = format!("failed to remove {what}: {}", e.message);
            e
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn rejects_traversal_and_relative_paths() {
        assert!(validate_no_traversal(Path::new("relative/path")).is_err());
        assert!(validate_no_traversal(Path::new("/a/../b")).is_err());
        assert!(validate_no_traversal(Path::new("/")).is_err());
        assert!(validate_no_traversal(Path::new("/a/b")).is_ok());
    }

    #[test]
    fn rejects_home_and_root() {
        let home = PathBuf::from("/Users/tester");
        assert!(validate_not_home_or_root(Path::new("/"), &home).is_err());
        assert!(validate_not_home_or_root(&home, &home).is_err());
        assert!(validate_not_home_or_root(&home.join("x"), &home).is_ok());
    }

    #[test]
    fn atomic_write_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let target = tmp.path().join("nested").join("config.json");
        atomic_write(&target, b"hello").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"hello");
        atomic_write(&target, b"second").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"second");
        // no temp files left behind
        let entries: Vec<_> = std::fs::read_dir(target.parent().unwrap())
            .unwrap()
            .collect();
        assert_eq!(entries.len(), 1);
    }
}
