import { useCallback, useEffect, useRef, useState } from "react";
import { api, normalizeError } from "@/lib/api";
import type { SkillOverview, SkillSyncError } from "@/types/domain";

/**
 * Loads the read-only overview from the native core. Both the Skills and
 * Tools pages share one instance so a refresh updates everything.
 *
 * The initial load runs on mount (bugfix: `loading` starts true, so the
 * Refresh button stays disabled until the first load completes — without
 * the mount effect the UI would sit on "Loading…" forever).
 */
export function useOverview() {
  const [overview, setOverview] = useState<SkillOverview | null>(null);
  const [error, setError] = useState<SkillSyncError | null>(null);
  const [loading, setLoading] = useState(true);
  const mounted = useRef(true);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const result = await api.scanOverview();
      if (mounted.current) {
        setOverview(result);
        setError(null);
      }
    } catch (e) {
      if (mounted.current) {
        setError(normalizeError(e));
      }
    } finally {
      if (mounted.current) {
        setLoading(false);
      }
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    void refresh();
    return () => {
      mounted.current = false;
    };
  }, [refresh]);

  return { overview, error, loading, refresh };
}
