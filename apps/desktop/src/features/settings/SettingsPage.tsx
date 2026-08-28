import { useEffect, useState } from "react";
import { CheckCircle2, CircleAlert, CircleX, Stethoscope } from "lucide-react";
import { api, normalizeError } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import type { CheckStatus, Config, DoctorReport, SyncMethod } from "@/types/domain";

export function SettingsPage() {
  const [config, setConfig] = useState<Config | null>(null);
  const [canonicalRoot, setCanonicalRoot] = useState("");
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [doctor, setDoctor] = useState<DoctorReport | null>(null);
  const [doctorBusy, setDoctorBusy] = useState(false);

  useEffect(() => {
    void api
      .getConfig()
      .then((loaded) => {
        setConfig(loaded);
        setCanonicalRoot(loaded.canonicalSkillRoot);
      })
      .catch((e) => setError(normalizeError(e).message));
  }, []);

  const save = async () => {
    if (!config) return;
    setSaving(true);
    setSaved(false);
    try {
      const next = await api.saveConfig({ ...config, canonicalSkillRoot: canonicalRoot });
      setConfig(next);
      setCanonicalRoot(next.canonicalSkillRoot);
      setSaved(true);
      setError(null);
    } catch (e) {
      setError(normalizeError(e).message);
    } finally {
      setSaving(false);
    }
  };

  const runDoctor = async () => {
    setDoctorBusy(true);
    try {
      setDoctor(await api.runDoctor());
    } catch (e) {
      setError(normalizeError(e).message);
    } finally {
      setDoctorBusy(false);
    }
  };

  return (
    <section aria-label="Settings" className="max-w-2xl space-y-6">
      <div>
        <h1 className="text-xl font-semibold">Settings</h1>
        <p className="text-sm text-muted-foreground">
          Configuration lives in <code>~/.skillsync/config.json</code>. Skill files are never
          stored here.
        </p>
      </div>

      {error ? (
        <div role="alert" className="rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">
          {error}
        </div>
      ) : null}

      <Card className="space-y-4 p-5">
        <div className="space-y-2">
          <Label htmlFor="canonical-root">Canonical skill root</Label>
          <Input
            id="canonical-root"
            value={canonicalRoot}
            onChange={(e) => setCanonicalRoot(e.target.value)}
            placeholder="~/.agents/skills"
          />
          <p className="text-xs text-muted-foreground">
            The single source of truth for your skills. May start with <code>~</code>. It can be
            a plain folder or a git repository.
          </p>
        </div>

        <div className="space-y-2">
          <Label htmlFor="sync-method">Sync method</Label>
          <select
            id="sync-method"
            value={config?.syncMethod ?? "auto"}
            onChange={(e) =>
              config && setConfig({ ...config, syncMethod: e.target.value as SyncMethod })
            }
            className="h-9 w-full rounded-md border border-input bg-card px-3 text-sm"
          >
            <option value="auto">auto — link where safe, copy otherwise</option>
            <option value="symlink">symlink — single physical copy</option>
            <option value="copy">copy — fingerprint-tracked copies</option>
          </select>
          <p className="text-xs text-muted-foreground">
            The synchronization engine arrives in the next slice; the choice is stored now.
          </p>
        </div>

        <div className="flex items-center gap-3">
          <Button onClick={() => void save()} disabled={saving || !config}>
            {saving ? "Saving…" : "Save settings"}
          </Button>
          {saved ? (
            <span className="text-sm text-success" role="status">
              Saved.
            </span>
          ) : null}
        </div>
      </Card>

      <Card className="space-y-3 p-5">
        <div className="flex items-center justify-between">
          <div>
            <h2 className="font-semibold">Diagnostics</h2>
            <p className="text-sm text-muted-foreground">
              Environment checks shared with <code>skillsync doctor</code>.
            </p>
          </div>
          <Button variant="outline" onClick={() => void runDoctor()} disabled={doctorBusy}>
            <Stethoscope className="size-4" aria-hidden />
            {doctorBusy ? "Running…" : "Run doctor"}
          </Button>
        </div>
        {doctor ? (
          <ul className="space-y-1.5">
            {doctor.checks.map((check) => (
              <li key={check.id} className="flex items-start gap-2 text-sm">
                <StatusIcon status={check.status} />
                <span>
                  <span className="font-medium">{check.title}</span>{" "}
                  <span className="text-muted-foreground">{check.detail}</span>
                </span>
              </li>
            ))}
          </ul>
        ) : (
          <p className="text-sm text-muted-foreground">Run the check to inspect this machine.</p>
        )}
      </Card>
    </section>
  );
}

function StatusIcon({ status }: { status: CheckStatus }) {
  if (status === "ok") {
    return <CheckCircle2 className="mt-0.5 size-4 shrink-0 text-success" aria-hidden />;
  }
  if (status === "warning") {
    return <CircleAlert className="mt-0.5 size-4 shrink-0 text-warning" aria-hidden />;
  }
  return <CircleX className="mt-0.5 size-4 shrink-0 text-destructive" aria-hidden />;
}
