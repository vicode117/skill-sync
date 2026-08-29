import { RefreshButton } from "@/components/feedback";
import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
import { SyncControl } from "./SyncControl";
import { useI18n } from "@/lib/i18n";
import type { SkillOverview, ToolInfo } from "@/types/domain";

export function ToolsPage({
  overview,
  loading,
  onRefresh,
  onToggleTool,
  busyTool,
}: {
  overview: SkillOverview | null;
  loading: boolean;
  onRefresh: () => void;
  onToggleTool: (toolId: string, enabled: boolean) => Promise<void>;
  busyTool: string | null;
}) {
  const { t } = useI18n();
  const tools = overview?.tools ?? [];
  return (
    <section aria-label="Tools" className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold">{t("tools.title")}</h1>
          <p className="text-sm text-muted-foreground">{t("tools.subtitle")}</p>
        </div>
        <RefreshButton onClick={onRefresh} loading={loading} />
      </div>

      <ul className="space-y-3">
        {tools.map((tool) => (
          <ToolCard
            key={tool.id}
            tool={tool}
            onToggle={onToggleTool}
            busy={busyTool === tool.id}
            onSynced={onRefresh}
          />
        ))}
      </ul>
    </section>
  );
}

function ToolCard({
  tool,
  onToggle,
  busy,
  onSynced,
}: {
  tool: ToolInfo;
  onToggle: (toolId: string, enabled: boolean) => Promise<void>;
  busy: boolean;
  onSynced: () => void;
}) {
  const { t } = useI18n();
  return (
    <Card className="p-4" data-testid="tool-card">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <h2 className="font-semibold">{tool.displayName}</h2>
          <Badge variant={tool.detection.installed ? "success" : "muted"}>
            {tool.detection.installed ? t("tools.detected") : t("tools.notDetected")}
          </Badge>
          <Badge variant="outline">{tool.id}</Badge>
        </div>
        <label className="flex items-center gap-2 text-sm text-muted-foreground">
          {t("tools.integration")}
          <Switch
            checked={tool.enabled}
            disabled={busy}
            onCheckedChange={(checked) => void onToggle(tool.id, checked)}
            aria-label={`${tool.displayName} integration`}
          />
        </label>
      </div>

      <p className="mt-1.5 text-sm text-muted-foreground">{tool.detection.evidence}</p>

      <div className="mt-3 space-y-1.5">
        {tool.locations.map((loc) => (
          <div key={loc.path} className="flex flex-wrap items-baseline gap-x-2 text-sm">
            <span className="font-mono text-xs">{loc.displayPath}</span>
            {loc.nativeCanonical ? <Badge variant="success">{t("tools.canonicalStore")}</Badge> : null}
            {loc.overridden ? <Badge variant="secondary">{t("tools.override")}</Badge> : null}
            <span className="text-xs text-muted-foreground">
              {loc.exists
                ? `${loc.skillCount} skills · ${loc.managedCount} managed`
                : t("tools.dirMissing")}
            </span>
          </div>
        ))}
      </div>

      <div className="mt-3 flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
        <span>
          {t("tools.discovered")}{" "}
          <strong className="text-foreground">
            {tool.skillCount}
          </strong>
          {t("tools.managedSuffix", { managed: tool.managedCount })}
        </span>
        <span>
          {t("tools.symlinks")}{" "}
          <strong className="text-foreground">
            {t(`symlink.${tool.symlinkSupport}`)}
          </strong>
        </span>
        <span>
          {t("tools.reload")}{" "}
          <strong className="text-foreground">{tool.reloadGuidance.summary}</strong>
        </span>
      </div>

      {tool.enabled ? (
        <SyncControl toolId={tool.id} toolName={tool.displayName} onSynced={onSynced} />
      ) : null}
    </Card>
  );
}
