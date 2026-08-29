import { useCallback, useEffect, useState } from "react";
import { PackageOpen } from "lucide-react";
import { api, normalizeError } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import type {
  FirstImportPlan,
  FirstImportReport,
  SkillSyncError,
} from "@/types/domain";

/**
 * First-run experience (design doc §7h, prompt §19/§56/§57): scan
 * findings, aggregated plan, apply only after explicit confirmation.
 * Nothing is imported automatically and tool directories are never
 * modified by an import.
 */
export function FirstImportBanner({
  onChanged,
}: {
  onChanged: () => void;
}) {
  const [plan, setPlan] = useState<FirstImportPlan | null>(null);
  const [report, setReport] = useState<FirstImportReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<SkillSyncError | null>(null);

  const reload = useCallback(async () => {
    try {
      const next = await api.firstImportPlan();
      setPlan(next);
      setError(null);
    } catch (e) {
      setError(normalizeError(e));
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const apply = async () => {
    if (!plan) return;
    setBusy(true);
    setError(null);
    try {
      setReport(await api.applyFirstImport(plan, false));
      await reload();
      onChanged();
    } catch (e) {
      setError(normalizeError(e));
    } finally {
      setBusy(false);
    }
  };

  if (error) {
    return (
      <p role="alert" className="text-xs text-destructive">
        {error.code}: {error.message}
      </p>
    );
  }
  if (report && report.imported.length > 0) {
    return (
      <section
        aria-label="First import"
        className="rounded-xl border bg-card p-4 text-sm"
        data-testid="first-import"
      >
        <p className="font-medium">Imported {report.imported.length} skill(s) into the canonical store.</p>
        <p className="mt-1 text-xs text-muted-foreground">
          Tool directories were not modified. Run a sync to install them into
          your tools as managed links or copies.
        </p>
        {report.skipped.length + report.failed.length > 0 ? (
          <ul className="mt-2 space-y-1 text-xs text-muted-foreground">
            {report.skipped.map((s) => (
              <li key={s.skillName}>skipped {s.skillName}: {s.reason}</li>
            ))}
            {report.failed.map((f) => (
              <li key={f.skillName} className="text-destructive">
                failed {f.skillName}: {f.error}
              </li>
            ))}
          </ul>
        ) : null}
      </section>
    );
  }
  if (!plan || (plan.imports.length === 0 && plan.conflicts.length === 0)) {
    return null;
  }

  return (
    <section
      aria-label="First import"
      className="space-y-3 rounded-xl border border-primary/30 bg-primary/5 p-4"
      data-testid="first-import"
    >
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <PackageOpen className="size-4 text-primary" aria-hidden />
          <h2 className="text-sm font-semibold">Adopt your existing skills</h2>
          <Badge variant="secondary">{plan.counts.unique} unique</Badge>
          {plan.counts.exactDuplicates > 0 ? (
            <Badge variant="muted">{plan.counts.exactDuplicates} duplicate files</Badge>
          ) : null}
          {plan.conflicts.length > 0 ? (
            <Badge variant="warning">{plan.conflicts.length} conflict(s)</Badge>
          ) : null}
        </div>
        <Button size="sm" onClick={() => void apply()} disabled={busy || plan.imports.length === 0}>
          {busy
            ? "Importing…"
            : `Import ${plan.imports.length} skill${plan.imports.length === 1 ? "" : "s"}`}
        </Button>
      </div>
      <p className="text-xs text-muted-foreground">
        Importing copies skills into {plan.canonicalRootDisplay} — your tool
        directories stay untouched. Conflicts (same name, different content)
        are never merged automatically.
      </p>
      <ul className="space-y-1 text-xs">
        {plan.imports.map((entry) => (
          <li key={entry.sourcePath} className="flex flex-wrap items-baseline gap-2">
            <span className="font-medium">{entry.skillName}</span>
            <span className="text-muted-foreground">
              from {entry.sourceToolId} · {entry.sourceDisplay}
            </span>
          </li>
        ))}
        {plan.conflicts.map((conflict) => (
          <li
            key={conflict.skillName}
            className="text-warning"
          >
            {conflict.skillName}: {conflict.occurrences.length} different versions —
            resolve manually after adopting the rest
          </li>
        ))}
      </ul>
    </section>
  );
}
