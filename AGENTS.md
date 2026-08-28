# AGENTS.md

Working rules for coding agents on SkillSync. Full design: `docs/ARCHITECTURE.md`.

## Repository structure

```text
apps/desktop/        Tauri 2 GUI (React/TS frontend in src/, Rust shell in src-tauri/)
crates/skillsync-core/  ALL business logic (single source used by GUI + CLI)
crates/skillsync-cli/   clap CLI over skillsync-core
fixtures/            test fixture skills      docs/  design docs
```

## Commands

```bash
# Rust (run from repo root; toolchain: stable)
cargo fmt --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test

# Frontend (pnpm; Node LTS)
pnpm install
pnpm lint          # eslint
pnpm typecheck     # tsc --noEmit
pnpm test          # vitest
pnpm build         # vite production build
pnpm tauri dev     # run desktop app in dev mode
pnpm tauri build   # build desktop app for current platform
```

Commit both lock files (`pnpm-lock.yaml`, `Cargo.lock`).

## Architecture boundaries

- Business logic exists **only** in `skillsync-core`. Never reimplement
  scanning/sync/conflict/fingerprint logic in TypeScript or in the CLI.
- Tool-specific behavior (paths, detection, capabilities, reload guidance)
  lives **only** in the tool's adapter module
  (`crates/skillsync-core/src/adapter/<tool>.rs`). Never add
  `if tool == "..."` elsewhere; adding a tool = new adapter + registry entry
  + contract tests.
- The Tauri layer is a thin typed boundary: commands delegate to core and
  return structured errors (`code`, `message`, `path`, `tool`, `skill`,
  `recoverable`). The frontend never parses error strings or knows
  filesystem internals; all `invoke` calls go through `src/lib/api.ts`.

## Non-negotiable safety rules

- **Never overwrite or delete unmanaged skills.** Observed target
  installations are `Unmanaged` until explicitly imported/adopted by the
  user. Default behavior: detect → report → ask, never overwrite.
- **One canonical source per managed skill.** Targets are derived
  installations (symlink/copy/native). Never treat tool skill dirs as
  authoritative copies; never fork shared content into per-tool versions.
- **Filesystem mutations require validated paths** (canonicalized, within
  declared roots, not `/` or home), ownership verification before any
  recursive delete, atomic writes (temp + rename), and a backup under
  `~/.skillsync/backups/` before replacing user-controlled files.
- **Never execute skill scripts** during scan/import/install/sync/
  validation. Validation reads bytes only.
- **No symlinks that escape the canonical store, no link cycles**
  (canonicalize before linking; refuse existing non-identical targets).
- Sync state derives from content fingerprints, not mtimes; conflicts are
  resolved only by explicit user choice.
- Keep skill directories portable: no SkillSync metadata inside skill
  folders; SkillSync state lives under `~/.skillsync/`.

## Tests

- Core: `cargo test` (unit + adapter contract tests). Filesystem tests use
  `tempfile` sandboxes; never touch real user skill directories.
- Fixtures live in `fixtures/` (basic-skill, multi-file-skill,
  codex-metadata-skill, invalid-frontmatter, conflicting-skill) and must not
  contain real credentials.
- Frontend: Vitest + Testing Library; test behavior through the typed API
  layer with mocked native calls.

## Generated code

shadcn/ui primitives in `apps/desktop/src/components/ui/` are vendored
generator output — edit only to match the design system, do not restructure.
Everything else is hand-written.
