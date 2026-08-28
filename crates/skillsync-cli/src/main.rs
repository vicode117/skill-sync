//! SkillSync CLI. Thin layer over `skillsync-core` — no business logic
//! here. Script-friendly: every command supports `--json`; exit codes are
//! `0` success, `1` operational error (or doctor found errors), `2` usage
//! error (clap).

use clap::{Parser, Subcommand};
use skillsync_core::{SkillSync, SyncState};

#[derive(Parser)]
#[command(
    name = "skillsync",
    version,
    about = "Manage Agent Skills across AI coding tools from one canonical store",
    long_about = None
)]
struct Cli {
    /// Emit machine-readable JSON instead of human tables.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// List all discovered skills (canonical + observed per tool).
    List,
    /// Show detected tools, their skill locations and capabilities.
    Tools,
    /// Full read-only scan of every tool skill directory.
    Scan,
    /// Environment diagnostics.
    Doctor,
}

fn main() {
    let cli = Cli::parse();
    std::process::exit(run(cli));
}

fn run(cli: Cli) -> i32 {
    let app = match SkillSync::discover() {
        Ok(app) => app,
        Err(err) => return report_error(&err),
    };

    let result = match cli.command {
        Commands::List => cmd_list(&app, cli.json),
        Commands::Tools => cmd_tools(&app, cli.json),
        Commands::Scan => cmd_scan(&app, cli.json),
        Commands::Doctor => cmd_doctor(&app, cli.json),
    };

    match result {
        Ok(code) => code,
        Err(err) => report_error(&err),
    }
}

fn report_error(err: &skillsync_core::SkillSyncError) -> i32 {
    eprintln!("error[{:?}]: {}", err.code, err.message);
    if let Some(path) = &err.path {
        eprintln!("  path: {}", path.display());
    }
    1
}

fn cmd_list(app: &SkillSync, json: bool) -> skillsync_core::Result<i32> {
    let overview = app.overview()?;
    if json {
        print_json(&overview.rows)?;
        return Ok(0);
    }
    if overview.rows.is_empty() {
        println!("No skills found.");
        println!(
            "Canonical store: {}{}",
            overview.canonical_root_display,
            if overview.canonical_root_exists {
                ""
            } else {
                " (does not exist yet)"
            }
        );
        return Ok(0);
    }

    let name_w = overview
        .rows
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    println!(
        "{:<name_w$}   {:<13}  {:<28} TOOLS",
        "NAME", "STATUS", "SOURCE"
    );
    for row in &overview.rows {
        let tools = row
            .installations
            .iter()
            .map(tool_cell)
            .collect::<Vec<_>>()
            .join(" ");
        println!(
            "{:<name_w$}   {:<13}  {:<28} {}",
            truncate(&row.name, name_w),
            status_label(row.status),
            skillsync_core::overview::row_source_label(row),
            tools
        );
    }
    Ok(0)
}

fn tool_cell(installation: &skillsync_core::Installation) -> String {
    let mark = match installation.state {
        SyncState::Native | SyncState::Synced => "✓",
        SyncState::NotInstalled | SyncState::Disabled => "-",
        SyncState::Unmanaged => "u",
        SyncState::Modified => "~",
        SyncState::Conflict => "!",
        SyncState::Unavailable => "×",
    };
    format!("{}:{}", short_tool_id(&installation.tool_id), mark)
}

fn short_tool_id(id: &str) -> &str {
    match id {
        "claude" => "cla",
        "codex" => "cod",
        "cursor" => "cur",
        "gemini" => "gem",
        other => other,
    }
}

fn status_label(state: SyncState) -> &'static str {
    match state {
        SyncState::Native => "native",
        SyncState::Synced => "synced",
        SyncState::NotInstalled => "not-installed",
        SyncState::Disabled => "disabled",
        SyncState::Modified => "modified",
        SyncState::Conflict => "conflict",
        SyncState::Unmanaged => "unmanaged",
        SyncState::Unavailable => "unavailable",
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max.saturating_sub(1)])
    }
}

fn cmd_tools(app: &SkillSync, json: bool) -> skillsync_core::Result<i32> {
    let overview = app.overview()?;
    if json {
        print_json(&overview.tools)?;
        return Ok(0);
    }
    for tool in &overview.tools {
        println!("{} ({})", tool.display_name, tool.id);
        println!(
            "  Detected: {} — {}",
            if tool.detection.installed {
                "yes"
            } else {
                "no"
            },
            tool.detection.evidence
        );
        println!("  Enabled:  {}", tool.enabled);
        for loc in &tool.locations {
            println!(
                "  Location: {}{} — {} ({} skills, {} managed)",
                loc.display_path,
                if loc.native_canonical {
                    " [canonical store]"
                } else {
                    ""
                },
                if loc.exists { "exists" } else { "missing" },
                loc.skill_count,
                loc.managed_count
            );
        }
        println!(
            "  Symlinks: {:?}; Reload: {}",
            tool.symlink_support, tool.reload_guidance.summary
        );
        println!();
    }
    Ok(0)
}

fn cmd_scan(app: &SkillSync, json: bool) -> skillsync_core::Result<i32> {
    let scanned = app.scan_all()?;
    if json {
        print_json(&scanned)?;
        return Ok(0);
    }
    if scanned.is_empty() {
        println!("No skills discovered in any tool location.");
        return Ok(0);
    }
    let mut current_tool = String::new();
    for skill in &scanned {
        if skill.tool_id != current_tool {
            current_tool = skill.tool_id.clone();
            println!("[{}]", current_tool);
        }
        println!(
            "  {} — {} ({})",
            skill.display_name,
            skillsync_core::scan_managedness_label(&skill.managedness),
            skill.path.display()
        );
        for issue in &skill.validation {
            let level = match issue.severity {
                skillsync_core::ValidationSeverity::Error => "error",
                skillsync_core::ValidationSeverity::Warning => "warn",
                skillsync_core::ValidationSeverity::Note => "note",
            };
            println!("    [{level}] {}: {}", issue.code, issue.message);
        }
    }
    Ok(0)
}

fn cmd_doctor(app: &SkillSync, json: bool) -> skillsync_core::Result<i32> {
    let report = app.doctor();
    if json {
        print_json(&report)?;
        return Ok(if report.has_errors() { 1 } else { 0 });
    }
    println!(
        "SkillSync doctor — os: {}, home: {}",
        report.os, report.skillsync_home
    );
    println!();
    for c in &report.checks {
        let icon = match c.status {
            skillsync_core::CheckStatus::Ok => "✓",
            skillsync_core::CheckStatus::Warning => "!",
            skillsync_core::CheckStatus::Error => "×",
        };
        println!("{icon} {:<24} {}", c.title, c.detail);
        if c.status == skillsync_core::CheckStatus::Error {
            println!("    id: {}", c.id);
        }
    }
    println!();
    if report.has_errors() {
        println!("One or more checks failed.");
        Ok(1)
    } else if report.has_warnings() {
        println!("All critical checks passed; some optional items need attention.");
        Ok(0)
    } else {
        println!("All checks passed.");
        Ok(0)
    }
}

fn print_json<T: serde::Serialize>(value: &T) -> skillsync_core::Result<i32> {
    let text = serde_json::to_string_pretty(value).map_err(|e| {
        skillsync_core::SkillSyncError::new(
            skillsync_core::ErrorCode::Io,
            format!("failed to serialize output: {e}"),
        )
    })?;
    println!("{text}");
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_labels_cover_all_states() {
        // Compile-time exhaustiveness guard for display mapping.
        for state in [
            SyncState::Native,
            SyncState::Synced,
            SyncState::NotInstalled,
            SyncState::Disabled,
            SyncState::Modified,
            SyncState::Conflict,
            SyncState::Unmanaged,
            SyncState::Unavailable,
        ] {
            assert!(!status_label(state).is_empty());
        }
    }

    #[test]
    fn list_output_is_stable_for_empty_overview() {
        let tmp = tempfile_dir();
        let env = skillsync_core::EnvContext::with_home(&tmp);
        let app = SkillSync::with_environment(env);
        let code = cmd_list(&app, false).unwrap();
        assert_eq!(code, 0);
    }

    fn tempfile_dir() -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("skillsync-cli-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
