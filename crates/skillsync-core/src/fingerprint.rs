//! Deterministic skill fingerprints (design doc §16).
//!
//! A fingerprint is a SHA-256 over the complete relevant skill tree:
//! for every entry, in sorted order — its slash-normalized relative path,
//! its kind (dir / file / symlink), and for files the SHA-256 of the bytes.
//! No mtimes, no permissions, no platform metadata. The same content
//! yields the same fingerprint on Windows, macOS and Linux.

use std::path::Path;

use sha2::{Digest, Sha256};

use crate::error::{Result, SkillSyncError};

/// OS-generated noise that must not affect identity.
fn is_ignored(name: &str) -> bool {
    matches!(name, ".DS_Store" | "Thumbs.db" | "desktop.ini")
}

fn hash_string(h: &mut Sha256, s: &str) {
    h.update((s.len() as u64).to_le_bytes());
    h.update(s.as_bytes());
}

fn hash_bytes(h: &mut Sha256, b: &[u8]) {
    h.update((b.len() as u64).to_le_bytes());
    h.update(b);
}

/// Compute the fingerprint of a directory tree.
pub fn fingerprint_dir(root: &Path) -> Result<String> {
    let mut entries: Vec<_> = walkdir::WalkDir::new(root)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| {
            SkillSyncError::new(
                crate::error::ErrorCode::Io,
                format!("failed to walk skill directory: {e}"),
            )
            .with_path(root)
        })?;

    entries.sort_by(|a, b| a.path().cmp(b.path()));

    let mut h = Sha256::new();
    for entry in entries {
        let rel = entry.path().strip_prefix(root).map_err(|_| {
            SkillSyncError::new(
                crate::error::ErrorCode::Io,
                "walkdir returned a path outside the skill root",
            )
            .with_path(root)
        })?;
        if rel.as_os_str().is_empty() {
            continue; // the root itself
        }
        let rel_str = rel.to_string_lossy().replace('\\', "/");
        let name = entry.file_name().to_string_lossy();
        if !rel_str.contains('/') && is_ignored(&name) {
            continue;
        }

        let file_type = entry.file_type();
        if file_type.is_symlink() {
            let target = std::fs::read_link(entry.path())
                .map_err(|e| SkillSyncError::io(&e, entry.path()))?;
            hash_string(&mut h, &rel_str);
            h.update(b"L");
            hash_bytes(&mut h, target.to_string_lossy().as_bytes());
        } else if file_type.is_dir() {
            hash_string(&mut h, &rel_str);
            h.update(b"D");
        } else {
            let bytes =
                std::fs::read(entry.path()).map_err(|e| SkillSyncError::io(&e, entry.path()))?;
            let mut fh = Sha256::new();
            fh.update(&bytes);
            hash_string(&mut h, &rel_str);
            h.update(b"F");
            hash_bytes(&mut h, &fh.finalize());
        }
    }

    Ok(hex::encode(h.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, bytes: &[u8]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }

    #[test]
    fn same_content_same_fingerprint() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        write(&a.path().join("SKILL.md"), b"---\nname: x\n---\nhi");
        write(&a.path().join("scripts/run.sh"), b"echo hi\n");
        write(&b.path().join("SKILL.md"), b"---\nname: x\n---\nhi");
        write(&b.path().join("scripts/run.sh"), b"echo hi\n");
        assert_eq!(
            fingerprint_dir(a.path()).unwrap(),
            fingerprint_dir(b.path()).unwrap()
        );
    }

    #[test]
    fn ignores_metadata_and_platform_noise() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        write(&a.path().join("SKILL.md"), b"content");
        write(&b.path().join("SKILL.md"), b"content");
        // b has extra OS noise and a different mtime is implicit
        write(&b.path().join(".DS_Store"), b"junk");
        assert_eq!(
            fingerprint_dir(a.path()).unwrap(),
            fingerprint_dir(b.path()).unwrap()
        );
    }

    #[test]
    fn different_content_different_fingerprint() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        write(&a.path().join("SKILL.md"), b"v1");
        write(&b.path().join("SKILL.md"), b"v2");
        assert_ne!(
            fingerprint_dir(a.path()).unwrap(),
            fingerprint_dir(b.path()).unwrap()
        );
    }

    #[test]
    fn path_change_changes_fingerprint() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        write(&a.path().join("docs/ref.md"), b"x");
        write(&b.path().join("ref.md"), b"x");
        assert_ne!(
            fingerprint_dir(a.path()).unwrap(),
            fingerprint_dir(b.path()).unwrap()
        );
    }

    #[test]
    fn empty_dir_is_stable() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        assert_eq!(
            fingerprint_dir(a.path()).unwrap(),
            fingerprint_dir(b.path()).unwrap()
        );
    }
}
