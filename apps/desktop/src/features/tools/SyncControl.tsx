import { useState } from "react";
import { ArrowDownUp, XCircle } from "lucide-react";
import { api, normalizeError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { Button } from "@/components/ui/button";
import type { PlanEntry, SyncPlan, SyncRunReport, SkillSyncError } from "@/types/domain";

/**
 * Per-tool sync control: preview the plan (§56/§58), apply only on
 * confirmation. Only managed targets are ever touched.
 */
export function SyncControl({
  toolId,
  toolName,
  onSynced,
}: {
  toolId: string;
  toolName: string;
  onSynced: () => void;
}) {
  const { t } = useI18n();
  const [plan, setPlan] = useState<SyncPlan | null>(null);
  const [report, setReport] = useState<SyncRunReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<SkillSyncError | null>(null);

  const preview = async () => {
    setBusy(true);
    setError(null);
    setReport(null);
    try {
      setPlan(await api.planSync(toolId));
    } catch (e) {
      setError(normalizeError(e));
    } finally {
      setBusy(false);
    }
  };

  const apply = async (dryRun: boolean) => {
    setBusy(true);
    setError(null);
    try {
      const result = await api.syncTool(toolId, dryRun);
      setReport(result);
      setPlan(null);
      if (!dryRun) onSynced();
    } catch (e) {
      setError(normalizeError(e));
    } finally {
      setBusy(false);
    }
  };

  if (report) {
    return (
      <div className="mt-3 border-t pt-3 text-xs" data-testid="sync-report">
        <p className={report.failed.length === 0 ? "text-success" : "text-warning"} role="status">
          {report.dryRun ? t("tools.dryRunPrefix") : ""}
          {t("tools.syncReport", { tool: toolName, summary: reportSummary(report) })}
        </p>
        {report.failed.map((f) => (
          <p key={f.skillId} className="mt-1 flex items-start gap-1.5 text-destructive">
            <XCircle className="mt-0.5 size-3 shrink-0" aria-hidden />
            {f.skillId}: {f.error ?? "unknown error"}
          </p>
        ))}
        <Button size="sm" variant="ghost" className="mt-2" onClick={() => setReport(null)}>
          Close
        </Button>
      </div>
    );
  }

  if (!plan) {
    return (
      <div className="mt-3 border-t pt-3">
        <Button variant="outline" size="sm" onClick={() => void preview()} disabled={busy}>
          <ArrowDownUp className="size-3.5" aria-hidden />
          {busy ? t("skills.planning") : t("tools.syncFromStore")}
        </Button>
        {error ? (
          <p role="alert" className="mt-2 text-xs text-destructive">
            {error.code}: {error.message}
          </p>
        ) : null}
      </div>
    );
  }

  const mutations = plan.entries.filter((e) => isMutation(e));

  return (
    <div className="mt-3 space-y-2 border-t pt-3 text-xs" data-testid="sync-plan">
      <p className="font-medium">
        {t("tools.syncPlan", {
          method: plan.method,
          dir: plan.targetDir ?? "<no location>",
        })}
      </p>
      {plan.entries.length === 0 ? (
        <p className="text-muted-foreground">
          {t("tools.storeEmpty")}
        </p>
      ) : (
        <ul className="max-h-48 space-y-1 overflow-y-auto">
          {plan.entries.map((entry) => (
            <li key={entry.skillId} className="flex items-baseline gap-2">
              <span className="w-20 shrink-0 font-mono uppercase text-muted-foreground">
                {entry.action.kind}
              </span>
              <span className="font-medium">{entry.skillName}</span>
              <span className="truncate text-muted-foreground">{entryDetail(entry)}</span>
            </li>
          ))}
        </ul>
      )}
      {plan.entries.flatMap((e) => e.notes).map((note, i) => (
        <p key={i} className="text-muted-foreground">
          {note}
        </p>
      ))}
      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          disabled={busy || mutations.length === 0}
          onClick={() => void apply(false)}
        >
          {busy ? t("settings.running") : t("tools.apply", { count: mutations.length })}
        </Button>
        <Button size="sm" variant="secondary" disabled={busy} onClick={() => void apply(true)}>
          {t("tools.dryRun")}
        </Button>
        <Button size="sm" variant="ghost" disabled={busy} onClick={() => setPlan(null)}>
          {t("common.cancel")}
        </Button>
      </div>
      {error ? (
        <p role="alert" className="text-destructive">
          {error.code}: {error.message}
        </p>
      ) : null}
    </div>
  );
}

function isMutation(entry: PlanEntry): boolean {
  return ["createLink", "createCopy", "updateCopy", "repairLink"].includes(entry.action.kind);
}

function entryDetail(entry: PlanEntry): string {
  switch (entry.action.kind) {
    case "createLink":
    case "repairLink":
      return entry.displayTarget;
    case "createCopy":
    case "updateCopy":
      return entry.displayTarget;
    case "skip":
      return entry.action.reason;
    default:
      return "";
  }
}

function reportSummary(report: SyncRunReport): string {
  return `${report.succeeded.length} succeeded, ${report.failed.length} failed${report.dryRun ? " (dry run)" : ""}`;
}
