import { invoke } from "@tauri-apps/api/core";
import type {
  Config,
  DoctorReport,
  ImportOutcome,
  ImportPlan,
  ImportResolution,
  SkillOverview,
  SkillSyncError,
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
};

export type Api = typeof api;
