# SkillSync Architecture

Status: design for MVP, Slice 1 (read-only discovery) implemented.

## 1. MVP Architecture Summary

```text
React/TS UI (Vite + Tailwind + shadcn/ui)      CLI (clap)
        │  typed Tauri commands only                    │
        └───────────────┬───────────────────────────────┘
                        ▼
              skillsync-core (Rust)  ← single implementation of all logic
   config · skill model · tool adapters · scanning · fingerprint
   sync engine · conflict detection · validation · fs-safety · doctor
                        ▼
        Canonical Skill Store (~/.agents/skills, configurable)
                        ▼   derived installations (symlink | copy | native)
   Claude Code (~/.claude/skills) · Codex (~/.codex/skills)
   Cursor (~/.cursor/skills, also reads ~/.agents/skills natively)
   Gemini CLI (~/.gemini/skills)      + future tools via adapters only
```

No database (files on disk are the truth), no HTTP backend, no cloud service.
Tool Sync (store → tools) and Machine Sync (store ↔ git/cloud) are separate concepts.

## 2. Repository Structure

```text
skillsync/
├── apps/desktop/            # Tauri 2 GUI
│   ├── src/                 # React: features/{skills,tools,settings}, lib/, types/
│   └── src-tauri/           # thin native boundary over skillsync-core
├── crates/
│   ├── skillsync-core/      # all business logic (used by GUI and CLI)
│   └── skillsync-cli/       # clap-based CLI
├── docs/
├── fixtures/                # test fixture skills (no real credentials)
├── scripts/
├── AGENTS.md
├── README.md
├── package.json             # pnpm workspace root (frontend tooling)
└── pnpm-lock.yaml
```

The Cargo workspace (root `Cargo.toml`) contains the two crates and the
Tauri app crate. pnpm workspace contains only the desktop frontend.

## 3. Domain Concepts

Defined in `skillsync-core/src/skill.rs` and `adapter.rs`:

| Concept | Meaning |
|---|---|
| `Skill` | A directory anchored by `SKILL.md` (plus optional scripts/references/assets/agents). One canonical root path + metadata + validation results + fingerprint. |
| `SkillScope` | `Global` (user-level) or `Project` (inside a project dir). MVP: global first. |
| `SkillSource` | `Canonical` (lives in the store) or `Observed` (found inside a tool location, possibly unmanaged). |
| `Tool` | One supported coding tool, represented only by its `ToolAdapter` implementation (id, display name, detection, locations, capabilities). |
| `SkillLocation` | A concrete directory a tool reads skills from: `(tool_id, scope, path, role)` where role is `Native` when the tool reads the canonical root itself. |
| `Managedness` | For an observed installation: `Unmanaged` (real dir, not ours), `ManagedSymlink { target }`, `ForeignSymlink { target }`, `NativeShared` (location is the canonical root). Ownership is never inferred from the name alone. |
| `SyncState` | Per Skill×Tool: `Native`, `Synced`, `NotInstalled`, `Disabled`, `Modified`, `Conflict`, `Unmanaged`, `Unavailable`. Slice 1 populates the read-only-derivable states; `Modified`/`Conflict` require the canonical store (Slices 2/5). |
| `SyncPlan` | The complete, validated, previewable set of filesystem actions for one sync run (create link, copy, remove managed, no-change, conflict). Produced by the sync engine (Slice 3); supports dry-run; executed only after validation. |

## 4. Tool Adapter Boundary

`skillsync-core/src/adapter/mod.rs` defines `ToolAdapter`, a read-only
discovery + capability interface (Slice 1):

```rust
pub trait ToolAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    fn detect(&self, env: &EnvContext) -> ToolDetection;
    fn global_skill_location(&self, env: &EnvContext, over: &ToolOverride) -> Option<SkillLocation>;
    fn project_skill_locations(&self, env: &EnvContext, over: &ToolOverride) -> Vec<SkillLocation>; // MVP: may be empty
    fn scan_skills(&self, env: &EnvContext, over: &ToolOverride) -> Result<Vec<ScannedSkill>>;
    fn symlink_support(&self) -> SymlinkSupport;   // Preferred / Supported / Avoided
    fn reload_guidance(&self) -> ReloadGuidance;   // adapter-owned, not global
}
```

Rules:

- Every adapter owns its tool's paths, detection, capabilities and reload
  guidance. No `if tool == "..."` outside the adapter module.
- All paths are auto-detected defaults with manual override via
  `ToolOverride` in `~/.skillsync/config.json` (`tools.<id>.globalSkillPath`).
- Mutation (install/remove/validate-destination) will be added as a separate
  `SkillInstaller` boundary in Slice 3 — not stubbed now.
- New tools = new module + one registry line + contract tests (see below).

Adapter defaults (verified against vendor docs, 2026-08; all overridable):

| Tool | Global skills | Project skills | Symlinks |
|---|---|---|---|
| Claude Code | `~/.claude/skills` | `.claude/skills` | Supported (dir symlinks) |
| Codex | `~/.codex/skills` (`$CODEX_HOME/skills`) | `.codex/skills` | Dir symlinks followed; symlinked `SKILL.md` files are skipped → never link individual files |
| Cursor | `~/.cursor/skills` (also reads `~/.agents/skills` natively) | `.cursor/skills`, `.agents/skills` | Supported |
| Gemini CLI | `~/.gemini/skills` | `.gemini/skills` | Does not follow symlinks for skill discovery → copy fallback |

## 5. Filesystem Safety Rules

1. Every mutating operation resolves and validates canonical paths first;
   refuse to operate on `/`, the home directory itself, or any path outside
   the declared canonical root / tool skill roots.
2. Never execute recursive deletion unless the target is verified as
   SkillSync-managed (symlink ownership via resolved target, or copy managed
   metadata) **and** inside an expected root **and** the removed entry is
   exactly the skill directory.
3. Writes use temp-file + fsync + atomic rename; destructive replacement
   requires an explicit resolution (never overwrite-by-default) and creates a
   backup under `~/.skillsync/backups/<timestamp>-<tool>-<skill>/` first.
4. Symlinks: refuse to create a link whose resolved target escapes the
   canonical store; refuse when the target path already exists (unless it is
   the identical managed link); detect cycles via canonicalization before
   linking.
5. Skills are never executed: scanning/validation only reads file bytes.
6. All operations are reported (plan → validate → execute → report what
   succeeded/failed); dry-run is available for every bulk operation.

## 6. Canonical Store & Conflict Semantics

- Canonical store: one directory (default `~/.agents/skills`, configurable,
  may be a git repo). A skill directory there is the single source of truth;
  it stays portable and usable without SkillSync (no metadata injected into
  skill folders; SkillSync state lives in `~/.skillsync/`).
- A tool that natively reads the canonical root (e.g. Cursor reads
  `~/.agents/skills`) is recognized as `Native` — no link, no copy.
- Adoption is explicit: importing an observed skill into the store is a
  user-confirmed operation (Slice 2). Until adopted, everything observed in
  tool directories is `Unmanaged` and is never modified or deleted.
- Fingerprints (deterministic SHA-256 over relative paths + contents,
  metadata-independent) drive: copy drift detection, duplicate detection
  (same name ≠ same skill), and conflict detection.
- Conflict = canonical exists AND a non-managed/modified target exists AND
  fingerprints differ. Never auto-resolve; offer Compare / Use Canonical /
  Import Target Version / Keep Both / Ignore. Timestamps never decide alone.

## 7. First Vertical Slice: Read-Only Discovery (implemented)

- Detect installed tools; discover global skill locations; scan skills.
- Parse SKILL.md frontmatter (name, description, extras) and validate:
  dir exists, SKILL.md exists/readable, frontmatter parses, referenced
  relative resources do not escape the skill dir; errors vs warnings.
- Classify each observed installation's `Managedness`/`SyncState`.
- `skillsync list|tools|scan|doctor` CLI (JSON output, meaningful exit codes).
- Tauri GUI: Skills page (search/filter/refresh), Tools page, Settings
  (config only). No filesystem mutation except `~/.skillsync/config.json`.

Dependencies for this slice only — Rust: `serde`, `serde_json`, `serde_yaml`,
`thiserror`, `dirs`, `walkdir`, `sha2`, `hex`, `clap` (CLI), `tempfile` (dev).
Frontend: React, Vite, TypeScript, Tailwind, shadcn/ui primitives, Tauri API,
Vitest + Testing Library (dev).

## 8. Later Slices (not implemented in this pass)

2 Canonical store + import/fingerprint → 3 one-way sync to Claude (plan,
symlink/copy, dry-run, safe managed removal) → 4 multi-tool + Skill×Tool
matrix → 5 conflict management + compare → 6 automatic sync (watcher,
debounced, copy-target refresh only) → 7 explicit git integration.
Each slice lands only after the previous one is working and tested.
