import { useEffect, useState } from "react";
import { CloudDownload, CloudUpload, GitBranch, GitCommitHorizontal } from "lucide-react";
import { api, normalizeError } from "@/lib/api";
import { useI18n } from "@/lib/i18n";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import type { GitStatus, SkillSyncError } from "@/types/domain";

/**
 * Machine sync (design doc §34/§35): the canonical store may be a git
 * repository. Every git action is explicit — SkillSync never commits,
 * pulls or pushes on its own.
 */
export function GitCard() {
  const { t } = useI18n();
  const [status, setStatus] = useState<GitStatus | null>(null);
  const [message, setMessage] = useState("Sync skills");
  const [busy, setBusy] = useState<string | null>(null);
  const [output, setOutput] = useState<string | null>(null);
  const [error, setError] = useState<SkillSyncError | null>(null);

  useEffect(() => {
    void api
      .gitStatus()
      .then(setStatus)
      .catch((e) => setError(normalizeError(e)));
  }, []);

  const run = async (action: () => Promise<string>, label: string) => {
    setBusy(label);
    setError(null);
    try {
      setOutput(await action());
      setStatus(await api.gitStatus());
    } catch (e) {
      setError(normalizeError(e));
    } finally {
      setBusy(null);
    }
  };

  return (
    <Card className="space-y-3 p-5">
      <div className="flex items-center justify-between">
        <div>
          <h2 className="font-semibold">{t("git.title")}</h2>
          <p className="text-sm text-muted-foreground">{t("git.description")}</p>
        </div>
        {status?.isRepo ? (
          <Badge variant="success">
            <GitBranch className="size-3" aria-hidden />
            {status.branch ?? "(detached)"}
            {status.hasUpstream
              ? ` ↑${status.ahead} ↓${status.behind}`
              : t("git.noUpstream")}
          </Badge>
        ) : (
          <Badge variant="muted">{t("git.notRepo")}</Badge>
        )}
      </div>

      {status?.isRepo ? (
        <>
          {status.changedSkills.length > 0 ? (
            <ul className="space-y-1 text-sm">
              {status.changedSkills.map((change) => (
                <li key={change.skillId} className="flex items-baseline gap-2">
                  <Badge
                    variant={
                      change.change === "deleted"
                        ? "destructive"
                        : change.change === "added"
                          ? "success"
                          : "secondary"
                    }
                  >
                    {change.change}
                  </Badge>
                  <span className="font-medium">{change.skillId}</span>
                  <span className="text-xs text-muted-foreground">
                    {t("git.files", { count: change.files.length })}
                  </span>
                </li>
              ))}
            </ul>
          ) : (
            <p className="text-sm text-muted-foreground">{t("git.workingTreeClean")}</p>
          )}

          <div className="flex flex-wrap items-center gap-2">
            <Button
              variant="outline"
              size="sm"
              disabled={busy !== null || !status.hasUpstream}
              onClick={() => void run(() => api.gitPull(), "pull")}
            >
              <CloudDownload className="size-3.5" aria-hidden />
              {busy === "pull" ? t("git.pulling") : t("git.pull")}
            </Button>
            <Input
              value={message}
              onChange={(e) => setMessage(e.target.value)}
              placeholder={t("git.commitMessage")}
              className="h-8 max-w-xs"
              aria-label={t("git.commitMessage")}
            />
            <Button
              variant="outline"
              size="sm"
              disabled={busy !== null || status.changedSkills.length === 0}
              onClick={() => void run(() => api.gitCommit(message), "commit")}
            >
              <GitCommitHorizontal className="size-3.5" aria-hidden />
              {busy === "commit" ? t("git.committing") : t("git.commitAll")}
            </Button>
            <Button
              variant="outline"
              size="sm"
              disabled={busy !== null || !status.hasUpstream}
              onClick={() => void run(() => api.gitPush(), "push")}
            >
              <CloudUpload className="size-3.5" aria-hidden />
              {busy === "push" ? t("git.pushing") : t("git.push")}
            </Button>
          </div>
          <p className="text-xs text-muted-foreground">{t("git.ffNote")}</p>
        </>
      ) : (
        <p className="text-sm text-muted-foreground">
          {t("git.initHint")}
        </p>
      )}

      {output ? (
        <pre className="max-h-40 overflow-auto rounded-md border bg-muted p-2 text-xs" data-testid="git-output">
          {output}
        </pre>
      ) : null}
      {error ? (
        <p role="alert" className="text-xs text-destructive">
          {error.code}: {error.message}
        </p>
      ) : null}
    </Card>
  );
}
