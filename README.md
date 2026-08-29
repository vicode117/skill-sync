# SkillSync

> Create a Skill once, manage it in one place, and automatically make it
> available to every supported AI coding tool.

SkillSync is a local-first desktop app + CLI that gives you a single control
plane over **Agent Skills** (`SKILL.md` directories) across AI coding tools:

| Tool | Global skills location (auto-detected, overridable) |
|---|---|
| Claude Code | `~/.claude/skills` |
| Codex | `~/.codex/skills` (`$CODEX_HOME`) |
| Cursor | `~/.cursor/skills` (+ reads `~/.agents/skills` natively) |
| Gemini CLI | `~/.gemini/skills` |

Skills live in one canonical store (default `~/.agents/skills`) and are
installed into tools as derived installations (symlink, copy, or recognized
natively). SkillSync never overwrites unmanaged skills and never executes
skill code.

## Status

**Slices 1–3 implemented**: read-only discovery (tool detection, skill
locations, SKILL.md scanning/validation, managedness, doctor, CLI, GUI), the
canonical store (adopt root, import skills with fingerprinting, conflict-safe
resolution, backups, dry-run), one-way sync (canonical → tool, symlink
with copy fallback, managed-ownership tracking, per-change preview), and the
Skill×Tool enablement matrix (per-skill toggles; disabling removes only the
managed installation), conflict management (compare view, explicit
backed-up resolutions, ignore), optional automatic synchronization
(debounced watcher, off by default — manual Sync Now always works), and
explicit git machine-sync for the canonical store (system git, status /
pull --ff-only / commit / push — never automatic), a guided first-import
plan for adopting existing skills, per-skill detail views, and structured
operation logs under `~/.skillsync/logs/`. Every filesystem change runs
only when you invoke it. See `docs/ARCHITECTURE.md`.

## CLI

```bash
skillsync list            # all discovered skills (canonical + per tool)
skillsync tools           # detected tools, locations, capabilities
skillsync scan            # full read-only scan of every tool location
skillsync doctor          # environment diagnostics
skillsync adopt-root      # create the canonical root folder if missing
skillsync import <path>   # import a skill into the canonical store
skillsync import <path> --dry-run              # preview the plan only
skillsync import <path> --keep-both            # conflict: import as <name>-2
skillsync import <path> --replace              # conflict: backup + replace
skillsync sync --tool claude              # canonical -> tool (symlink/copy)
skillsync sync --tool claude --dry-run    # preview the plan only
skillsync sync --all                      # every detected, enabled tool
skillsync disable tdd --tool gemini       # remove only the managed install
skillsync enable tdd --tool gemini        # install it again
skillsync import-plan                     # adopt existing skills (§19)
skillsync import-all [--dry-run]          # import content-unique skills
skillsync conflicts                     # canonical vs unmanaged conflicts
skillsync diff tdd --tool claude        # file + line-level compare
skillsync resolve tdd --tool claude \
  --resolution use-canonical [--dry-run] # explicit, backed-up resolution
skillsync git status                  # machine sync: branch + changed skills
skillsync git pull | commit | push    # always explicit, never automatic
skillsync scan --json     # machine-readable output (all commands)
```

Exit codes: `0` success, `1` operational error / doctor found errors,
`2` usage error.

## Development

Prerequisites: Rust (stable), Node LTS + pnpm, platform Tauri prerequisites.

```bash
pnpm install
pnpm tauri dev            # desktop app
cargo test                # core tests (uses temp dirs + fixtures/ only)
pnpm lint && pnpm typecheck && pnpm test && pnpm build
cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings
pnpm tauri build          # bundle for current platform (.app on macOS;
                          # add "dmg" to bundle.targets for a disk image)
```

Repository layout and architecture rules: [`AGENTS.md`](AGENTS.md),
[`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md).
