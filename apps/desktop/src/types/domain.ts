/**
 * Domain types mirroring `skillsync-core`'s serde output (camelCase).
 * This is the single contract between the Rust core and the React UI;
 * components must not know anything about `invoke` or the filesystem.
 */

export type SyncState =
  | "native"
  | "synced"
  | "notInstalled"
  | "disabled"
  | "modified"
  | "conflict"
  | "unmanaged"
  | "unavailable";

export type ValidationSeverity = "error" | "warning" | "note";

export interface ValidationIssue {
  severity: ValidationSeverity;
  message: string;
  code: string;
  file?: string;
}

export type Managedness =
  | { kind: "unmanaged" }
  | { kind: "managedSymlink"; canonicalPath: string }
  | { kind: "foreignSymlink"; target: string }
  | { kind: "nativeShared" }
  | { kind: "brokenSymlink" };

export interface Installation {
  toolId: string;
  toolDisplayName: string;
  path: string;
  displayPath: string;
  state: SyncState;
  managedness: Managedness;
  fingerprint?: string;
  validation: ValidationIssue[];
}

export interface CanonicalInfo {
  path: string;
  displayPath: string;
  fingerprint?: string;
  validation: ValidationIssue[];
}

export interface SkillRow {
  key: string;
  name: string;
  description?: string;
  canonical?: CanonicalInfo;
  installations: Installation[];
  status: SyncState;
}

export type LocationKind = "standard" | "agentStandard";
export type SymlinkSupport = "preferred" | "supported" | "avoided";

export interface ToolDetection {
  installed: boolean;
  evidence: string;
  configDir?: string;
}

export interface LocationInfo {
  path: string;
  displayPath: string;
  kind: LocationKind;
  overridden: boolean;
  exists: boolean;
  nativeCanonical: boolean;
  skillCount: number;
  managedCount: number;
  unmanagedCount: number;
}

export interface ReloadGuidance {
  summary: string;
  detail: string;
}

export interface ToolInfo {
  id: string;
  displayName: string;
  detection: ToolDetection;
  enabled: boolean;
  locations: LocationInfo[];
  symlinkSupport: SymlinkSupport;
  reloadGuidance: ReloadGuidance;
  skillCount: number;
  managedCount: number;
}

export interface SkillOverview {
  canonicalRoot: string;
  canonicalRootDisplay: string;
  canonicalRootExists: boolean;
  tools: ToolInfo[];
  rows: SkillRow[];
}

export type SyncMethod = "auto" | "symlink" | "copy";

export interface ToolOverride {
  enabled?: boolean;
  globalSkillPath?: string;
}

export interface Config {
  canonicalSkillRoot: string;
  syncMethod: SyncMethod;
  tools: Record<string, ToolOverride>;
  repositories: unknown[];
  autoSync: boolean;
}

export type CheckStatus = "ok" | "warning" | "error";

export interface DoctorCheck {
  id: string;
  title: string;
  status: CheckStatus;
  detail: string;
}

export interface DoctorReport {
  os: string;
  skillsyncHome: string;
  checks: DoctorCheck[];
}

/** Planned action for one import (mirrors core `ImportAction`). */
export type ImportAction =
  | { kind: "create"; target: string }
  | { kind: "alreadyPresent"; target: string }
  | { kind: "keepBoth"; target: string }
  | { kind: "replace"; target: string; backupDir: string }
  | { kind: "conflict"; target: string };

export interface ImportPlan {
  source: string;
  canonicalRoot: string;
  skillId: string;
  action: ImportAction;
  fingerprint?: string;
  notes: string[];
}

export interface ImportOutcome {
  actionTaken: ImportAction;
  target: string;
  fingerprint?: string;
  dryRun: boolean;
}

export type ImportResolution = "skip" | "keepBoth" | "replace";

/** One-way sync (canonical store → tool), mirroring core `sync` types. */
export type EffectiveMethod = "symlink" | "copy";

export type PlanAction =
  | { kind: "createLink"; target: string; source: string }
  | { kind: "createCopy"; target: string; source: string }
  | { kind: "updateCopy"; target: string; source: string; backupDir: string }
  | { kind: "repairLink"; target: string; source: string }
  | { kind: "noChange"; target?: string }
  | { kind: "native" }
  | { kind: "skip"; target?: string; reason: string };

export interface PlanEntry {
  skillId: string;
  skillName: string;
  action: PlanAction;
  displayTarget: string;
  notes: string[];
}

export interface SyncPlan {
  toolId: string;
  toolDisplayName: string;
  method: EffectiveMethod;
  canonicalRoot: string;
  canonicalRootDisplay: string;
  targetDir?: string;
  entries: PlanEntry[];
}

export interface EntryOutcome {
  skillId: string;
  actionKind: string;
  ok: boolean;
  error?: string;
  backupDir?: string;
}

export interface SyncRunReport {
  toolId: string;
  method: EffectiveMethod;
  dryRun: boolean;
  succeeded: EntryOutcome[];
  failed: EntryOutcome[];
}

/** Structured native error (design doc §68). */
export interface SkillSyncError {
  code: string;
  message: string;
  path?: string;
  tool?: string;
  skill?: string;
  recoverable: boolean;
}

export const STATUS_LABELS: Record<SyncState, string> = {
  native: "Native",
  synced: "Synced",
  notInstalled: "Not installed",
  disabled: "Disabled",
  modified: "Modified",
  conflict: "Conflict",
  unmanaged: "Unmanaged",
  unavailable: "Unavailable",
};

export const STATUS_MARKS: Record<SyncState, string> = {
  native: "✓",
  synced: "✓",
  notInstalled: "-",
  disabled: "-",
  modified: "~",
  conflict: "!",
  unmanaged: "u",
  unavailable: "×",
};
