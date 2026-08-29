import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { ToolsPage } from "./ToolsPage";
import type { SkillOverview } from "@/types/domain";

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
  },
}));

function makeOverview(): SkillOverview {
  return {
    canonicalRoot: "/home/t/.agents/skills",
    canonicalRootDisplay: "~/.agents/skills",
    canonicalRootExists: true,
    tools: [
      {
        id: "claude",
        displayName: "Claude Code",
        detection: { installed: true, evidence: "found ~/.claude" },
        enabled: true,
        locations: [
          {
            path: "/home/t/.claude/skills",
            displayPath: "~/.claude/skills",
            kind: "standard",
            overridden: false,
            exists: true,
            nativeCanonical: false,
            skillCount: 12,
            managedCount: 5,
            unmanagedCount: 7,
          },
        ],
        symlinkSupport: "preferred",
        reloadGuidance: { summary: "Changes are detected automatically", detail: "…" },
        skillCount: 12,
        managedCount: 5,
      },
    ],
    rows: [],
  };
}

describe("ToolsPage", () => {
  it("shows detection state, locations and capabilities", () => {
    render(
      <ToolsPage
        overview={makeOverview()}
        loading={false}
        onRefresh={() => {}}
        onToggleTool={vi.fn()}
        busyTool={null}
      />,
    );
    expect(screen.getByText("Claude Code")).toBeInTheDocument();
    expect(screen.getByText("Detected")).toBeInTheDocument();
    expect(screen.getByText("~/.claude/skills")).toBeInTheDocument();
    expect(screen.getByText(/12 skills · 5 managed/)).toBeInTheDocument();
    expect(screen.getByText(/preferred/)).toBeInTheDocument();
  });

  it("toggles integration through the callback", async () => {
    const user = userEvent.setup();
    const onToggleTool = vi.fn().mockResolvedValue(undefined);
    render(
      <ToolsPage
        overview={makeOverview()}
        loading={false}
        onRefresh={() => {}}
        onToggleTool={onToggleTool}
        busyTool={null}
      />,
    );
    await user.click(screen.getByLabelText("Claude Code integration"));
    expect(onToggleTool).toHaveBeenCalledWith("claude", false);
  });

  it("previews the sync plan and applies only on confirmation", async () => {
    const user = userEvent.setup();
    const { api } = await import("@/lib/api");
    const onRefresh = vi.fn();
    vi.mocked(api.planSync).mockResolvedValueOnce({
      toolId: "claude",
      toolDisplayName: "Claude Code",
      method: "symlink",
      canonicalRoot: "/h/.agents/skills",
      canonicalRootDisplay: "~/.agents/skills",
      targetDir: "/h/.claude/skills",
      entries: [
        {
          skillId: "tdd",
          skillName: "tdd",
          action: { kind: "createLink", target: "/h/.claude/skills/tdd", source: "/h/.agents/skills/tdd" },
          displayTarget: "~/.claude/skills/tdd",
          notes: [],
        },
        {
          skillId: "legacy",
          skillName: "legacy",
          action: { kind: "skip", target: "/h/.claude/skills/legacy", reason: "unmanaged conflict" },
          displayTarget: "~/.claude/skills/legacy",
          notes: [],
        },
      ],
    });
    vi.mocked(api.syncTool).mockResolvedValueOnce({
      toolId: "claude",
      method: "symlink",
      dryRun: false,
      succeeded: [
        { skillId: "tdd", actionKind: "createLink", ok: true },
      ],
      failed: [],
    });

    render(
      <ToolsPage
        overview={makeOverview()}
        loading={false}
        onRefresh={onRefresh}
        onToggleTool={vi.fn()}
        busyTool={null}
      />,
    );
    await user.click(screen.getByRole("button", { name: /Sync from canonical store/ }));
    expect(api.planSync).toHaveBeenCalledWith("claude");
    expect(await screen.findByText(/symlink into/)).toBeInTheDocument();
    expect(screen.getByText(/unmanaged conflict/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /Apply \(1 changes?\)/ }));
    expect(api.syncTool).toHaveBeenCalledWith("claude", false);
    await screen.findByText(/1 succeeded, 0 failed/);
    expect(onRefresh).toHaveBeenCalled();
  });
});
