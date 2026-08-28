import { describe, expect, it, vi } from "vitest";
import { api } from "./api";
import type { SkillOverview } from "@/types/domain";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

import { invoke } from "@tauri-apps/api/core";
const invokeMock = vi.mocked(invoke);

const sample: SkillOverview = {
  canonicalRoot: "/home/tester/.agents/skills",
  canonicalRootDisplay: "~/.agents/skills",
  canonicalRootExists: true,
  tools: [],
  rows: [],
};

describe("typed native api layer", () => {
  it("calls the scan_overview command without arguments", async () => {
    invokeMock.mockResolvedValueOnce(sample);
    const result = await api.scanOverview();
    expect(invokeMock).toHaveBeenCalledWith("scan_overview");
    expect(result).toEqual(sample);
  });

  it("passes the config positionally to save_config", async () => {
    const config = {
      canonicalSkillRoot: "~/.agents/skills",
      syncMethod: "auto" as const,
      tools: {},
      repositories: [],
      autoSync: false,
    };
    invokeMock.mockResolvedValueOnce(config);
    await api.saveConfig(config);
    expect(invokeMock).toHaveBeenCalledWith("save_config", { config });
  });

  it("maps runDoctor to the run_doctor command", async () => {
    invokeMock.mockResolvedValueOnce({ os: "macos", skillsyncHome: "/x", checks: [] });
    await api.runDoctor();
    expect(invokeMock).toHaveBeenCalledWith("run_doctor");
  });

  it("plans imports read-only and passes the source path", async () => {
    const plan = {
      source: "/h/.claude/skills/a",
      canonicalRoot: "/h/.agents/skills",
      skillId: "a",
      action: { kind: "create", target: "/h/.agents/skills/a" },
      fingerprint: "ff",
      notes: [],
    };
    invokeMock.mockResolvedValueOnce(plan);
    const result = await api.planImport("/h/.claude/skills/a");
    expect(invokeMock).toHaveBeenCalledWith("plan_import", {
      sourcePath: "/h/.claude/skills/a",
    });
    expect(result).toEqual(plan);
  });

  it("executes imports with an explicit resolution", async () => {
    invokeMock.mockResolvedValueOnce({
      actionTaken: { kind: "create", target: "/h/.agents/skills/a" },
      target: "/h/.agents/skills/a",
      dryRun: false,
    });
    await api.importSkill("/h/.claude/skills/a", "keepBoth", false);
    expect(invokeMock).toHaveBeenCalledWith("import_skill", {
      sourcePath: "/h/.claude/skills/a",
      resolution: "keepBoth",
      dryRun: false,
    });
  });

  it("adopts the canonical root", async () => {
    invokeMock.mockResolvedValueOnce({ canonicalRoot: "/h/.agents/skills" });
    await api.adoptCanonicalRoot();
    expect(invokeMock).toHaveBeenCalledWith("adopt_canonical_root");
  });
});
