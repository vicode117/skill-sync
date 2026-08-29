import { invoke } from "@tauri-apps/api/core";
import type {
  Config,
  ConflictReport,
  DiffEntry,
  DoctorReport,
  FirstImportPlan,
  FirstImportReport,
  GitStatus,
  ImportOutcome,
  ImportPlan,
  ImportResolution,
  SkillOverview,
  SkillSyncError,
  Resolution,
  ResolutionReport,
  SyncPlan,
  SyncRunReport,
} from "@/types/domain";

/** Coerce anything thrown across the native boundary into a SkillSyncError. */
export function normalizeError(e: unknown): SkillSyncError {
  if (typeof e === "string") {
    return { code: "UNKNOWN", message: e, recoverable: true };
  }
  if (e && typeof e === "object" && "message" in e) {
    return e as SkillSyncError;
  }
  return { code: "UNKNOWN", message: String(e), recoverable: true };
}

/**
 * The typed native API layer (design doc §67). Every Tauri invocation in
 * the app goes through here; components never call `invoke` directly.
 */
export const api = {
  getConfig: (): Promise<Config> => invoke("get_config"),
  saveConfig: (config: Config): Promise<Config> => invoke("save_config", { config }),
  scanOverview: (): Promise<SkillOverview> => invoke("scan_overview"),
  runDoctor: (): Promise<DoctorReport> => invoke("run_doctor"),
  adoptCanonicalRoot: (): Promise<{ canonicalRoot: string }> => invoke("adopt_canonical_root"),
  planImport: (sourcePath: string): Promise<ImportPlan> =>
    invoke("plan_import", { sourcePath }),
  importSkill: (
    sourcePath: string,
    resolution: ImportResolution,
    dryRun = false,
  ): Promise<ImportOutcome> =>
    invoke("import_skill", { sourcePath, resolution, dryRun }),
  planSync: (toolId: string): Promise<SyncPlan> => invoke("plan_sync", { toolId }),
  syncTool: (toolId: string, dryRun = false): Promise<SyncRunReport> =>
    invoke("sync_tool", { toolId, dryRun }),
  syncAll: (dryRun = false): Promise<SyncRunReport[]> => invoke("sync_all", { dryRun }),
  setSkillToolEnabled: (
    skillId: string,
    toolId: string,
    enabled: boolean,
    dryRun = false,
  ): Promise<SyncRunReport> =>
    invoke("set_skill_tool_enabled", { skillId, toolId, enabled, dryRun }),
  listConflicts: (): Promise<ConflictReport[]> => invoke("list_conflicts"),
  diffSkill: (skillId: string, toolId: string): Promise<DiffEntry[]> =>
    invoke("diff_skill", { skillId, toolId }),
  resolveConflict: (
    skillId: string,
    toolId: string,
    resolution: Resolution,
    dryRun = false,
  ): Promise<ResolutionReport> =>
    invoke("resolve_conflict", { skillId, toolId, resolution, dryRun }),
  setConflictIgnored: (skillId: string, toolId: string, ignored: boolean): Promise<Config> =>
    invoke("set_conflict_ignored", { skillId, toolId, ignored }),
  gitStatus: (): Promise<GitStatus> => invoke("git_status"),
  gitPull: (): Promise<string> => invoke("git_pull"),
  gitCommit: (message: string): Promise<string> => invoke("git_commit", { message }),
  gitPush: (): Promise<string> => invoke("git_push"),
  firstImportPlan: (): Promise<FirstImportPlan> => invoke("first_import_plan"),
  readSkillFile: (path: string): Promise<string> => invoke("read_skill_file", { path }),
  openInExplorer: (path: string): Promise<void> => invoke("open_in_explorer", { path }),
  applyFirstImport: (plan: FirstImportPlan, dryRun = false): Promise<FirstImportReport> =>
    invoke("apply_first_import", { plan, dryRun }),
};

export type Api = typeof api;
