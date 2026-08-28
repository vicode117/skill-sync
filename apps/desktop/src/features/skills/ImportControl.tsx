import { useState } from "react";
import { Download, Undo2 } from "lucide-react";
import { api, normalizeError } from "@/lib/api";
import { Button } from "@/components/ui/button";
import type { ImportPlan, ImportResolution, SkillSyncError } from "@/types/domain";

/**
 * Import control for an unmanaged skill: preview the plan first (§56),
 * then apply only on explicit confirmation. Conflicts require an explicit
 * resolution — nothing is ever overwritten automatically (§18, §30).
 */
export function ImportControl({
  sourcePath,
  onImported,
}: {
  sourcePath: string;
  onImported: () => void;
}) {
  const [plan, setPlan] = useState<ImportPlan | null>(null);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState<string | null>(null);
  const [error, setError] = useState<SkillSyncError | null>(null);

  const preview = async () => {
    setBusy(true);
    setError(null);
    try {
      setPlan(await api.planImport(sourcePath));
    } catch (e) {
      setError(normalizeError(e));
    } finally {
      setBusy(false);
    }
  };

  const apply = async (resolution: ImportResolution) => {
    setBusy(true);
    setError(null);
    try {
      const outcome = await api.importSkill(sourcePath, resolution);
      setDone(
        outcome.actionTaken.kind === "replace"
          ? `Imported (previous copy backed up to ${outcome.actionTaken.backupDir})`
          : `Imported to ${outcome.target}`,
      );
      setPlan(null);
      onImported();
    } catch (e) {
      setError(normalizeError(e));
    } finally {
      setBusy(false);
    }
  };

  if (done) {
    return (
      <div className="mt-3 flex items-center gap-2 text-xs text-success" role="status">
        <Download className="size-3" aria-hidden />
        {done}
      </div>
    );
  }

  return (
    <div className="mt-3 border-t pt-3" data-testid="import-control">
      {!plan ? (
        <Button variant="outline" size="sm" onClick={() => void preview()} disabled={busy}>
          <Download className="size-3.5" aria-hidden />
          {busy ? "Planning…" : "Import to canonical store"}
        </Button>
      ) : (
        <div className="space-y-2 text-xs">
          <p className="font-medium">
            Import plan:{" "}
            <span className="font-mono">{describeAction(plan)}</span>
          </p>
          {plan.notes.map((note, i) => (
            <p key={i} className="text-muted-foreground">
              {note}
            </p>
          ))}
          {plan.action.kind === "conflict" ? (
            <div className="flex flex-wrap gap-2">
              <Button size="sm" variant="secondary" disabled={busy} onClick={() => void apply("keepBoth")}>
                Keep both
              </Button>
              <Button size="sm" variant="destructive" disabled={busy} onClick={() => void apply("replace")}>
                Replace (backup first)
              </Button>
              <Button size="sm" variant="ghost" disabled={busy} onClick={() => setPlan(null)}>
                Cancel
              </Button>
            </div>
          ) : plan.action.kind === "alreadyPresent" ? (
            <Button size="sm" variant="ghost" onClick={() => setPlan(null)}>
              Close
            </Button>
          ) : (
            <div className="flex flex-wrap gap-2">
              <Button size="sm" disabled={busy} onClick={() => void apply("skip")}>
                Confirm import
              </Button>
              <Button size="sm" variant="ghost" disabled={busy} onClick={() => setPlan(null)}>
                <Undo2 className="size-3" aria-hidden />
                Cancel
              </Button>
            </div>
          )}
        </div>
      )}
      {error ? (
        <p role="alert" className="mt-2 text-xs text-destructive">
          {error.code}: {error.message}
        </p>
      ) : null}
    </div>
  );
}

function describeAction(plan: ImportPlan): string {
  switch (plan.action.kind) {
    case "create":
      return `create ${plan.action.target}`;
    case "alreadyPresent":
      return "no change (identical copy already present)";
    case "keepBoth":
      return `import as ${plan.action.target}`;
    case "replace":
      return `replace ${plan.action.target} (backup first)`;
    case "conflict":
      return `conflict with ${plan.action.target}`;
  }
}
