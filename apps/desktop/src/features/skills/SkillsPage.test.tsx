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
    adoptCanonicalRoot: vi.fn(),
    planImport: vi.fn(),
    importSkill: vi.fn(),
    planSync: vi.fn(),
    syncTool: vi.fn(),
    syncAll: vi.fn(),
    setSkillToolEnabled: vi.fn(),
    listConflicts: vi.fn().mockResolvedValue([]),
    diffSkill: vi.fn(),
    resolveConflict: vi.fn(),
    setConflictIgnored: vi.fn(),
    gitStatus: vi.fn(),
    gitPull: vi.fn(),
    gitCommit: vi.fn(),
    gitPush: vi.fn(),
    firstImportPlan: vi.fn().mockResolvedValue({
      canonicalRoot: "/h/.agents/skills",
      canonicalRootDisplay: "~/.agents/skills",
      counts: { unique: 0, exactDuplicates: 0, conflicts: 0, alreadyCanonical: 0 },
      imports: [],
      conflicts: [],
      notes: [],
    }),
    applyFirstImport: vi.fn(),
    readSkillFile: vi.fn(),
    openInExplorer: vi.fn(),
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
  const onToggleSkillTool = vi.fn().mockResolvedValue(undefined);

  function renderPage() {
    return render(
      <SkillsPage
        overview={makeOverview()}
        loading={false}
        onRefresh={() => {}}
        onToggleSkillTool={onToggleSkillTool}
        toggling={null}
      />,
    );
  }

  it("renders all discovered skills with status and source", () => {
    renderPage();
    expect(screen.getByText("git-commit")).toBeInTheDocument();
    expect(screen.getByText("legacy-tool")).toBeInTheDocument();
    expect(screen.getByText("~/.agents/skills/git-commit")).toBeInTheDocument();
    expect(
      screen.getByText("2 skill(s) · canonical store ~/.agents/skills"),
    ).toBeInTheDocument();
  });

  it("filters by search query", async () => {
    const user = userEvent.setup();
    renderPage();
    await user.type(screen.getByLabelText("Search skills"), "legacy");
    expect(screen.getByText("legacy-tool")).toBeInTheDocument();
    expect(screen.queryByText("git-commit")).not.toBeInTheDocument();
  });

  it("filters by status chip", async () => {
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByRole("button", { name: "Synced" }));
    expect(screen.getByText("git-commit")).toBeInTheDocument();
    expect(screen.queryByText("legacy-tool")).not.toBeInTheDocument();
  });

  it("filters by tool chip", async () => {
    const user = userEvent.setup();
    renderPage();
    await user.click(screen.getByRole("button", { name: "Codex" }));
    expect(screen.getByText("legacy-tool")).toBeInTheDocument();
    expect(screen.queryByText("git-commit")).not.toBeInTheDocument();
  });

  it("shows validation warnings on cards", () => {
    renderPage();
    const cards = screen.getAllByTestId("skill-card");
    const legacyCard = cards.find((el) => within(el).queryByText("legacy-tool") !== null);
    expect(legacyCard).toBeDefined();
    expect(within(legacyCard!).getByText(/no description/)).toBeInTheDocument();
  });

  it("toggles a Skill×Tool combination from the matrix chip", async () => {
    const user = userEvent.setup();
    renderPage();
    // git-commit is synced with Claude: clicking that card's chip disables it.
    const cards = screen.getAllByTestId("skill-card");
    const gitCard = cards.find((el) => within(el).queryByText("git-commit") !== null);
    expect(gitCard).toBeDefined();
    const claudeChip = within(gitCard!).getByRole("button", { name: /Claude/ });
    await user.click(claudeChip);
    expect(onToggleSkillTool).toHaveBeenCalledWith("git-commit", "claude", false);
  });

  it("shows the detail view with fingerprint and a read-only SKILL.md preview", async () => {
    const user = userEvent.setup();
    const { api } = await import("@/lib/api");
    vi.mocked(api.readSkillFile).mockResolvedValueOnce("---\nname: git-commit\n---\nbody");
    renderPage();
    const cards = screen.getAllByTestId("skill-card");
    const gitCard = cards.find((el) => within(el).queryByText("git-commit") !== null)!;
    await user.click(within(gitCard).getByRole("button", { name: "Details" }));
    // Fingerprint shown (from the fixture: canonical fingerprint "abc").
    expect(within(gitCard).getByText(/Fingerprint:/)).toBeInTheDocument();
    // Open-in-explorer buttons exist for canonical + installation.
    expect(
      within(gitCard).getByRole("button", { name: "Open canonical directory in file explorer" }),
    ).toBeInTheDocument();
    // Preview loads through the typed API with the canonical path.
    await user.click(within(gitCard).getByRole("button", { name: /Preview SKILL.md/ }));
    expect(api.readSkillFile).toHaveBeenCalledWith("/home/tester/.agents/skills/git-commit/SKILL.md");
    expect(await within(gitCard).findByTestId("skill-preview")).toHaveTextContent("git-commit");
  });
});
