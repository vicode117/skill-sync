import { useState } from "react";
import { api, normalizeError } from "@/lib/api";
import type { SkillOverview, SkillSyncError } from "@/types/domain";

/**
 * Loads the read-only overview from the native core. Both the Skills and
 * Tools pages share one instance so a refresh updates everything.
 */
export function useOverview() {
  const [overview, setOverview] = useState<SkillOverview | null>(null);
  const [error, setError] = useState<SkillSyncError | null>(null);
  const [loading, setLoading] = useState(true);

  const refresh = async () => {
    setLoading(true);
    try {
      setOverview(await api.scanOverview());
      setError(null);
    } catch (e) {
      setError(normalizeError(e));
    } finally {
      setLoading(false);
    }
  };

  return { overview, error, loading, refresh };
}
