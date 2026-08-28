import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { SkillsPage } from "./SkillsPage";
import type { SkillOverview } from "@/types/domain";

vi.mock("@/lib/api", () => ({
  api: {
    getConfig: vi.fn(),
    saveConfig: vi.fn(),
    scanOverview: vi.fn(),
    runDoctor: vi.fn(),
  },
}));

function makeOverview(): SkillOverview {
  return {
    canonicalRoot: "/home/tester/.agents/skills",
    canonicalRootDisplay: "~/.agents/skills",
    canonicalRootExists: true,
    tools: [
      {
        id: "claude",
        displayName: "Claude Code",
        detection: { installed: true, evidence: "config dir" },
        enabled: true,
        locations: [],
        symlinkSupport: "preferred",
        reloadGuidance: { summary: "auto", detail: "auto" },
        skillCount: 2,
        managedCount: 1,
      },
      {
        id: "codex",
        displayName: "Codex",
        detection: { installed: true, evidence: "config dir" },
        enabled: true,
        locations: [],
        symlinkSupport: "supported",
        reloadGuidance: { summary: "auto", detail: "auto" },
        skillCount: 1,
        managedCount: 1,
      },
    ],
    rows: [
      {
        key: "git-commit",
        name: "git-commit",
        description: "Creates good commits.",
        canonical: {
          path: "/home/tester/.agents/skills/git-commit",
          displayPath: "~/.agents/skills/git-commit",
          fingerprint: "abc",
          validation: [],
        },
        installations: [
          {
            toolId: "claude",
            toolDisplayName: "Claude Code",
            path: "/home/tester/.claude/skills/git-commit",
            displayPath: "~/.claude/skills/git-commit",
            state: "synced",
            managedness: { kind: "managedSymlink", canonicalPath: "/home/tester/.agents/skills/git-commit" },
            fingerprint: "abc",
            validation: [],
          },
        ],
        status: "synced",
      },
      {
        key: "legacy-tool",
        name: "legacy-tool",
        description: "An old unmanaged skill.",
        installations: [
          {
            toolId: "codex",
            toolDisplayName: "Codex",
            path: "/home/tester/.codex/skills/legacy-tool",
            displayPath: "~/.codex/skills/legacy-tool",
            state: "unmanaged",
            managedness: { kind: "unmanaged" },
            fingerprint: "def",
            validation: [
              { severity: "warning", message: "no description", code: "missing_description" },
            ],
          },
        ],
        status: "unmanaged",
      },
    ],
  };
}

describe("SkillsPage", () => {
  it("renders all discovered skills with status and source", () => {
    render(
      <SkillsPage overview={makeOverview()} loading={false} onRefresh={() => {}} />,
    );
    expect(screen.getByText("git-commit")).toBeInTheDocument();
    expect(screen.getByText("legacy-tool")).toBeInTheDocument();
    expect(screen.getByText("~/.agents/skills/git-commit")).toBeInTheDocument();
    expect(screen.getByText("2 skills · canonical store ~/.agents/skills")).toBeInTheDocument();
  });

  it("filters by search query", async () => {
    const user = userEvent.setup();
    render(
      <SkillsPage overview={makeOverview()} loading={false} onRefresh={() => {}} />,
    );
    await user.type(screen.getByLabelText("Search skills"), "legacy");
    expect(screen.getByText("legacy-tool")).toBeInTheDocument();
    expect(screen.queryByText("git-commit")).not.toBeInTheDocument();
  });

  it("filters by status chip", async () => {
    const user = userEvent.setup();
    render(
      <SkillsPage overview={makeOverview()} loading={false} onRefresh={() => {}} />,
    );
    await user.click(screen.getByRole("button", { name: "Synced" }));
    expect(screen.getByText("git-commit")).toBeInTheDocument();
    expect(screen.queryByText("legacy-tool")).not.toBeInTheDocument();
  });

  it("filters by tool chip", async () => {
    const user = userEvent.setup();
    render(
      <SkillsPage overview={makeOverview()} loading={false} onRefresh={() => {}} />,
    );
    await user.click(screen.getByRole("button", { name: "Codex" }));
    expect(screen.getByText("legacy-tool")).toBeInTheDocument();
    expect(screen.queryByText("git-commit")).not.toBeInTheDocument();
  });

  it("shows validation warnings on cards", () => {
    render(
      <SkillsPage overview={makeOverview()} loading={false} onRefresh={() => {}} />,
    );
    const cards = screen.getAllByTestId("skill-card");
    const legacyCard = cards.find((el) => within(el).queryByText("legacy-tool") !== null);
    expect(legacyCard).toBeDefined();
    expect(within(legacyCard!).getByText(/no description/)).toBeInTheDocument();
  });
});
