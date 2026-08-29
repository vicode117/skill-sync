import { useCallback, useEffect, useState } from "react";
import { FolderTree, Puzzle, Settings } from "lucide-react";
import { listen } from "@tauri-apps/api/event";
import { api } from "@/lib/api";
import { useOverview } from "@/hooks/useOverview";
import { SkillsPage } from "@/features/skills/SkillsPage";
import { ToolsPage } from "@/features/tools/ToolsPage";
import { SettingsPage } from "@/features/settings/SettingsPage";
import { ErrorBanner } from "@/components/feedback";
import { cn } from "@/lib/utils";
import { useI18n } from "@/lib/i18n";

type PageId = "skills" | "tools" | "settings";

const NAV: { id: PageId; key: "skills" | "tools" | "settings"; icon: typeof Puzzle }[] = [
  { id: "skills", key: "skills", icon: FolderTree },
  { id: "tools", key: "tools", icon: Puzzle },
  { id: "settings", key: "settings", icon: Settings },
];

export default function App() {
  const [page, setPage] = useState<PageId>("skills");
  const { overview, error, loading, refresh } = useOverview();
  const [busyTool, setBusyTool] = useState<string | null>(null);
  const [toggling, setToggling] = useState<string | null>(null);
  const { t } = useI18n();
  const [autoSyncNote, setAutoSyncNote] = useState<string | null>(null);

  // Automatic synchronization passes (§32) refresh the UI when they run.
  useEffect(() => {
    const unlisten = listen<string[]>("auto-sync-ran", (event) => {
      void refresh();
      setAutoSyncNote(t("app.autoSyncNote", { summary: event.payload.join(" · ") }));
      window.setTimeout(() => setAutoSyncNote(null), 8000);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const handleToggleTool = useCallback(
    async (toolId: string, enabled: boolean) => {
      setBusyTool(toolId);
      try {
        const config = await api.getConfig();
        await api.saveConfig({
          ...config,
          tools: {
            ...config.tools,
            [toolId]: { ...config.tools[toolId], enabled },
          },
        });
        await refresh();
      } finally {
        setBusyTool(null);
      }
    },
    [refresh],
  );

  const handleToggleSkillTool = useCallback(
    async (skillId: string, toolId: string, enabled: boolean) => {
      setToggling(`${skillId}:${toolId}`);
      try {
        await api.setSkillToolEnabled(skillId, toolId, enabled, false);
        await refresh();
      } finally {
        setToggling(null);
      }
    },
    [refresh],
  );

  return (
    <div className="flex min-h-screen">
      <nav className="flex w-52 shrink-0 flex-col border-r bg-card p-3" aria-label="Main">
        <div className="flex items-center gap-2 px-2 py-3">
          <div className="flex size-7 items-center justify-center rounded-md bg-primary text-sm font-bold text-primary-foreground">
            S
          </div>
          <span className="font-semibold">SkillSync</span>
        </div>
        <ul className="mt-2 space-y-1">
          {NAV.map(({ id, key, icon: Icon }) => {
            return (
              <li key={id}>
                <button
                  type="button"
                  onClick={() => setPage(id)}
                  aria-current={page === id ? "page" : undefined}
                  className={cn(
                    "flex w-full items-center gap-2 rounded-md px-2.5 py-2 text-sm font-medium",
                    page === id
                      ? "bg-secondary text-secondary-foreground"
                      : "text-muted-foreground hover:bg-accent hover:text-accent-foreground",
                  )}
                >
                  <Icon className="size-4" aria-hidden />
                  {t(`nav.${key}`)}
                </button>
              </li>
            );
          })}
        </ul>
        <p className="mt-auto px-2 pb-1 text-[11px] leading-4 text-muted-foreground">
          {t("app.tagline")}
        </p>
      </nav>

      <main className="flex-1 overflow-y-auto p-6">
        {autoSyncNote ? (
          <p
            role="status"
            className="mb-4 rounded-lg border bg-card p-3 text-xs text-muted-foreground"
          >
            {autoSyncNote}
          </p>
        ) : null}
        {error ? (
          <div className="mb-4">
            <ErrorBanner error={error} />
          </div>
        ) : null}
        {page === "skills" && (
          <SkillsPage
            overview={overview}
            loading={loading}
            onRefresh={() => void refresh()}
            onToggleSkillTool={handleToggleSkillTool}
            toggling={toggling}
          />
        )}
        {page === "tools" && (
          <ToolsPage
            overview={overview}
            loading={loading}
            onRefresh={() => void refresh()}
            onToggleTool={handleToggleTool}
            busyTool={busyTool}
          />
        )}
        {page === "settings" && <SettingsPage />}
      </main>
    </div>
  );
}
