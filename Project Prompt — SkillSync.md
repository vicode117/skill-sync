# Project Prompt — SkillSync

You are helping me design and implement a new cross-platform developer tool:

# SkillSync

A local-first desktop application and CLI for centrally managing and synchronizing Agent Skills across AI coding tools.

The product is conceptually similar to the unified management experience of CC Switch, but its scope is intentionally focused on:

```text
Agent Skills
```

rather than API providers, proxies, MCP configuration, or general AI CLI configuration.

The primary goal is:

> Create a Skill once, manage it in one place, and automatically make it available to every supported AI coding tool.

Target tools initially include:

```text
Claude Code
OpenAI Codex
Cursor
Gemini CLI
```

Architecture must make it straightforward to add:

```text
OpenCode
Hermes Agent
OpenClaw
other Agent Skills compatible tools
```

later.

Do not hard-code the entire application around today's four tools.

---

# 1. Product Vision

Different AI coding tools increasingly support the Agent Skills / `SKILL.md` model, but each tool may:

- discover skills from different directories
- support different scopes
- support different metadata extensions
- handle symlinks differently
- support different reload behavior
- introduce tool-specific files alongside `SKILL.md`

SkillSync provides a single control plane over these differences.

The user should think in terms of:

```text
My Skills
```

instead of:

```text
My Claude Skills
My Codex Skills
My Gemini Skills
My Cursor Skills
```

---

# 2. Core Principle

The most important architectural rule is:

# Single Source of Truth

Skills should have exactly one canonical source.

Do NOT treat every target application's skill directory as an independent authoritative copy.

The conceptual architecture is:

```text
              SkillSync
                 │
                 │
        Canonical Skill Store
                 │
       ┌─────────┼──────────┐
       │         │          │
       ▼         ▼          ▼
    Claude     Codex      Gemini
       │         │          │
       └──── Cursor / others
```

Synchronization targets are derived installations of the canonical Skill.

---

# 3. Canonical Skill Store

Default canonical user-level Skill directory:

```text
~/.agents/skills/
```

when practical.

Reason:

Multiple Agent Skills compatible tools already recognize the `.agents/skills` convention.

However, the canonical location must be configurable.

Example:

```text
~/.agents/skills/

or

~/Developer/my-agent-skills/

or

~/OneDrive/agent-skills/

or

~/Dropbox/agent-skills/
```

Do not scatter SkillSync metadata throughout Skill folders unnecessarily.

A Skill directory should remain portable and usable without SkillSync.

---

# 4. Skill Format

Treat a Skill as a directory.

Minimal structure:

```text
my-skill/
├── SKILL.md
└── ...
```

Potential optional files:

```text
my-skill/
├── SKILL.md
├── scripts/
├── references/
├── assets/
└── agents/
```

`SKILL.md` is required.

Do not assume a Skill consists only of `SKILL.md`.

Synchronization must copy/link the entire Skill directory.

---

# 5. Portability Principle

The base Skill should remain compatible with the common Agent Skills format whenever possible.

A Skill may contain tool-specific extensions.

For example:

```text
my-skill/
├── SKILL.md
│
├── scripts/
├── references/
├── assets/
│
└── agents/
    └── openai.yaml
```

Tool-specific files must NOT cause the common Skill content to be forked into separate copies.

Prefer:

```text
one shared Skill
+
optional tool-specific metadata
```

instead of:

```text
claude-version/
codex-version/
gemini-version/
cursor-version/
```

---

# 6. Technology Stack

This is a desktop/local developer tool.

Do NOT use FastAPI, ASP.NET Core, or NestJS merely because they are part of the standard web backend profiles.

Use:

```text
Tauri 2
Rust
React
Vite
TypeScript
Tailwind CSS
shadcn/ui
pnpm
```

Responsibilities:

```text
React / TypeScript
    ↓
UI and application presentation

Tauri Commands
    ↓
Typed native boundary

Rust Core
    ↓
Filesystem
Symlinks
Directory scanning
Hashing
Git integration
File watching
Atomic operations
OS-specific behavior
```

Do not run a localhost HTTP backend unless a real requirement appears.

---

# 7. Why Tauri

This product needs:

```text
filesystem access
symlink / junction management
cross-platform path handling
file watching
process execution
system tray
native dialogs
Git command execution
safe atomic file operations
```

These belong in the native layer.

Do not implement privileged filesystem manipulation through browser-style frontend hacks.

---

# 8. Package Management

Frontend:

```text
pnpm
```

Rust:

```text
Cargo
```

Commit both relevant lock files.

Use current stable compatible dependency versions.

Avoid alpha/beta/RC dependencies unless necessary.

---

# 9. No Database Initially

Do NOT introduce PostgreSQL.

Do NOT introduce SQLite in the MVP unless a concrete need appears.

The source of truth is:

```text
Skill files on disk
```

Application configuration can use a simple local configuration file.

Example:

```text
~/.skillsync/config.json
```

or platform-appropriate app configuration directory.

Potential configuration:

```json
{
  "canonicalSkillRoot": "~/.agents/skills",
  "syncMethod": "auto",
  "tools": {},
  "repositories": []
}
```

Do not put mutable Skill content inside a database.

If future features require:

```text
large version history
marketplace index
analytics
complex search
```

SQLite may be evaluated later.

---

# 10. Tool Adapter Architecture

Different coding tools must be implemented through adapters.

Define a clear conceptual interface similar to:

```text
SkillToolAdapter

id
displayName

detectInstallation()

getGlobalSkillLocations()

getProjectSkillLocations()

scanSkills()

validateSkill()

installSkill()

removeManagedSkill()

supportsSymlink()

reloadInstructions()
```

This is conceptual.

Choose idiomatic Rust interfaces rather than mechanically reproducing this pseudo-interface.

Each tool adapter owns knowledge about that tool.

Do NOT scatter code like:

```text
if tool == "claude"
if tool == "codex"
if tool == "gemini"
```

throughout the application.

---

# 11. Initial Tool Adapters

Implement initial adapters for:

```text
Claude Code
Codex
Cursor
Gemini CLI
```

Each adapter should:

1. detect whether the tool appears installed/configured
2. discover known Skill locations
3. identify existing Skills
4. determine whether each Skill is managed by SkillSync
5. expose synchronization capability
6. validate destination safety

Paths should be based on documented behavior but remain overridable.

Do not assume tool filesystem conventions will never change.

---

# 12. Scope Model

Skills can exist at different scopes.

Initially distinguish:

```text
Global / User Skill
Project Skill
```

MVP should prioritize:

```text
Global / User Skills
```

because they are the primary synchronization use case.

Project-level synchronization can be added after global synchronization is reliable.

Do not combine user-level and project-level Skills into ambiguous state.

---

# 13. Synchronization Model

Support three synchronization methods:

```text
auto
symlink
copy
```

## Auto

Recommended default.

SkillSync decides the safest available mechanism for the target platform/tool.

Prefer linking where appropriate.

Fall back to copy when linking is unavailable or unsafe.

---

# 14. Symlink Mode

Conceptually:

```text
Canonical:

~/.agents/skills/git-commit/

            │
            │ symlink
            ▼

~/.claude/skills/git-commit/
```

Benefits:

```text
single physical copy
instant updates
no copy drift
```

When a target tool directly supports the canonical directory, do not create redundant links.

Example:

If a tool already reads:

```text
~/.agents/skills/
```

there may be nothing to synchronize physically.

SkillSync should recognize this state as:

```text
Native
```

or:

```text
Already Shared
```

rather than creating duplicate directories.

---

# 15. Copy Mode

Copy mode is a compatibility fallback.

If used:

SkillSync must track whether the target copy still matches the canonical Skill.

Never assume copied directories remain synchronized.

Use content fingerprints to detect drift.

---

# 16. Skill Fingerprint

Calculate a deterministic fingerprint for every Skill.

The fingerprint must cover the complete relevant Skill directory tree.

Example inputs:

```text
relative path
file content
```

Use a stable cryptographic hash such as SHA-256.

Ignore irrelevant filesystem metadata such as:

```text
mtime
creation timestamp
OS-specific metadata
```

unless specifically needed.

The same Skill content should produce the same fingerprint across Windows, macOS, and Linux.

---

# 17. Sync States

Every Skill / Tool combination should have a clear state.

Use states conceptually similar to:

```text
Native

Synced

Not Installed

Disabled

Modified

Conflict

Unmanaged

Unavailable
```

UI must make these states understandable.

Do not represent everything as a generic boolean.

---

# 18. Conflict Detection

Conflict handling is one of the most important features.

Example:

Canonical:

```text
~/.agents/skills/git-commit/
```

Claude target:

```text
~/.claude/skills/git-commit/
```

If both exist and content differs:

DO NOT overwrite automatically.

Mark:

```text
Conflict
```

Offer explicit actions such as:

```text
Compare

Use Canonical

Import Target Version

Keep Both

Ignore
```

Never silently destroy a user-edited Skill.

---

# 19. First Import

When SkillSync starts for the first time:

scan all supported tool Skill directories.

Example result:

```text
Claude Code

creating-git-commits
frontend-design
debugging

Codex

creating-git-commits
system-design

Gemini

frontend-design
```

Then determine:

```text
same Skill
duplicate Skill
different Skill with same name
unique Skill
```

Present an import plan before modifying files.

---

# 20. Unmanaged Skills

A Skill found inside a target application directory but not controlled by SkillSync is:

```text
Unmanaged
```

The user may:

```text
Import into SkillSync

Leave unmanaged

Compare with canonical Skill

Replace with managed version
```

Never delete unmanaged Skills automatically.

---

# 21. Duplicate Detection

Do not detect duplicates by directory name alone.

Consider:

```text
Skill name
SKILL.md metadata
directory fingerprint
content similarity when useful
```

Two Skills named:

```text
code-review
```

may contain completely different workflows.

Do not automatically merge them.

---

# 22. Skill Validation

Validate Skills before installation.

At minimum verify:

```text
Skill directory exists

SKILL.md exists

SKILL.md is readable

YAML frontmatter is parseable when present

required common metadata exists when required

referenced relative resources do not escape the Skill directory
```

Validation should produce:

```text
errors
warnings
tool compatibility notes
```

Do not mutate a Skill merely to make validation pass.

---

# 23. Compatibility Analysis

A Skill may contain tool-specific extensions.

The UI should eventually show compatibility information:

```text
creating-git-commits

Claude Code       Compatible
Codex             Compatible
Cursor            Compatible
Gemini CLI        Compatible
```

or:

```text
Claude Code       Full
Codex             Partial
Gemini CLI        Partial
```

Do not pretend all tool-specific metadata behaves identically.

Preserve unsupported metadata instead of deleting it.

---

# 24. Skill List UI

The main screen should be Skill-centric.

Example:

```text
Skills
────────────────────────────────────────────

creating-git-commits

Claude   ✓
Codex    ✓
Cursor   ✓
Gemini   ✓

Source
~/.agents/skills/creating-git-commits

Status
Synced
```

Each row/card should make it easy to understand:

```text
Skill name
Description
Source
Sync status
Enabled tools
Conflicts
Git status if relevant
```

Avoid a dashboard full of meaningless metrics.

---

# 25. Tool Matrix

Provide a matrix view.

Example:

```text
                     Claude   Codex   Cursor   Gemini

git-commit             ✓        ✓       ✓        ✓

tdd                    ✓        ✓       ✓        -

frontend-design        ✓        ✓       ✓        ✓

pdf                    ✓        -       ✓        -
```

Users should be able to enable/disable synchronization per:

```text
Skill × Tool
```

---

# 26. Skill Detail Page

A Skill detail view should show:

```text
Name

Description

Canonical location

SKILL.md preview

Files

Fingerprint

Source repository

Target tools

Compatibility

Sync status

Conflicts
```

Allow opening the directory in the OS file explorer.

Do not build a full IDE/editor in the first version.

A lightweight Markdown preview is sufficient.

External editors should remain first-class.

---

# 27. Enable / Disable

For every Skill allow:

```text
Enable for Claude

Enable for Codex

Enable for Cursor

Enable for Gemini
```

Disabling a Skill for a tool means:

remove only the SkillSync-managed installation/link for that tool.

Do not delete the canonical Skill.

Do not delete unmanaged user data.

---

# 28. Managed Ownership

SkillSync must know whether it owns a target installation.

Never infer ownership merely because:

```text
directory name matches
```

For symlinks, ownership can often be determined from the link target.

For copied installations, maintain minimal metadata outside the Skill whenever possible.

Do not inject SkillSync-specific files into third-party Skill packages unless necessary.

---

# 29. Safe Filesystem Operations

Filesystem safety is critical.

Use:

```text
temporary write
fsync where appropriate
atomic rename
backup before destructive replacement
```

Validate canonicalized paths.

Protect against:

```text
path traversal
recursive symlinks
link loops
accidental home-directory deletion
filesystem root deletion
```

Never execute recursive deletion on a path unless ownership and path boundaries have been verified.

---

# 30. Never Destroy User Files Silently

This is a non-negotiable rule.

Before replacing:

```text
existing non-managed directory
modified copied Skill
conflicting Skill
```

require explicit resolution.

Default behavior should be:

```text
detect
report
ask
```

not:

```text
overwrite
```

---

# 31. Backup

Before operations that replace user-controlled files:

create a lightweight backup.

Example:

```text
~/.skillsync/backups/
```

Backups should include enough metadata to understand:

```text
when
which tool
which Skill
original path
```

Do not implement an enterprise backup system.

Keep this simple.

---

# 32. Automatic Synchronization

After the basic manual sync is reliable, support optional automatic synchronization.

Possible model:

```text
Canonical Skill changes
        ↓
Filesystem watcher
        ↓
Debounce
        ↓
Fingerprint update
        ↓
Sync managed copy targets
```

Symlinked targets require no file copying.

Do not implement automatic background sync before conflict handling is correct.

Correctness > real-time synchronization.

---

# 33. File Watcher

Watch only directories SkillSync owns or explicitly manages.

Debounce filesystem events.

Editors often generate multiple temporary files during one save.

Avoid reacting to every raw filesystem event independently.

Watcher failures must not cause data loss.

A manual:

```text
Refresh
```

and:

```text
Sync Now
```

must always remain available.

---

# 34. Cross-Machine Synchronization

Separate two concepts:

```text
Tool Sync
```

and:

```text
Machine Sync
```

Tool Sync:

```text
Canonical Skill Store
    ↓
Claude / Codex / Gemini / Cursor
```

Machine Sync:

```text
Computer A
    ↕
Git / Cloud Folder
    ↕
Computer B
```

Do not mix these into one synchronization algorithm.

---

# 35. Git Integration

Git is the preferred optional cross-machine synchronization mechanism.

A canonical Skill directory may itself be a Git repository.

Support useful operations eventually such as:

```text
Repository status

Pull

Commit

Push

View changed Skills
```

But do NOT attempt to implement a complete Git client.

Use system Git initially.

Git actions must be explicit.

Do not automatically commit or push without user configuration.

---

# 36. Git-Based Skill Repository

A recommended setup may be:

```text
~/Developer/agent-skills/
├── .git/
│
├── creating-git-commits/
│   └── SKILL.md
│
├── debugging/
│   └── SKILL.md
│
└── frontend-design/
    └── SKILL.md
```

SkillSync may use this folder as its canonical store.

This makes Skills:

```text
portable
version controlled
machine-syncable
reviewable
backup-friendly
```

---

# 37. Remote Skill Installation

After local synchronization is stable, support installation from:

```text
GitHub repository

Git URL

local folder

ZIP
```

This is NOT required for the first vertical slice.

Security review is required before executing bundled scripts.

Installing a Skill does not imply executing the Skill's scripts.

---

# 38. Skill Sources

Eventually each Skill may have source metadata such as:

```text
Local

Git repository

GitHub repository

Imported from Claude

Imported from Codex

Imported from Gemini
```

Source metadata must not become authoritative over actual files.

The files remain the truth.

---

# 39. Updating Remote Skills

Do not automatically replace locally modified Skills with upstream content.

When an upstream version changes:

show:

```text
Update available
```

and detect whether local modifications exist.

Possible states:

```text
Clean update

Local modifications

Conflict
```

Never blindly overwrite local changes.

---

# 40. CLI

Provide a small CLI using the same Rust core as the GUI.

Do NOT implement separate business logic for CLI and GUI.

Potential commands:

```bash
skillsync list

skillsync tools

skillsync scan

skillsync sync

skillsync status

skillsync doctor
```

Later:

```bash
skillsync install <source>

skillsync enable <skill> --tool claude

skillsync disable <skill> --tool gemini

skillsync import --tool claude

skillsync diff <skill> --tool claude
```

The CLI should be script-friendly.

Support meaningful exit codes.

---

# 41. GUI and CLI Architecture

Architecture:

```text
             Rust Core
                │
        ┌───────┴────────┐
        │                │
        ▼                ▼
    Tauri GUI           CLI
```

The following logic must exist only once:

```text
Skill scanning

Fingerprinting

Tool adapters

Synchronization

Conflict detection

Validation

Git integration
```

Do not implement it separately in TypeScript and Rust.

---

# 42. Doctor Command

Provide a diagnostic capability:

```text
skillsync doctor
```

It should inspect:

```text
OS

canonical Skill directory

supported tools detected

target Skill directories

symlink capability

Git availability

broken symlinks

duplicate Skills

conflicts

permissions
```

The GUI should expose the same diagnostic information.

This will be particularly useful for Windows path/symlink issues.

---

# 43. Cross-Platform Requirements

Support:

```text
Windows
macOS
Linux
```

Never manually concatenate paths with:

```text
/
```

Use platform-safe path APIs.

Handle:

```text
home directory resolution

Windows drive letters

UNC paths where relevant

case-sensitive filesystems

case-insensitive filesystems

symlink permissions

junction differences
```

Do not assume Linux filesystem behavior on Windows.

---

# 44. Windows Symlink Strategy

Windows may have different symlink permission/environment behavior.

The synchronization layer must gracefully fall back.

Conceptually:

```text
Auto
  ↓
Can create safe directory symlink?
  ├─ Yes → symlink
  └─ No
      ↓
   supported alternative?
      ├─ Yes → use it
      └─ No → copy
```

Do not require users to enable unsafe workarounds merely to use SkillSync.

---

# 45. Tool Reload Behavior

Different tools may detect Skill changes differently.

Adapters should provide reload guidance.

Example UI:

```text
Claude Code
Changes detected automatically

Codex
Changes detected automatically / restart if necessary

Gemini CLI
Refresh command available
```

Do not encode reload instructions globally.

Each adapter owns them.

---

# 46. Tool Path Configuration

Every adapter must have:

```text
Auto Detect
```

and:

```text
Manual Override
```

because users may use:

```text
custom HOME

WSL

portable installations

containers

remote environments

custom configuration directories
```

Never make path detection impossible to override.

---

# 47. WSL

Treat Windows and WSL as separate environments initially.

Example:

```text
Windows host

C:\Users\victor\...

WSL

/home/victor/...
```

Do not automatically synchronize across the Windows/WSL filesystem boundary unless the user configures it.

Future environments may be modeled as:

```text
Local Windows

WSL Ubuntu

Remote SSH

Container
```

But do not implement remote environments in the MVP.

---

# 48. Security Model

Skills are not merely text documents.

They may include:

```text
shell scripts
Python scripts
Node scripts
templates
configuration
instructions that cause an Agent to execute tools
```

Treat downloaded Skills similarly to downloaded code.

Do NOT automatically execute bundled scripts during:

```text
scan

import

install

sync

validation
```

Validation should inspect files, not execute arbitrary Skill code.

---

# 49. Secret Detection

Do not sync obvious credentials intentionally stored accidentally in Skill directories without warning.

A future lightweight security scan may detect common secret patterns.

Do NOT build a full security scanner in the MVP.

At minimum ensure SkillSync itself never logs secrets found inside Skill files.

---

# 50. Logging

Use structured native logging.

Logs may include:

```text
Skill name

tool

operation

path

status

error code
```

Avoid logging complete Skill contents.

Do not log environment secrets.

---

# 51. UI Navigation

Initial application navigation:

```text
Skills

Tools

Conflicts

Repositories

Settings
```

For MVP:

```text
Skills
Tools
Settings
```

may be enough.

Do not create empty navigation sections before their features exist.

---

# 52. Skills Page

The Skills page is the primary page.

Support:

```text
search

status filters

tool filters

refresh

sync all
```

Potential filters:

```text
All

Synced

Conflict

Unmanaged

Modified
```

---

# 53. Tools Page

Show detected tools.

Example:

```text
Claude Code
Detected

Skill location:
~/.claude/skills

Managed Skills:
12
```

```text
Codex
Detected

Skill location:
~/.agents/skills

Native canonical store
```

Allow:

```text
Enable integration

Disable integration

Change path

Run scan
```

---

# 54. Conflicts UX

Conflicts must be highly visible but not alarming.

Example:

```text
creating-git-commits

Canonical
Modified 10 minutes ago

Claude Code
Modified 2 minutes ago

[Compare]
[Use Canonical]
[Import Claude Version]
```

Never resolve a conflict automatically based solely on modification timestamps.

Content is more important than timestamps.

---

# 55. Compare View

A later vertical slice should provide directory-aware diff.

At minimum:

```text
Added files

Removed files

Modified files
```

For text files:

show textual diff.

Do not build binary diff functionality.

---

# 56. First-Run Experience

First launch should:

1. detect supported tools
2. detect canonical Skill candidates
3. scan existing tool Skill directories
4. show findings
5. ask user to choose/import the canonical collection
6. preview planned filesystem changes
7. apply only after confirmation

Never reorganize the user's existing Skills immediately on launch.

---

# 57. Import Strategy

If the user already has Skills across multiple tools:

build an import plan.

Example:

```text
Found:

Claude       8
Codex       10
Cursor       4
Gemini       6

Unique Skills       14
Exact duplicates     9
Conflicts             2
```

Allow the user to review conflicts before creating the canonical store.

---

# 58. Dry Run

Filesystem synchronization commands should support a dry-run concept.

Example:

```bash
skillsync sync --dry-run
```

Output:

```text
CREATE LINK
~/.claude/skills/tdd
→ ~/.agents/skills/tdd

NO CHANGE
creating-git-commits

CONFLICT
frontend-design
```

The GUI should similarly preview risky bulk operations.

---

# 59. Atomic Bulk Operations

For:

```text
Sync All
Import All
Enable All
```

first calculate an operation plan.

Validate the entire plan.

Then execute.

If part of the operation fails:

report exactly what succeeded and failed.

Do not leave status ambiguous.

Do not pretend filesystem operations across multiple directories are globally transactional when they are not.

---

# 60. Application State Model

Separate:

```text
Observed State
```

from:

```text
Desired State
```

Example:

Desired:

```text
creating-git-commits enabled for Claude
```

Observed:

```text
Claude path contains outdated copy
```

Result:

```text
Modified / Needs Sync
```

This model will make synchronization logic significantly clearer.

---

# 61. Do Not Over-Engineer

Do not initially introduce:

```text
PostgreSQL
Redis
Web server
cloud backend
user accounts
microservices
message broker
CRDT
custom synchronization protocol
embedded Git implementation
plugin framework
```

The first release is a local desktop application.

Use ordinary code.

---

# 62. Extensibility Without Plugin Over-Engineering

Tool adapters should be cleanly separated.

However:

Do NOT build a dynamic plugin marketplace/API for adapters in the MVP.

Adding a new tool adapter in source code should be straightforward.

That is enough initially.

---

# 63. Repository Structure

Recommended structure:

```text
skillsync/
├── apps/
│   └── desktop/
│       ├── src/
│       ├── src-tauri/
│       └── ...
│
├── crates/
│   ├── skillsync-core/
│   └── skillsync-cli/
│
├── docs/
├── scripts/
│
├── AGENTS.md
├── README.md
├── package.json
└── pnpm-lock.yaml
```

Exact Rust workspace structure may be adjusted if a simpler idiomatic structure is preferable.

Repository conventions should remain understandable to both Rust and TypeScript developers.

---

# 64. Rust Core Modules

A possible initial organization:

```text
core/
  skill/
  sync/
  fingerprint/
  conflict/
  filesystem/
  config/

adapters/
  claude/
  codex/
  cursor/
  gemini/
```

Do not create empty modules in advance.

Create them as vertical slices require them.

---

# 65. Frontend Structure

Use feature-oriented organization.

Example:

```text
src/
├── components/
│   └── ui/
│
├── features/
│   ├── skills/
│   ├── tools/
│   └── settings/
│
├── lib/
├── hooks/
├── routes/
└── types/
```

Use:

```text
shadcn/ui
```

for UI primitives.

Avoid large generic:

```text
utils/
services/
```

directories containing unrelated logic.

---

# 66. State Management

Native filesystem state is server-like external state from the React perspective.

Keep native data access behind typed Tauri commands.

Do not allow React components to know filesystem implementation details.

Use React state for simple UI state.

Introduce Zustand only if real complex client state emerges.

Do not install TanStack Query merely because it is part of the standard web stack unless it simplifies native async state meaningfully.

For an MVP, a thin typed native data layer may be sufficient.

---

# 67. Typed Tauri Boundary

All frontend/native communication must be typed.

Avoid ad-hoc:

```text
invoke("something", arbitraryObject)
```

spread throughout components.

Provide a small TypeScript API layer around Tauri commands.

Example conceptual API:

```text
skills.list()

skills.scan()

skills.sync()

tools.list()

conflicts.resolve()
```

Keep native command naming stable and explicit.

---

# 68. Error Model

Native operations should return structured errors.

Conceptually:

```text
code
message
path
tool
skill
recoverable
```

Examples:

```text
PERMISSION_DENIED

TARGET_CONFLICT

BROKEN_SYMLINK

INVALID_SKILL

TOOL_NOT_FOUND

GIT_NOT_FOUND
```

Frontend should not parse Rust error strings to determine behavior.

---

# 69. Testing

Rust core:

```text
cargo test
```

Prioritize tests for:

```text
fingerprinting

path safety

skill scanning

duplicate detection

conflict detection

sync planning

adapter behavior

copy synchronization

symlink synchronization
```

Use temporary directories for filesystem tests.

Do not modify real user Skill directories during automated tests.

---

# 70. Adapter Contract Tests

Every tool adapter should pass common behavior tests.

Examples:

```text
detect missing directory safely

scan valid Skill

ignore unrelated files

identify managed symlink

detect conflicting directory

never delete unmanaged Skill
```

This helps adding new tools remain reliable.

---

# 71. Frontend Testing

Use:

```text
Vitest
Testing Library
```

Use Playwright only for important application flows when Tauri testing infrastructure makes it practical.

Prioritize Rust core correctness over exhaustive UI snapshots.

---

# 72. Test Fixtures

Create fixture Skills.

Example:

```text
fixtures/
├── basic-skill/
├── multi-file-skill/
├── codex-metadata-skill/
├── invalid-frontmatter/
└── conflicting-skill/
```

Fixtures must not contain real credentials.

---

# 73. AGENTS.md

Create a concise:

```text
AGENTS.md
```

Document:

```text
Repository structure

Build commands

Test commands

Frontend commands

Rust commands

Architecture boundaries

Tool Adapter rules

Filesystem safety rules

Canonical Skill rule

Conflict rules

Generated code policy if any
```

Important rules should include:

```text
Never overwrite unmanaged Skills.

Never implement tool-specific logic outside adapters without a strong reason.

Never execute Skill scripts during scan/sync.

Canonical Skills are authoritative only after explicit import/adoption.

Filesystem mutations require safety validation.
```

Keep AGENTS.md concise and actionable.

---

# 74. Definition of Done

Before claiming work is complete:

Frontend:

```bash
pnpm lint
pnpm typecheck
pnpm test
pnpm build
```

Rust:

```bash
cargo fmt --check
cargo clippy --all-targets --all-features
cargo test
```

Tauri:

verify the application builds for the current development platform.

Do not claim:

```text
working
fixed
all tests pass
```

without running the relevant verification.

---

# 75. MVP Vertical Slices

Do NOT build the complete product at once.

## Slice 1 — Discovery

Implement:

```text
Detect Claude / Codex / Cursor / Gemini

Discover Skill directories

Scan Skills

Parse SKILL.md metadata

Display all discovered Skills
```

No filesystem mutation except configuration.

---

## Slice 2 — Canonical Store

Implement:

```text
Choose canonical Skill root

Import one Skill

Fingerprint Skill

Display canonical Skills
```

Protect existing files.

---

## Slice 3 — One-Way Sync

Start with:

```text
Canonical
    ↓
Claude Code
```

Implement:

```text
sync plan

symlink

copy fallback

dry run

safe removal of managed target
```

Do not implement Sync All until one adapter works correctly.

---

## Slice 4 — Multi-Tool Sync

Add:

```text
Codex
Cursor
Gemini
```

Recognize tools that natively consume the canonical directory.

Add Skill × Tool enablement matrix.

---

## Slice 5 — Conflict Management

Implement:

```text
Modified target detection

Unmanaged Skill detection

Conflict view

Compare

Use Canonical

Import Target
```

No silent overwrite.

---

## Slice 6 — Automatic Sync

Only after conflict handling is trustworthy:

```text
filesystem watcher

debounced change detection

automatic copy-target refresh
```

---

## Slice 7 — Git Sync

Add optional:

```text
Git repository detection

status

pull

commit

push
```

Keep Git operations explicit.

---

# 76. MVP Non-Goals

Do NOT initially implement:

```text
Skill marketplace

cloud account

team collaboration

remote SSH sync

WebDAV

GitHub OAuth

custom adapter plugins

AI-generated Skills

automatic Skill rewriting

full Git GUI

Skill execution

MCP synchronization

Prompt synchronization

provider switching
```

These can be evaluated after the core Skill synchronization experience works reliably.

---

# 77. Future Features

Architecture may allow future additions such as:

```text
GitHub Skill installation

Skill repository browser

Skill update detection

Skill version history

Profiles

Work / Personal Skill collections

Remote machines

WSL environments

Skill compatibility analyzer

Skill linting

Skill security review

Export / Import bundle

Skill marketplace integration
```

Do not implement these prematurely.

---

# 78. Profiles

A useful later feature is Skill Profiles.

Example:

```text
General Development

creating-git-commits
systematic-debugging
tdd
code-review
```

```text
Frontend

frontend-design
react-patterns
accessibility
```

```text
.NET Industrial

dotnet
opcua
grpc
industrial-testing
```

A Profile represents desired Skill enablement.

Do not physically duplicate Skills between Profiles.

---

# 79. Core Invariants

These invariants must always hold:

1. A managed Skill has one canonical source.

2. Synchronization must never silently destroy unmanaged user data.

3. A target modification must be detected before overwriting it.

4. Skill directories are synchronized as complete packages, not only `SKILL.md`.

5. Tool-specific behavior belongs in adapters.

6. The same synchronization engine serves GUI and CLI.

7. Skill scripts are never executed merely by scanning/installing/syncing.

8. Filesystem paths are validated before destructive operations.

9. Symlinks must not create recursive loops.

10. Tool Sync and Machine Sync are separate concepts.

11. The application must remain useful without any cloud service.

12. Avoid architecture that requires SkillSync to remain running for linked Skills to work.

---

# 80. Coding Agent Working Method

This is a new architectural project.

Before implementation:

1. Inspect the repository if it already exists.

2. Read `AGENTS.md` if present.

3. Verify current stable documentation for Tauri and supported Skill tools when tool-specific behavior matters.

4. Present a concise architecture proposal.

5. Identify assumptions that affect filesystem safety.

6. Implement one vertical slice at a time.

Do not scaffold every future subsystem.

For architectural decisions with irreversible filesystem consequences:

present the design before implementing the destructive behavior.

---

# 81. Initial Task

Start this project by doing ONLY the following first:

1. Propose the MVP architecture in approximately 20 lines or fewer.

2. Show the proposed repository structure.

3. Define the `Skill`, `Tool`, `SkillLocation`, `SyncState`, and `SyncPlan` domain concepts.

4. Define the Tool Adapter boundary.

5. Design the filesystem safety rules.

6. Design the canonical-store and conflict semantics.

7. Identify the first vertical slice:

```text
read-only discovery
```

8. List only the dependencies required for that slice.

Do not implement synchronization mutations until read-only discovery is working and tested.

After presenting this design, follow the project's normal design/implementation approval workflow before modifying the repository.