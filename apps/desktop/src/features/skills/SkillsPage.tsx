import { useMemo, useState } from "react";
import { AlertCircle, XCircle } from "lucide-react";
import { RefreshButton } from "@/components/feedback";
import { Badge } from "@/components/ui/badge";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import type { SkillRow, SkillOverview, SyncState, ValidationIssue } from "@/types/domain";
import { STATUS_LABELS, STATUS_MARKS } from "@/types/domain";

const STATUS_VARIANTS: Record<SyncState, "success" | "warning" | "destructive" | "muted"> = {
  synced: "success",
  native: "success",
  modified: "warning",
  unmanaged: "warning",
  conflict: "destructive",
  unavailable: "destructive",
  notInstalled: "muted",
  disabled: "muted",
};

const FILTER_STATUSES = [
  "all",
  "synced",
  "native",
  "unmanaged",
  "notInstalled",
  "conflict",
  "unavailable",
] as const;
type StatusFilter = (typeof FILTER_STATUSES)[number];

const FILTER_LABELS: Record<StatusFilter, string> = {
  all: "All",
  synced: "Synced",
  native: "Native",
  unmanaged: "Unmanaged",
  notInstalled: "Not installed",
  conflict: "Conflict",
  unavailable: "Unavailable",
};

export function SkillsPage({
  overview,
  loading,
  onRefresh,
}: {
  overview: SkillOverview | null;
  loading: boolean;
  onRefresh: () => void;
}) {
  const [query, setQuery] = useState("");
  const [status, setStatus] = useState<StatusFilter>("all");
  const [toolFilter, setToolFilter] = useState<string>("all");

  const rows = useMemo(() => overview?.rows ?? [], [overview]);
  const filtered = useMemo(() => {
    const q = query.trim().toLowerCase();
    return rows.filter((row) => {
      if (status !== "all" && row.status !== status) return false;
      if (
        toolFilter !== "all" &&
        !row.installations.some(
          (i) => i.toolId === toolFilter && i.state !== "notInstalled",
        )
      ) {
        return false;
      }
      if (!q) return true;
      return (
        row.name.toLowerCase().includes(q) ||
        (row.description ?? "").toLowerCase().includes(q)
      );
    });
  }, [rows, query, status, toolFilter]);

  return (
    <section aria-label="Skills" className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h1 className="text-xl font-semibold">Skills</h1>
          <p className="text-sm text-muted-foreground">
            {overview
              ? `${rows.length} skill${rows.length === 1 ? "" : "s"} · canonical store ${overview.canonicalRootDisplay}${overview.canonicalRootExists ? "" : " (not created yet)"}`
              : "Loading…"}
          </p>
        </div>
        <RefreshButton onClick={onRefresh} loading={loading} />
      </div>

      <div className="flex flex-wrap items-center gap-2">
        <Input
          type="search"
          placeholder="Search skills…"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          className="max-w-xs"
          aria-label="Search skills"
        />
        <div className="flex flex-wrap gap-1" role="group" aria-label="Status filter">
          {FILTER_STATUSES.map((s) => (
            <button
              key={s}
              type="button"
              onClick={() => setStatus(s)}
              aria-pressed={status === s}
              className={`rounded-md px-2.5 py-1 text-xs font-medium ${
                status === s
                  ? "bg-primary text-primary-foreground"
                  : "bg-secondary text-secondary-foreground hover:bg-accent"
              }`}
            >
              {FILTER_LABELS[s]}
            </button>
          ))}
        </div>
        <div className="flex flex-wrap gap-1" role="group" aria-label="Tool filter">
          <button
            type="button"
            onClick={() => setToolFilter("all")}
            aria-pressed={toolFilter === "all"}
            className={`rounded-md px-2.5 py-1 text-xs font-medium ${
              toolFilter === "all"
                ? "bg-primary text-primary-foreground"
                : "bg-secondary text-secondary-foreground hover:bg-accent"
            }`}
          >
            All tools
          </button>
          {(overview?.tools ?? []).map((t) => (
            <button
              key={t.id}
              type="button"
              onClick={() => setToolFilter(t.id)}
              aria-pressed={toolFilter === t.id}
              className={`rounded-md px-2.5 py-1 text-xs font-medium ${
                toolFilter === t.id
                  ? "bg-primary text-primary-foreground"
                  : "bg-secondary text-secondary-foreground hover:bg-accent"
              }`}
            >
              {t.displayName}
            </button>
          ))}
        </div>
      </div>

      {filtered.length === 0 ? (
        <Card className="p-8 text-center text-sm text-muted-foreground">
          {rows.length === 0
            ? "No skills discovered yet. Install tools or adopt skills into the canonical store first."
            : "No skills match the current filters."}
        </Card>
      ) : (
        <ul className="space-y-3">
          {filtered.map((row) => (
            <SkillCard key={row.key} row={row} tools={overview?.tools ?? []} />
          ))}
        </ul>
      )}
    </section>
  );
}

function SkillCard({ row, tools }: { row: SkillRow; tools: SkillOverview["tools"] }) {
  return (
    <Card className="p-4" data-testid="skill-card">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2">
          <h2 className="font-semibold">{row.name}</h2>
          <Badge variant={STATUS_VARIANTS[row.status]}>{STATUS_LABELS[row.status]}</Badge>
          {row.canonical ? (
            <Badge variant="outline">canonical</Badge>
          ) : (
            <Badge variant="muted">unmanaged</Badge>
          )}
        </div>
        <div className="flex flex-wrap gap-1.5" aria-label="Tool matrix">
          {tools.map((tool) => {
            const installation = row.installations.find((i) => i.toolId === tool.id);
            return (
              <ToolMatrixChip
                key={tool.id}
                toolId={tool.id}
                label={shortTool(tool.id)}
                state={installation?.state}
              />
            );
          })}
        </div>
      </div>

      {row.description ? (
        <p className="mt-1.5 line-clamp-2 text-sm text-muted-foreground">{row.description}</p>
      ) : null}

      <p className="mt-2 font-mono text-xs text-muted-foreground">
        {row.canonical ? row.canonical.displayPath : row.installations[0]?.displayPath ?? ""}
      </p>

      <ValidationLines issues={collectIssues(row)} />
    </Card>
  );
}

function ToolMatrixChip({
  toolId,
  label,
  state,
}: {
  toolId: string;
  label: string;
  state?: SyncState;
}) {
  const known = state !== undefined && state !== "notInstalled" && state !== "disabled";
  return (
    <span
      data-tool={toolId}
      data-state={state ?? "none"}
      title={state ? `${label}: ${STATUS_LABELS[state]}` : `${label}: not installed`}
      className={`inline-flex items-center gap-1 rounded-md border px-2 py-0.5 text-xs ${
        known ? "border-border bg-card" : "border-transparent bg-muted text-muted-foreground"
      }`}
    >
      <span aria-hidden>{state ? STATUS_MARKS[state] : "-"}</span>
      {label}
    </span>
  );
}

function shortTool(id: string): string {
  switch (id) {
    case "claude":
      return "Claude";
    case "codex":
      return "Codex";
    case "cursor":
      return "Cursor";
    case "gemini":
      return "Gemini";
    default:
      return id;
  }
}

function collectIssues(row: SkillRow): ValidationIssue[] {
  const issues: ValidationIssue[] = [...(row.canonical?.validation ?? [])];
  for (const install of row.installations) {
    issues.push(...install.validation);
  }
  return issues.filter((i) => i.severity !== "note");
}

function ValidationLines({ issues }: { issues: ValidationIssue[] }) {
  if (issues.length === 0) return null;
  const errors = issues.filter((i) => i.severity === "error");
  const warnings = issues.filter((i) => i.severity === "warning");
  return (
    <ul className="mt-2 space-y-1">
      {errors.slice(0, 3).map((issue, idx) => (
        <li key={`e${idx}`} className="flex items-start gap-1.5 text-xs text-destructive">
          <XCircle className="mt-0.5 size-3 shrink-0" aria-hidden />
          <span>
            {issue.message}
            {issue.file ? ` (${issue.file})` : ""}
          </span>
        </li>
      ))}
      {warnings.slice(0, 3).map((issue, idx) => (
        <li key={`w${idx}`} className="flex items-start gap-1.5 text-xs text-warning">
          <AlertCircle className="mt-0.5 size-3 shrink-0" aria-hidden />
          <span>
            {issue.message}
            {issue.file ? ` (${issue.file})` : ""}
          </span>
        </li>
      ))}
    </ul>
  );
}
