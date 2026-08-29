# SkillSync Architecture

Status: MVP COMPLETE — Slices 1–7 implemented and gated behind explicit
user actions for every filesystem mutation (discovery, canonical store +
import, one-way sync, Skill×Tool matrix, conflict management, optional
auto-sync off by default, explicit git machine-sync). Beyond-MVP scope is
listed in §8.

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

## 7b. Slice 2: Canonical Store (implemented)

- `skillsync adopt-root` / Settings button: creates the configured
  canonical root when missing (never over an existing file, never `/` or
  home). An explicit user action; nothing is adopted automatically.
- `skillsync import <path> [--dry-run] [--keep-both|--replace]` / GUI
  ImportControl: plan → preview → confirm → execute.
  - Plan is computed first and is fully previewable; dry-run writes nothing.
  - Content identity decides: identical target → no-op; differing target →
    conflict. Conflicts require an explicit resolution (`skip` blocks with
    `TARGET_CONFLICT`, `keep-both` imports as `<name>-2`, `replace` backs up
    to `~/.skillsync/backups/<ts>-canonical-<skill>/` + `<…>.json` metadata
    (when/tool/skill/original path) before any replacement).
  - First import may create the (empty) canonical root; the source
    directory is never modified; tool directories are never touched.
  - Copies preserve the full tree (subdirectories and symlinks).
- Fingerprints (SHA-256 tree hash) drive identity everywhere; timestamps
  never decide (§54).

Dependencies for this slice only — Rust: `serde`, `serde_json`, `serde_yaml`,
`thiserror`, `dirs`, `walkdir`, `sha2`, `hex`, `clap` (CLI), `tempfile` (dev).
Frontend: React, Vite, TypeScript, Tailwind, shadcn/ui primitives, Tauri API,
Vitest + Testing Library (dev).

## 7c. Slice 3: One-Way Sync (implemented)

- `skillsync sync --tool <id> [--dry-run]` / GUI per-tool SyncControl:
  plan → preview → apply/dry-run, per skill × tool.
- Method resolution: config `syncMethod` (auto/symlink/copy) + adapter
  knowledge (Gemini avoids symlinks → copy) + a live platform probe;
  probe failure falls back to copies (§44).
- Plan actions: `createLink`, `createCopy`, `updateCopy` (backup first),
  `repairLink` (managed dangling links only), `noChange`, `native`
  (tool reads the store directly — nothing to install), `skip`
  (unmanaged/conflict/foreign — reported, never touched).
- Ownership (§28): symlinks are owned when they resolve into the canonical
  store; copies only when recorded in `~/.skillsync/managed.json`.
  Unmanaged targets are never modified or deleted; conflicts surface as
  `skip` with a reason and wait for explicit resolution (Slice 5).
- Copy drift is detected by comparing fingerprints against the registry
  record; updates back up the old copy first (§31).
- Reports exactly what succeeded and failed (§59); exit code 1 on failures.

## 7d. Slice 4: Multi-Tool Sync & Skill×Tool Matrix (implemented)

- Enablement matrix (§25/§27): `config.skills.<skillId>.tools.<toolId>`
  (missing = enabled). `skillsync enable|disable <skill> --tool <id>
  [--dry-run]` and clickable matrix chips in the GUI persist the choice
  and apply it: enabling installs that one installation; disabling removes
  ONLY the managed one (registry/link-ownership re-verified at execution;
  unmanaged content is reported and never deleted).
- The overview matrix shows `Disabled` states, derived from the same config.
- `skillsync sync --all [--dry-run]`: one plan + report per detected,
  enabled tool; failures are reported per tool (§59).
- Native-canonical tools honor enablement as a no-op (nothing to install).

## 7e. Slice 5: Conflict Management & Compare (implemented)

- Detection (§18/§21): canonical skill + unmanaged same-name directory with
  differing fingerprints = conflict. Identical copies stay import
  candidates, never conflicts. Managed-copy drift is an `updateCopy` plan
  action, not a conflict.
- Compare (§55): directory-aware diff — added/removed/modified files, with
  line-level text diffs for UTF-8 files (size-capped); binaries report
  without a text diff. `skillsync diff <skill> --tool <id>` / GUI Compare.
- Resolutions (§54), explicit only, backups always: `use-canonical`
  (target backed up, canonical installed as link/copy), `import-target`
  (canonical backed up and replaced by the target content, then the target
  becomes managed), `keep-both` (target imported under `<name>-2`; nothing
  replaced), and `ignore` (recorded per skill×tool; detect/resolve refuse
  until unignored). Managed targets are refused — sync handles those.
- `skillsync conflicts [--json]`, `skillsync resolve … [--dry-run]
  [--ignore]` / GUI Conflicts section above the skill list.

## 7f. Slice 6: Automatic Sync (implemented)

- `watcher.rs`: a background thread watches SkillSync's own home as the
  event anchor; bursts are debounced (2 s quiet) and passes are rate-limited
  (5 s minimum between runs) — editors fire many events per save (§33).
- Every pass re-reads the config: `autoSync` (default **off**) applies
  immediately; passes go through `sync_all`, so only managed targets ever
  change — symlink targets are no-ops, drifted copies are updated with a
  backup (§32). The GUI receives an `auto-sync-ran` event and refreshes;
  manual Refresh/Sync Now always remains available (§33).
- Watcher failure can never lose data: at worst it stops notifying; the
  engine's own validation still guards every mutation.

## 7g. Slice 7: Git Integration (implemented)

- Machine sync (§34) stays a separate concept from tool sync; the canonical
  store itself may be a git repository and the **system git** binary is
  used (no embedded git, no shell interpolation — argument lists only).
- `skillsync git status|pull|commit --message <m>|push` / GUI Git card:
  branch, ahead/behind, changed skills (paths grouped per skill directory),
  and explicit pull `--ff-only` (never merges or overwrites silently),
  explicit commit, explicit push. Nothing git-related runs automatically
  (§35); non-repo stores report gracefully.

## 7i. Slices 8–10: First Import, Detail View, Operation Log

- **First import (§19/§56/§57)** — `firstimport.rs`: every observed skill
  is classified by content — already-canonical (fingerprint match),
  unique imports (one entry per content group), exact-duplicate count, and
  same-name/different-content conflicts (never merged, listed with all
  occurrences). `skillsync import-plan` / `import-all [--dry-run]`; the GUI
  shows a first-run banner with counts + sources and an explicit Import
  button. Applying reuses the import machinery: create-only, never
  overwrites, tool directories untouched, raced entries skipped with a
  reason.
- **Skill detail (§26)** — expandable detail per skill card: fingerprint,
  locations with open-in-file-explorer, read-only size-capped SKILL.md
  preview. Reads are constrained to the canonical store and configured
  tool locations; deliberately not an editor.
- **Operation log (§50)** — JSONL under `~/.skillsync/logs/skillsync.log`:
  operation, tool, skill, path, status, error codes (import / sync /
  resolve / git). No skill contents, no environment secrets (credential-
  like lines are redacted); single-step size rotation; logging is
  best-effort and can never break a user operation.

## 8. Later Slices (not implemented in this pass)

Slices 1–7 are implemented and gated behind explicit user actions for every
filesystem mutation. Beyond the MVP (design doc §76/§77, intentionally NOT
built): skill marketplace, cloud accounts, remote SSH/WSL environments,
remote skill installation (§37 requires a security review first), profiles
(§78), custom adapter plugins, skill execution, MCP/prompt/provider sync.
