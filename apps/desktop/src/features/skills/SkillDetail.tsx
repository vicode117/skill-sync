import { useState } from "react";
import { ExternalLink, FolderTree, Loader2 } from "lucide-react";
import { api, normalizeError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { Button } from "@/components/ui/button";
import type { SkillRow, SkillSyncError } from "@/types/domain";

/**
 * Skill detail view (design doc §26): files, fingerprint, SKILL.md
 * preview, and open-in-file-explorer. Deliberately not an editor —
 * external editors remain first-class.
 */
export function SkillDetail({ row }: { row: SkillRow }) {
  const { t } = useI18n();
  const [preview, setPreview] = useState<string | null>(null);
  const [previewPath, setPreviewPath] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<SkillSyncError | null>(null);

  const openDir = async (path: string) => {
    setError(null);
    try {
      await api.openInExplorer(path);
    } catch (e) {
      setError(normalizeError(e));
    }
  };

  const showPreview = async (path: string) => {
    if (previewPath === path) {
      setPreview(null);
      setPreviewPath(null);
      return;
    }
    setLoading(true);
    setError(null);
    try {
      setPreview(await api.readSkillFile(path));
      setPreviewPath(path);
    } catch (e) {
      setError(normalizeError(e));
    } finally {
      setLoading(false);
    }
  };

  const canonical = row.canonical;
  const fingerprint = canonical?.fingerprint ?? row.installations[0]?.fingerprint;

  return (
    <div className="mt-3 space-y-3 border-t pt-3 text-xs" data-testid="skill-detail">
      <div className="grid gap-2 sm:grid-cols-2">
        {fingerprint ? (
          <p>
            <span className="text-muted-foreground">{t("skills.fingerprint")}</span>
            <span className="font-mono">
              {fingerprint.slice(0, 16)}…
            </span>
          </p>
        ) : null}
        <p>
          <span className="text-muted-foreground">{t("skills.installations")}</span>
          {row.installations.length}
        </p>
      </div>

      <div className="space-y-1">
        <p className="flex items-center gap-1 font-medium">
          <FolderTree className="size-3" aria-hidden /> {t("skills.locations")}
        </p>
        <ul className="space-y-1">
          {canonical ? (
            <li className="flex flex-wrap items-center gap-2">
              <span className="font-mono">{canonical.displayPath}</span>
              <Badgeish text="canonical" />
              <Button
                size="sm"
                variant="ghost"
                className="h-6 px-1.5"
                onClick={() => void openDir(canonical.path)}
                aria-label={t("skills.openCanonical")}
              >
                <ExternalLink className="size-3" aria-hidden /> {t("skills.open")}
              </Button>
            </li>
          ) : null}
          {row.installations
            .filter((i) => i.path !== "")
            .map((install) => (
              <li key={`${install.toolId}:${install.path}`} className="flex flex-wrap items-center gap-2">
                <span className="font-mono">{install.displayPath}</span>
                <Badgeish text={`${install.toolId} · ${install.state}`} />
                <Button
                  size="sm"
                  variant="ghost"
                  className="h-6 px-1.5"
                  onClick={() => void openDir(install.path)}
                  aria-label={t("skills.openToolDir", { tool: install.toolId })}
                >
                  <ExternalLink className="size-3" aria-hidden /> {t("skills.open")}
                </Button>
              </li>
            ))}
        </ul>
      </div>

      <div>
        <div className="flex items-center gap-2">
          <Button
            size="sm"
            variant="outline"
            className="h-7"
            disabled={loading}
            onClick={() => {
              const mdPath = canonical
                ? `${canonical.path}/SKILL.md`
                : (row.installations.find((i) => i.path !== "")?.path ?? "") + "/SKILL.md";
              void showPreview(mdPath);
            }}
          >
            {loading ? <Loader2 className="size-3 animate-spin" aria-hidden /> : null}
            {previewPath !== null ? t("skills.hidePreview") : t("skills.preview")}
          </Button>
          <span className="text-muted-foreground">{t("skills.previewNote")}</span>
        </div>
        {preview !== null ? (
          <pre
            className="mt-2 max-h-64 overflow-auto rounded-md border bg-muted p-2 font-mono text-[11px]"
            data-testid="skill-preview"
          >
            {preview}
          </pre>
        ) : null}
      </div>

      {error ? (
        <p role="alert" className="text-destructive">
          {error.code}: {error.message}
        </p>
      ) : null}
    </div>
  );
}

function Badgeish({ text }: { text: string }) {
  return (
    <span className="rounded border border-border bg-muted px-1.5 py-0.5 text-[10px] text-muted-foreground">
      {text}
    </span>
  );
}
