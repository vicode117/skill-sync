import { RefreshButton } from "@/components/feedback";
import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import { Switch } from "@/components/ui/switch";
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
  const tools = overview?.tools ?? [];
  return (
    <section aria-label="Tools" className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold">Tools</h1>
          <p className="text-sm text-muted-foreground">
            Detected AI coding tools and their skill locations.
          </p>
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
}: {
  tool: ToolInfo;
  onToggle: (toolId: string, enabled: boolean) => Promise<void>;
  busy: boolean;
}) {
  return (
    <Card className="p-4" data-testid="tool-card">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <h2 className="font-semibold">{tool.displayName}</h2>
          <Badge variant={tool.detection.installed ? "success" : "muted"}>
            {tool.detection.installed ? "Detected" : "Not detected"}
          </Badge>
          <Badge variant="outline">{tool.id}</Badge>
        </div>
        <label className="flex items-center gap-2 text-sm text-muted-foreground">
          Integration
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
            {loc.nativeCanonical ? <Badge variant="success">canonical store</Badge> : null}
            {loc.overridden ? <Badge variant="secondary">override</Badge> : null}
            <span className="text-xs text-muted-foreground">
              {loc.exists
                ? `${loc.skillCount} skills · ${loc.managedCount} managed`
                : "directory missing"}
            </span>
          </div>
        ))}
      </div>

      <div className="mt-3 flex flex-wrap gap-x-4 gap-y-1 text-xs text-muted-foreground">
        <span>
          Skills discovered: <strong className="text-foreground">{tool.skillCount}</strong> (
          {tool.managedCount} managed)
        </span>
        <span>
          Symlinks: <strong className="text-foreground">{tool.symlinkSupport}</strong>
        </span>
        <span>
          Reload: <strong className="text-foreground">{tool.reloadGuidance.summary}</strong>
        </span>
      </div>
    </Card>
  );
}
