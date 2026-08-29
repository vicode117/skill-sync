import { renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { useOverview } from "./useOverview";

vi.mock("@/lib/api", () => ({
  api: {
    getConfig: vi.fn(),
    saveConfig: vi.fn(),
    scanOverview: vi.fn(),
    runDoctor: vi.fn(),
    adoptCanonicalRoot: vi.fn(),
    planImport: vi.fn(),
    importSkill: vi.fn(),
    planSync: vi.fn(),
    syncTool: vi.fn(),
    syncAll: vi.fn(),
    setSkillToolEnabled: vi.fn(),
    listConflicts: vi.fn(),
    diffSkill: vi.fn(),
    resolveConflict: vi.fn(),
    setConflictIgnored: vi.fn(),
    gitStatus: vi.fn(),
    gitPull: vi.fn(),
    gitCommit: vi.fn(),
    gitPush: vi.fn(),
    firstImportPlan: vi.fn(),
    applyFirstImport: vi.fn(),
    readSkillFile: vi.fn(),
    openInExplorer: vi.fn(),
  },
  normalizeError: (e: unknown) =>
    typeof e === "object" && e && "message" in e
      ? (e as { code: string; message: string })
      : { code: "UNKNOWN", message: String(e) },
}));

import { api } from "@/lib/api";

describe("useOverview", () => {
  it("loads the overview on mount (regression: UI stuck on Loading…)", async () => {
    vi.mocked(api.scanOverview).mockResolvedValueOnce({
      canonicalRoot: "/h/.agents/skills",
      canonicalRootDisplay: "~/.agents/skills",
      canonicalRootExists: true,
      tools: [],
      rows: [
        {
          key: "tdd",
          name: "tdd",
          installations: [],
          status: "synced",
        },
      ],
    });

    const { result } = renderHook(() => useOverview());
    expect(api.scanOverview).toHaveBeenCalledTimes(1);

    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.overview?.rows[0]?.name).toBe("tdd");
    expect(result.current.error).toBeNull();
  });

  it("surfaces native errors but always clears loading", async () => {
    vi.mocked(api.scanOverview).mockRejectedValueOnce({
      code: "PERMISSION_DENIED",
      message: "denied",
    });

    const { result } = renderHook(() => useOverview());
    await waitFor(() => expect(result.current.loading).toBe(false));
    expect(result.current.error?.code).toBe("PERMISSION_DENIED");
  });
});
