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
});
