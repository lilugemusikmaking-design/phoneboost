import { useCallback, useEffect, useRef, useState } from "react";
import axios from "axios";

export const UNKNOWN_PREFERENCE = Object.freeze({
  enabled: null, persistence: null, fresh: false, pending: false, observedAt: 0, error: null,
});

export function normalizePreference(value, observedAt, now = Date.now()) {
  if (!value || typeof value !== "object" || Array.isArray(value) ||
      Object.keys(value).length !== 2 || typeof value.enabled !== "boolean" ||
      !["DEFAULT", "SAVED", "ERROR"].includes(value.persistence) ||
      (value.persistence === "DEFAULT" && !value.enabled) ||
      (value.persistence === "ERROR" && value.enabled) ||
      now < observedAt || now - observedAt >= 3000) return { ...UNKNOWN_PREFERENCE };
  return { ...UNKNOWN_PREFERENCE, ...value, observedAt, fresh: true };
}

// Read generations prevent a pre-mutation or out-of-order response from
// restoring an obsolete state. A failed POST is reconciled, never replayed.
export default function useAutoUsePreference(capability) {
  const [preference, setPreference] = useState(UNKNOWN_PREFERENCE);
  const generation = useRef(0);
  const pending = useRef(false);
  const alive = useRef(true);
  const headers = useRef({ "X-PhoneBoost-Bridge-Token": capability });

  const refresh = useCallback(async () => {
    if (!capability || pending.current) return;
    const current = ++generation.current;
    const started = Date.now();
    try {
      const response = await axios.get("/bridge/v1/auto-use", { headers: headers.current, timeout: 5500 });
      if (alive.current && current === generation.current) {
        setPreference(normalizePreference(response.data, started));
      }
    } catch {
      if (alive.current && current === generation.current) setPreference({ ...UNKNOWN_PREFERENCE, error: "UNAVAILABLE" });
    }
  }, [capability]);

  const change = useCallback(async (enabled) => {
    if (!capability || pending.current || typeof enabled !== "boolean") return;
    pending.current = true;
    const current = ++generation.current;
    const started = Date.now();
    setPreference({ ...UNKNOWN_PREFERENCE, pending: true });
    try {
      const response = await axios.post("/bridge/v1/auto-use", { enabled }, { headers: headers.current, timeout: 5500 });
      const next = normalizePreference(response.data, started);
      if (!next.fresh || next.enabled !== enabled || next.persistence !== "SAVED") throw new Error("unconfirmed preference");
      if (alive.current && current === generation.current) setPreference(next);
    } catch {
      if (alive.current && current === generation.current) setPreference({ ...UNKNOWN_PREFERENCE, error: "OUTCOME_UNKNOWN" });
    } finally {
      pending.current = false;
    }
    // A new read is safe even after a lost reply; it cannot repeat a mutation.
    if (alive.current) await refresh();
  }, [capability, refresh]);

  useEffect(() => {
    alive.current = true;
    const timer = window.setInterval(() => setPreference((value) => {
      if (!value.fresh) return value;
      return normalizePreference({ enabled: value.enabled, persistence: value.persistence }, value.observedAt);
    }), 250);
    return () => { alive.current = false; generation.current += 1; window.clearInterval(timer); };
  }, []);

  return { preference, refresh, change };
}
