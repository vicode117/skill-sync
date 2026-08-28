//! `SKILL.md` frontmatter parsing.
//!
//! A skill's `SKILL.md` is `---`-delimited YAML frontmatter followed by
//! markdown. YAML is parsed with a real YAML parser; unknown keys are
//! preserved verbatim. Parsing never executes anything and never mutates
//! the skill.

use crate::error::{ErrorCode, Result, SkillSyncError};
use crate::skill::SkillFrontmatter;

/// Parse the frontmatter of a `SKILL.md` given as raw bytes.
///
/// Returns `Ok(None)` when the file has no frontmatter block.
pub fn parse(bytes: &[u8]) -> Result<Option<SkillFrontmatter>> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| SkillSyncError::new(ErrorCode::InvalidSkill, "SKILL.md is not valid UTF-8"))?;

    let mut lines = text.lines();
    let first = lines.next().unwrap_or("");
    if first.trim_end() != "---" {
        return Ok(None);
    }

    // Find the closing delimiter line.
    let mut frontmatter = String::new();
    let mut closed = false;
    for line in lines {
        if line.trim_end() == "---" {
            closed = true;
            break;
        }
        frontmatter.push_str(line);
        frontmatter.push('\n');
    }
    if !closed {
        return Err(SkillSyncError::new(
            ErrorCode::InvalidSkill,
            "SKILL.md frontmatter is missing its closing `---`",
        ));
    }

    let value: serde_yaml::Value = serde_yaml::from_str(&frontmatter).map_err(|e| {
        SkillSyncError::new(
            ErrorCode::InvalidSkill,
            format!("SKILL.md frontmatter is not parseable YAML: {e}"),
        )
    })?;

    match value {
        serde_yaml::Value::Mapping(map) => {
            let json = serde_json::to_value(&map)
                .map_err(|e| SkillSyncError::new(ErrorCode::InvalidSkill, e.to_string()))?;
            Ok(Some(SkillFrontmatter {
                name: json_string(&json, "name"),
                description: json_string(&json, "description"),
                raw: json,
            }))
        }
        serde_yaml::Value::Null => Ok(Some(SkillFrontmatter {
            name: None,
            description: None,
            raw: serde_json::Value::Object(Default::default()),
        })),
        _ => Err(SkillSyncError::new(
            ErrorCode::InvalidSkill,
            "SKILL.md frontmatter must be a YAML mapping",
        )),
    }
}

fn json_string(value: &serde_json::Value, key: &str) -> Option<String> {
    match value.get(key) {
        Some(serde_json::Value::String(s)) => Some(s.clone()),
        // Be forgiving with unquoted scalars that YAML reads as non-strings
        // (e.g. a name like `true`); stringify them.
        Some(other) if !other.is_null() && !other.is_object() && !other.is_array() => {
            Some(other.to_string().trim_matches('"').to_string())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_standard_frontmatter() {
        let md = b"---\nname: git-commit\ndescription: Creates good commits\nlicense: MIT\n---\n\n# Body";
        let fm = parse(md).unwrap().unwrap();
        assert_eq!(fm.name.as_deref(), Some("git-commit"));
        assert_eq!(fm.description.as_deref(), Some("Creates good commits"));
        assert_eq!(fm.raw.get("license").and_then(|v| v.as_str()), Some("MIT"));
    }

    #[test]
    fn preserves_unknown_metadata() {
        let md = b"---\nname: x\ndescription: y\nmetadata:\n  openai:\n    model: gpt-5\n---\nbody";
        let fm = parse(md).unwrap().unwrap();
        let metadata = fm.raw.get("metadata").unwrap();
        assert!(metadata.get("openai").is_some());
    }

    #[test]
    fn returns_none_without_frontmatter() {
        assert!(parse(b"# Just markdown\n").unwrap().is_none());
        assert!(parse(b"").unwrap().is_none());
    }

    #[test]
    fn rejects_unterminated_frontmatter() {
        let err = parse(b"---\nname: x\n").unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidSkill);
    }

    #[test]
    fn rejects_invalid_yaml() {
        let md = b"---\nname: [unclosed\n---\nbody";
        let err = parse(md).unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidSkill);
        assert!(err.message.contains("YAML"));
    }

    #[test]
    fn rejects_non_mapping_frontmatter() {
        let err = parse(b"---\n- a\n- b\n---\n").unwrap_err();
        assert_eq!(err.code, ErrorCode::InvalidSkill);
    }

    #[test]
    fn coerces_scalar_name() {
        let md = b"---\nname: 42\ndescription: n\n---\n";
        let fm = parse(md).unwrap().unwrap();
        assert_eq!(fm.name.as_deref(), Some("42"));
    }
}
