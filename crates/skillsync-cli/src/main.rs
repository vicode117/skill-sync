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
enum GitAction {
    /// Branch, ahead/behind, changed skills.
    Status,
    /// Pull with --ff-only (never merges or overwrites silently).
    Pull,
    /// Stage all changes in the store and commit with a message.
    Commit {
        #[arg(long)]
        message: String,
    },
    /// Push to the configured upstream.
    Push,
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
    /// Create the canonical skill root folder if it does not exist yet.
    AdoptRoot,
    /// Sync canonical skills into tool directories (one-way).
    Sync {
        /// Tool to sync into; omit with --all.
        #[arg(long, group = "target")]
        tool: Option<String>,
        /// Sync every detected, enabled tool.
        #[arg(long, group = "target")]
        all: bool,
        /// Preview the plan without writing anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// Enable a canonical skill for one tool (installs it).
    Enable {
        /// Canonical skill directory name.
        skill: String,
        /// Tool to enable (see `skillsync tools`).
        #[arg(long)]
        tool: String,
        /// Preview without writing anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// Disable a canonical skill for one tool (removes only the managed
    /// installation; unmanaged files are never deleted).
    Disable {
        /// Canonical skill directory name.
        skill: String,
        /// Tool to disable (see `skillsync tools`).
        #[arg(long)]
        tool: String,
        /// Preview without writing anything.
        #[arg(long)]
        dry_run: bool,
    },
    /// List canonical vs unmanaged-target conflicts.
    Conflicts,
    /// Machine sync: explicit git operations on the canonical store (§35).
    Git {
        #[command(subcommand)]
        action: GitAction,
    },
    /// Directory-aware diff between a canonical skill and a tool target.
    Diff {
        /// Canonical skill directory name.
        skill: String,
        /// Tool whose installation to compare.
        #[arg(long)]
        tool: String,
    },
    /// Resolve a conflict (never without a backup; explicit choice only).
    Resolve {
        /// Canonical skill directory name.
        skill: String,
        /// Tool whose conflicting target to resolve.
        #[arg(long)]
        tool: String,
        /// Resolution: use-canonical | import-target | keep-both.
        #[arg(long)]
        resolution: String,
        /// Preview without writing anything.
        #[arg(long)]
        dry_run: bool,
        /// Silence this conflict without changing anything.
        #[arg(long)]
        ignore: bool,
    },
    /// Import a skill directory into the canonical store.
    Import {
        /// Path of the skill directory to import.
        path: String,
        /// Preview the plan without writing anything.
        #[arg(long)]
        dry_run: bool,
        /// On conflicting content: import as `<name>-2` instead of failing.
        #[arg(long, group = "resolution")]
        keep_both: bool,
        /// On conflicting content: back up the canonical copy, then replace.
        #[arg(long, group = "resolution")]
        replace: bool,
    },
}

fn main() {
    let cli = Cli::parse();
    std::process::exit(run(cli));
}

fn run(cli: Cli) -> i32 {
    let mut app = match SkillSync::discover() {
        Ok(app) => app,
        Err(err) => return report_error(&err),
    };

    let result = match cli.command {
        Commands::List => cmd_list(&app, cli.json),
        Commands::Tools => cmd_tools(&app, cli.json),
        Commands::Scan => cmd_scan(&app, cli.json),
        Commands::Doctor => cmd_doctor(&app, cli.json),
        Commands::AdoptRoot => cmd_adopt_root(&app, cli.json),
        Commands::Sync { tool, all, dry_run } => {
            if all {
                cmd_sync_all(&app, dry_run, cli.json)
            } else if let Some(tool) = tool {
                cmd_sync(&app, &tool, dry_run, cli.json)
            } else {
                eprintln!("error: specify --tool <id> or --all");
                Ok(2)
            }
        }
        Commands::Conflicts => cmd_conflicts(&app, cli.json),
        Commands::Git { action } => cmd_git(&app, action, cli.json),
        Commands::Diff { skill, tool } => cmd_diff(&app, &skill, &tool, cli.json),
        Commands::Resolve {
            skill,
            tool,
            resolution,
            dry_run,
            ignore,
        } => cmd_resolve(
            &mut app,
            &skill,
            &tool,
            &resolution,
            dry_run,
            ignore,
            cli.json,
        ),
        Commands::Enable {
            skill,
            tool,
            dry_run,
        } => cmd_set_enabled(&mut app, &skill, &tool, true, dry_run, cli.json),
        Commands::Disable {
            skill,
            tool,
            dry_run,
        } => cmd_set_enabled(&mut app, &skill, &tool, false, dry_run, cli.json),
        Commands::Import {
            path,
            dry_run,
            keep_both,
            replace,
        } => {
            let resolution = if keep_both {
                skillsync_core::ImportResolution::KeepBoth
            } else if replace {
                skillsync_core::ImportResolution::Replace
            } else {
                skillsync_core::ImportResolution::Skip
            };
            cmd_import(&app, &path, resolution, dry_run, cli.json)
        }
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

fn cmd_sync_all(app: &SkillSync, dry_run: bool, json: bool) -> skillsync_core::Result<i32> {
    let reports = app.sync_all(dry_run)?;
    if json {
        return print_json(&reports);
    }
    let mut failures = 0;
    for report in &reports {
        println!("Sync {} — {}", report.tool_id, report.summary());
        failures += report.failed.len();
    }
    if reports.is_empty() {
        println!("No detected, enabled tools to sync.");
    }
    Ok(if failures == 0 { 0 } else { 1 })
}

fn cmd_set_enabled(
    app: &mut SkillSync,
    skill: &str,
    tool: &str,
    enabled: bool,
    dry_run: bool,
    json: bool,
) -> skillsync_core::Result<i32> {
    let report = app.set_skill_tool_enabled(skill, tool, enabled, dry_run)?;
    if json {
        return print_json(&report);
    }
    if dry_run {
        println!("DRY RUN — no changes made.");
    }
    for outcome in report.succeeded.iter().chain(report.failed.iter()) {
        let verb = if enabled { "enable" } else { "disable" };
        println!(
            "{}: {} {} {}",
            skill,
            verb,
            outcome.action_kind,
            if outcome.ok { "ok" } else { "FAILED" }
        );
    }
    if report.failed.is_empty() && report.succeeded.is_empty() {
        println!(
            "{skill} is already {} for {tool} (nothing to change)",
            if enabled { "enabled" } else { "disabled" }
        );
    }
    Ok(if report.failed.is_empty() { 0 } else { 1 })
}

fn cmd_git(app: &SkillSync, action: GitAction, json: bool) -> skillsync_core::Result<i32> {
    match action {
        GitAction::Status => {
            let status = app.git_status()?;
            if json {
                return print_json(&status);
            }
            if !status.is_repo {
                println!(
                    "Canonical store is not a git repository{}",
                    status.error.map(|e| format!(" ({e})")).unwrap_or_default()
                );
                return Ok(1);
            }
            println!(
                "branch: {}{}",
                status.branch.as_deref().unwrap_or("(detached)"),
                if status.has_upstream {
                    format!(" [ahead {}, behind {}]", status.ahead, status.behind)
                } else {
                    " (no upstream)".to_string()
                }
            );
            if status.changed_skills.is_empty() {
                println!("working tree clean");
            } else {
                for change in &status.changed_skills {
                    println!(
                        "  {:?}: {} ({} file{})",
                        change.change,
                        change.skill_id,
                        change.files.len(),
                        if change.files.len() == 1 { "" } else { "s" }
                    );
                }
            }
            Ok(0)
        }
        GitAction::Pull => {
            let out = app.git_pull()?;
            if !json {
                println!("{out}");
            } else {
                print_json(&serde_json::json!({ "output": out }))?;
            }
            Ok(0)
        }
        GitAction::Commit { message } => {
            let out = app.git_commit(&message)?;
            if !json {
                println!("{out}");
            } else {
                print_json(&serde_json::json!({ "output": out }))?;
            }
            Ok(0)
        }
        GitAction::Push => {
            let out = app.git_push()?;
            if !json {
                println!("{out}");
            } else {
                print_json(&serde_json::json!({ "output": out }))?;
            }
            Ok(0)
        }
    }
}

fn cmd_conflicts(app: &SkillSync, json: bool) -> skillsync_core::Result<i32> {
    let conflicts = app.conflicts()?;
    let active: Vec<_> = conflicts.iter().filter(|c| !c.ignored).collect();
    if json {
        return print_json(&conflicts);
    }
    if active.is_empty() {
        println!("No conflicts.");
        return Ok(0);
    }
    println!(
        "{:<22} {:<10} {:<28} {:<28} STATUS",
        "SKILL", "TOOL", "CANONICAL", "TARGET"
    );
    for c in &active {
        println!(
            "{:<22} {:<10} {:<28} {:<28} CONFLICT",
            c.skill_name, c.tool_id, c.canonical_display, c.target_display
        );
    }
    println!(
        "\n{} conflict(s). Resolve with: skillsync resolve <skill> --tool <id> \\\n  --resolution use-canonical|import-target|keep-both [--dry-run]",
        active.len()
    );
    Ok(0)
}

fn cmd_diff(app: &SkillSync, skill: &str, tool: &str, json: bool) -> skillsync_core::Result<i32> {
    let diff = app.diff_skill_tool(skill, tool)?;
    if json {
        return print_json(&diff);
    }
    if diff.is_empty() {
        println!("No differences.");
        return Ok(0);
    }
    for entry in &diff {
        println!(
            "{:<10} {}",
            entry.kind.kind_label().to_uppercase(),
            entry.relative_path
        );
        if let Some(text) = &entry.text_diff {
            for line in text.lines() {
                println!("    {line}");
            }
        }
    }
    Ok(0)
}

fn cmd_resolve(
    app: &mut SkillSync,
    skill: &str,
    tool: &str,
    resolution: &str,
    dry_run: bool,
    ignore: bool,
    json: bool,
) -> skillsync_core::Result<i32> {
    if ignore {
        let mut config = app.config().clone();
        config.set_conflict_ignored(skill, tool, true);
        app.save_config(config)?;
        if !json {
            println!("{skill} × {tool} conflicts will be ignored.");
        } else {
            print_json(&serde_json::json!({ "ignored": true }))?;
        }
        return Ok(0);
    }
    let resolution = match resolution {
        "use-canonical" => skillsync_core::Resolution::UseCanonical,
        "import-target" => skillsync_core::Resolution::ImportTarget,
        "keep-both" => skillsync_core::Resolution::KeepBoth,
        other => {
            eprintln!(
                "error: unknown resolution `{other}`; use use-canonical, import-target or keep-both"
            );
            return Ok(2);
        }
    };
    let report = app.resolve_conflict(skill, tool, resolution, dry_run)?;
    if json {
        return print_json(&report);
    }
    if dry_run {
        println!("DRY RUN — no changes made.");
    }
    for note in &report.notes {
        println!("  note: {note}");
    }
    for backup in &report.backups {
        println!("  backup: {}", backup.display());
    }
    println!(
        "Resolved {skill} × {tool}: {}",
        match report.resolution {
            skillsync_core::Resolution::UseCanonical => "canonical version installed",
            skillsync_core::Resolution::ImportTarget => "target version imported",
            skillsync_core::Resolution::KeepBoth => "imported under a new name",
        }
    );
    Ok(0)
}

fn cmd_sync(
    app: &SkillSync,
    tool_id: &str,
    dry_run: bool,
    json: bool,
) -> skillsync_core::Result<i32> {
    let plan = app.plan_sync(tool_id)?;
    if dry_run && json {
        return print_json(&plan);
    }
    if !json {
        let method = match plan.method {
            skillsync_core::EffectiveMethod::Symlink => "symlink",
            skillsync_core::EffectiveMethod::Copy => "copy",
        };
        let target = plan
            .target_dir
            .as_deref()
            .map(|t| t.display().to_string())
            .unwrap_or_else(|| "<no location>".into());
        println!(
            "SYNC PLAN {} ({method}) -> {}",
            plan.tool_display_name, target
        );
        for entry in &plan.entries {
            let label = entry.action.kind_label();
            let detail = match &entry.action {
                skillsync_core::PlanAction::CreateLink { source, .. } => format!(
                    "-> {}",
                    skillsync_core::env::abbreviate_home(source, app.env())
                ),
                skillsync_core::PlanAction::Skip { reason, .. } => format!("({reason})"),
                _ => String::new(),
            };
            println!(
                "  {:<10} {:<24} {} {}",
                label.to_uppercase(),
                entry.skill_name,
                entry.display_target,
                detail
            );
        }
        for note in plan.entries.iter().flat_map(|e| e.notes.iter()) {
            println!("  note: {note}");
        }
    }
    let report = app.sync_tool(tool_id, dry_run)?;
    if json {
        return print_json(&report);
    }
    if dry_run {
        println!("DRY RUN — no changes made.");
    }
    for failure in &report.failed {
        println!(
            "FAILED {}: {} ({})",
            failure.skill_id,
            failure.action_kind,
            failure.error.as_deref().unwrap_or("unknown error")
        );
    }
    println!("Sync {} — {}", plan.tool_display_name, report.summary());
    Ok(if report.failed.is_empty() { 0 } else { 1 })
}

fn cmd_adopt_root(app: &SkillSync, json: bool) -> skillsync_core::Result<i32> {
    let root = app.adopt_canonical_root()?;
    if json {
        return print_json(&serde_json::json!({ "canonicalRoot": root }));
    }
    println!("Canonical skill root ready: {}", root.display());
    Ok(0)
}

fn cmd_import(
    app: &SkillSync,
    path: &str,
    resolution: skillsync_core::ImportResolution,
    dry_run: bool,
    json: bool,
) -> skillsync_core::Result<i32> {
    let source = std::path::PathBuf::from(shellexpand_home(path, app));
    let plan = app.plan_import(&source, resolution)?;
    if dry_run && json {
        return print_json(&plan);
    }
    if !json {
        println!("IMPORT PLAN");
        println!("  source:  {}", plan.source.display());
        println!("  action:  {}", import_action_label(&plan.action));
        if let Some(fp) = &plan.fingerprint {
            println!("  content: {}…", &fp[..fp.len().min(12)]);
        }
        for note in &plan.notes {
            println!("  note:    {note}");
        }
    }
    let outcome = app.execute_import(&plan, dry_run)?;
    if json {
        return print_json(&outcome);
    }
    if dry_run {
        println!("DRY RUN — no changes made.");
    } else {
        match &outcome.action_taken {
            skillsync_core::ImportAction::AlreadyPresent { .. } => {
                println!("No change — identical skill already in the canonical store.");
            }
            skillsync_core::ImportAction::Replace { backup_dir, .. } => {
                println!("Replaced. Backup saved to {}", backup_dir.display());
            }
            _ => println!("Imported to {}", outcome.target.display()),
        }
    }
    Ok(0)
}

fn import_action_label(action: &skillsync_core::ImportAction) -> String {
    match action {
        skillsync_core::ImportAction::Create { target } => {
            format!("create {}", target.display())
        }
        skillsync_core::ImportAction::AlreadyPresent { target } => {
            format!("no change ({} identical)", target.display())
        }
        skillsync_core::ImportAction::KeepBoth { target } => {
            format!("keep both — import as {}", target.display())
        }
        skillsync_core::ImportAction::Replace { target, backup_dir } => {
            format!(
                "replace {} (backup: {})",
                target.display(),
                backup_dir.display()
            )
        }
        skillsync_core::ImportAction::Conflict { target } => format!(
            "CONFLICT — {} differs from the source; pass --keep-both or --replace",
            target.display()
        ),
    }
}

/// Expand a leading `~` using the app's environment (CLI convenience).
fn shellexpand_home(path: &str, app: &SkillSync) -> String {
    if path == "~" || path.starts_with("~/") || path.starts_with("~\\") {
        let expanded = skillsync_core::env::expand_home(path, app.env());
        return expanded.to_string_lossy().into_owned();
    }
    path.to_string()
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
