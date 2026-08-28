import { useCallback, useEffect, useState } from "react";
import { GitCompareArrows, TriangleAlert } from "lucide-react";
import { api, normalizeError } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import type {
  ConflictReport,
  DiffEntry,
  Resolution,
  SkillSyncError,
} from "@/types/domain";

/**
 * Conflicts UX (design doc §54): highly visible but not alarming. Every
 * resolution is an explicit choice; nothing is ever resolved
 * automatically and everything replaced is backed up first (§18, §30).
 */
export function ConflictsSection({ onResolved }: { onResolved: () => void }) {
  const [conflicts, setConflicts] = useState<ConflictReport[]>([]);
  const [error, setError] = useState<SkillSyncError | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [expanded, setExpanded] = useState<string | null>(null);
  const [diff, setDiff] = useState<DiffEntry[] | null>(null);

  const reload = useCallback(async () => {
    try {
      const all = await api.listConflicts();
      setConflicts(all.filter((c) => !c.ignored));
      setError(null);
    } catch (e) {
      setError(normalizeError(e));
    }
  }, []);

  useEffect(() => {
    void reload();
  }, [reload]);

  const act = async (
    conflict: ConflictReport,
    action: () => Promise<unknown>,
  ) => {
    const key = `${conflict.skillId}:${conflict.toolId}`;
    setBusy(key);
    setError(null);
    try {
      await action();
      setExpanded(null);
      setDiff(null);
      await reload();
      onResolved();
    } catch (e) {
      setError(normalizeError(e));
    } finally {
      setBusy(null);
    }
  };

  const resolve = (conflict: ConflictReport, resolution: Resolution) =>
    act(conflict, () => api.resolveConflict(conflict.skillId, conflict.toolId, resolution));

  const compare = async (conflict: ConflictReport) => {
    const key = `${conflict.skillId}:${conflict.toolId}`;
    if (expanded === key) {
      setExpanded(null);
      setDiff(null);
      return;
    }
    setBusy(key);
    try {
      setDiff(await api.diffSkill(conflict.skillId, conflict.toolId));
      setExpanded(key);
    } catch (e) {
      setError(normalizeError(e));
    } finally {
      setBusy(null);
    }
  };

  if (conflicts.length === 0 && !error) {
    return null;
  }

  return (
    <section
      aria-label="Conflicts"
      className="space-y-3 rounded-xl border border-warning/40 bg-warning/5 p-4"
    >
      <div className="flex items-center gap-2">
        <TriangleAlert className="size-4 text-warning" aria-hidden />
        <h2 className="text-sm font-semibold">
          Conflicts need your decision
        </h2>
        <Badge variant="warning">{conflicts.length}</Badge>
      </div>
      <p className="text-xs text-muted-foreground">
        A canonical skill and an unmanaged copy with the same name hold
        different content. Nothing is changed until you choose; whatever is
        replaced is backed up first.
      </p>

      {error ? (
        <p role="alert" className="text-xs text-destructive">
          {error.code}: {error.message}
        </p>
      ) : null}

      <ul className="space-y-3">
        {conflicts.map((conflict) => {
          const key = `${conflict.skillId}:${conflict.toolId}`;
          return (
            <li key={key} className="rounded-lg border bg-card p-3" data-testid="conflict-card">
              <div className="flex flex-wrap items-baseline justify-between gap-2">
                <div>
                  <span className="font-medium">{conflict.skillName}</span>
                  <span className="text-muted-foreground"> · {conflict.toolDisplayName}</span>
                </div>
                <div className="flex flex-wrap gap-2">
                  <Button
                    size="sm"
                    variant="outline"
                    disabled={busy === key}
                    onClick={() => void compare(conflict)}
                  >
                    <GitCompareArrows className="size-3.5" aria-hidden />
                    {expanded === key ? "Hide diff" : "Compare"}
                  </Button>
                  <Button
                    size="sm"
                    disabled={busy === key}
                    onClick={() => void resolve(conflict, "useCanonical")}
                  >
                    Use canonical
                  </Button>
                  <Button
                    size="sm"
                    variant="secondary"
                    disabled={busy === key}
                    onClick={() => void resolve(conflict, "importTarget")}
                  >
                    Import {conflict.toolDisplayName} version
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={busy === key}
                    onClick={() => void resolve(conflict, "keepBoth")}
                  >
                    Keep both
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    disabled={busy === key}
                    onClick={() =>
                      void act(conflict, () =>
                        api.setConflictIgnored(conflict.skillId, conflict.toolId, true),
                      )
                    }
                  >
                    Ignore
                  </Button>
                </div>
              </div>
              <div className="mt-2 grid gap-1 text-xs text-muted-foreground sm:grid-cols-2">
                <p>
                  Canonical: <span className="font-mono">{conflict.canonicalDisplay}</span>
                </p>
                <p>
                  Target: <span className="font-mono">{conflict.targetDisplay}</span>
                </p>
              </div>
              {expanded === key && diff !== null ? (
                <div className="mt-3 max-h-64 overflow-y-auto rounded-md border p-2 font-mono text-[11px]" data-testid="conflict-diff">
                  {diff.length === 0 ? (
                    <p className="text-muted-foreground">No file-level differences.</p>
                  ) : (
                    diff.map((entry) => (
                      <div key={entry.relativePath} className="mb-2">
                        <p
                          className={
                            entry.kind === "added"
                              ? "text-success"
                              : entry.kind === "removed"
                                ? "text-destructive"
                                : "text-warning"
                          }
                        >
                          {entry.kind.toUpperCase()} {entry.relativePath}
                        </p>
                        {entry.textDiff
                          ? entry.textDiff.split("\n").map((line, i) => (
                              <p key={i} className="whitespace-pre-wrap pl-3">
                                {line}
                              </p>
                            ))
                          : null}
                      </div>
                    ))
                  )}
                </div>
              ) : null}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
