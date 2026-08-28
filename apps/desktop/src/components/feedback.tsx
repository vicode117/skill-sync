import type { ReactNode } from "react";
import { AlertTriangle, Info, RefreshCw } from "lucide-react";
import type { SkillSyncError } from "@/types/domain";

export function ErrorBanner({ error }: { error: SkillSyncError }) {
  return (
    <div
      role="alert"
      className="flex items-start gap-3 rounded-lg border border-destructive/30 bg-destructive/5 p-4 text-sm"
    >
      <AlertTriangle className="mt-0.5 size-4 shrink-0 text-destructive" aria-hidden />
      <div>
        <p className="font-medium text-destructive">Native operation failed</p>
        <p className="mt-1 text-foreground">{error.message}</p>
        <p className="mt-1 text-xs text-muted-foreground">
          code: {error.code}
          {error.path ? ` · path: ${error.path}` : ""}
          {error.tool ? ` · tool: ${error.tool}` : ""}
        </p>
      </div>
    </div>
  );
}

export function InfoNote({ children }: { children: ReactNode }) {
  return (
    <div className="flex items-start gap-3 rounded-lg border bg-card p-4 text-sm text-muted-foreground">
      <Info className="mt-0.5 size-4 shrink-0" aria-hidden />
      <div>{children}</div>
    </div>
  );
}

export function RefreshButton({
  onClick,
  loading,
}: {
  onClick: () => void;
  loading: boolean;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      disabled={loading}
      className="inline-flex h-9 items-center gap-2 rounded-md border border-input bg-card px-3 text-sm font-medium hover:bg-accent disabled:opacity-50"
    >
      <RefreshCw className={`size-4 ${loading ? "animate-spin" : ""}`} aria-hidden />
      Refresh
    </button>
  );
}
