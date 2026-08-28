import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { ImportControl } from "./ImportControl";

vi.mock("@/lib/api", () => ({
  api: {
    getConfig: vi.fn(),
    saveConfig: vi.fn(),
    scanOverview: vi.fn(),
    runDoctor: vi.fn(),
    adoptCanonicalRoot: vi.fn(),
    planImport: vi.fn(),
    importSkill: vi.fn(),
  },
  normalizeError: (e: unknown) =>
    typeof e === "object" && e && "message" in e
      ? (e as { code: string; message: string })
      : { code: "UNKNOWN", message: String(e) },
}));

import { api } from "@/lib/api";
const planImport = vi.mocked(api.planImport);
const importSkill = vi.mocked(api.importSkill);

beforeEach(() => {
  planImport.mockReset();
  importSkill.mockReset();
});

describe("ImportControl", () => {
  it("previews the plan before applying, then imports on confirmation", async () => {
    const user = userEvent.setup();
    const onImported = vi.fn();
    planImport.mockResolvedValueOnce({
      source: "/h/.claude/skills/alpha",
      canonicalRoot: "/h/.agents/skills",
      skillId: "alpha",
      action: { kind: "create", target: "/h/.agents/skills/alpha" },
      fingerprint: "aa",
      notes: [],
    });
    importSkill.mockResolvedValueOnce({
      actionTaken: { kind: "create", target: "/h/.agents/skills/alpha" },
      target: "/h/.agents/skills/alpha",
      dryRun: false,
    });

    render(<ImportControl sourcePath="/h/.claude/skills/alpha" onImported={onImported} />);

    // Nothing happens until the user asks for it.
    expect(planImport).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: /Import to canonical store/ }));
    expect(planImport).toHaveBeenCalledWith("/h/.claude/skills/alpha");
    expect(screen.getByText(/create \/h\/.agents\/skills\/alpha/)).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Confirm import" }));
    expect(importSkill).toHaveBeenCalledWith("/h/.claude/skills/alpha", "skip");
    expect(onImported).toHaveBeenCalled();
    expect(await screen.findByText(/Imported to/)).toBeInTheDocument();
  });

  it("offers explicit resolutions for conflicts instead of overwriting", async () => {
    const user = userEvent.setup();
    planImport.mockResolvedValueOnce({
      source: "/h/.claude/skills/alpha",
      canonicalRoot: "/h/.agents/skills",
      skillId: "alpha",
      action: { kind: "conflict", target: "/h/.agents/skills/alpha" },
      fingerprint: "bb",
      notes: ["differs"],
    });

    render(<ImportControl sourcePath="/h/.claude/skills/alpha" onImported={() => {}} />);
    await user.click(screen.getByRole("button", { name: /Import to canonical store/ }));

    expect(screen.getByText(/conflict with/)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Keep both" }));
    expect(importSkill).toHaveBeenCalledWith("/h/.claude/skills/alpha", "keepBoth");
  });

  it("surfaces native errors", async () => {
    const user = userEvent.setup();
    planImport.mockRejectedValueOnce({
      code: "INVALID_SKILL",
      message: "not a skill",
    });
    render(<ImportControl sourcePath="/nowhere" onImported={() => {}} />);
    await user.click(screen.getByRole("button", { name: /Import to canonical store/ }));
    expect(await screen.findByText(/INVALID_SKILL: not a skill/)).toBeInTheDocument();
  });
});
