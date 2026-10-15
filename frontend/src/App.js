import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import axios from "axios";
import "@/App.css";
import Dashboard from "@/pages/Dashboard";
import useAutoUsePreference from "@/useAutoUsePreference";
import useFileCompute from "@/useFileCompute";
import FileComputePanel from "@/FileComputePanel";
import { safeRecordedPayloads } from "@/recordedApi";
import {
  consumeBridgeToken,
  expireLiveState,
  normalizeComputeResult,
  normalizeLiveSnapshot,
  unavailableLive,
} from "@/liveBridge";
import {
  HOSTED_LIVE_UNAVAILABLE,
  RECORDED_ARCHITECTURE,
  RECORDED_EVIDENCE,
  RECORDED_FIXTURES,
  RECORDED_ROADMAP,
  RECORDED_SNAPSHOT,
} from "@/recordedData";
import { loadLanguagePreference, storeLanguagePreference } from "@/i18n";

const BACKEND_URL = process.env.REACT_APP_BACKEND_URL || "";
const RECORDED_API = BACKEND_URL ? `${BACKEND_URL}/api` : null;
const BRIDGE_CAPABILITY =
  typeof window === "undefined" ? null : consumeBridgeToken(window.location, window.history);
const COMPUTE_ACTION_UNAVAILABLE = "COMPUTE_ACTION_UNAVAILABLE";

export default function App() {
  const { preference, refresh: refreshPreference, change: changePreference } = useAutoUsePreference(BRIDGE_CAPABILITY);
  const runtimeGeneration = useRef(0);
  const recordedApiBase = BRIDGE_CAPABILITY ? null : RECORDED_API;
  const [language, setLanguage] = useState(() =>
    loadLanguagePreference(typeof window === "undefined" ? null : window)
  );

  const [snapshot, setSnapshot] = useState(RECORDED_SNAPSHOT);
  const [live, setLive] = useState(HOSTED_LIVE_UNAVAILABLE);
  const [evidence, setEvidence] = useState(RECORDED_EVIDENCE);
  const [roadmap, setRoadmap] = useState(RECORDED_ROADMAP);
  const [arch, setArch] = useState(RECORDED_ARCHITECTURE);
  const [fixtures, setFixtures] = useState(RECORDED_FIXTURES);
  const [compute, setCompute] = useState({ running: false, result: null, error: null });
  const fileCompute = useFileCompute(BRIDGE_CAPABILITY, preference, compute.running);

  useEffect(() => {
    if (!recordedApiBase) return undefined;
    let cancelled = false;
    (async () => {
      try {
        const [snap, ev, rm, ar, fx] = await Promise.all([
          axios.get(`${recordedApiBase}/system/snapshot`),
          axios.get(`${recordedApiBase}/evidence/index`),
          axios.get(`${recordedApiBase}/roadmap`),
          axios.get(`${recordedApiBase}/architecture`),
          axios.get(`${recordedApiBase}/fixtures/manifest`),
        ]);
        if (cancelled) return;
        const safe = safeRecordedPayloads({
          snapshot: snap?.data,
          evidence: ev?.data,
          roadmap: rm?.data,
          arch: ar?.data,
          fixtures: fx?.data,
        });
        setSnapshot(safe.snapshot);
        setEvidence(safe.evidence);
        setRoadmap(safe.roadmap);
        setArch(safe.arch);
        setFixtures(safe.fixtures);
      } catch {
        // Checked-in recorded data remains visible and separately labeled.
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [recordedApiBase]);

  useEffect(() => {
    if (!BRIDGE_CAPABILITY) return undefined;
    let cancelled = false;
    const headers = { "X-PhoneBoost-Bridge-Token": BRIDGE_CAPABILITY };
    let polling = false;
    const poll = async () => {
      if (polling) return;
      polling = true;
      const currentGeneration = runtimeGeneration.current;
      try {
        await refreshPreference();
        if (cancelled || currentGeneration !== runtimeGeneration.current) return;
        const response = await axios.get("/bridge/v1/snapshot", { headers, timeout: 2500 });
        if (!cancelled && currentGeneration === runtimeGeneration.current) setLive(normalizeLiveSnapshot(response.data));
      } catch {
        if (!cancelled && currentGeneration === runtimeGeneration.current) {
          setLive((previous) => unavailableLive("LOCAL_RUNTIME_UNAVAILABLE", previous.runtime));
          setCompute((current) => ({ ...current, result: null }));
        }
      } finally {
        polling = false;
      }
    };
    poll();
    const pollTimer = window.setInterval(poll, 2000);
    const freshnessTimer = window.setInterval(() => {
      if (!cancelled) setLive((current) => expireLiveState(current));
    }, 250);
    return () => {
      cancelled = true;
      window.clearInterval(pollTimer);
      window.clearInterval(freshnessTimer);
    };
  }, [refreshPreference]);

  useEffect(() => {
    if (!live.fresh) {
      setCompute((current) =>
        current.result ? { ...current, result: null } : current
      );
    }
  }, [live.fresh]);

  useEffect(() => {
    document.documentElement.lang = language;
    storeLanguagePreference(typeof window === "undefined" ? null : window, language);
  }, [language]);

  const runCompute = useCallback(async () => {
    if (!BRIDGE_CAPABILITY || fileCompute.running || compute.running || !preference.fresh || preference.pending || preference.enabled !== true) return;
    setCompute({ running: true, result: null, error: null });
    try {
      const response = await axios.post(
        "/bridge/v1/compute/blake3",
        { fixture: "c10-abc-v1" },
        {
          headers: { "X-PhoneBoost-Bridge-Token": BRIDGE_CAPABILITY },
          timeout: 65000,
        }
      );
      const result = normalizeComputeResult(response.data);
      if (!result) throw new Error("invalid bridge response");
      setCompute({ running: false, result, error: null });
    } catch {
      setCompute({ running: false, result: null, error: COMPUTE_ACTION_UNAVAILABLE });
    }
  }, [preference, fileCompute.running, compute.running]);

  const api = useMemo(() => ({ base: recordedApiBase }), [recordedApiBase]);

  const changeAutoUse = useCallback(async (enabled) => {
    runtimeGeneration.current += 1;
    setLive(unavailableLive("PREFERENCE_CHANGING"));
    await changePreference(enabled);
    // Readiness observed before/during an epoch change cannot be reused as
    // proof after ON, even when its previous three-second TTL has not expired.
    runtimeGeneration.current += 1;
    setLive(unavailableLive("PREFERENCE_CHANGED"));
  }, [changePreference]);

  return (
    <Dashboard
      api={api}
      snapshot={snapshot}
      live={live}
      evidence={evidence}
      roadmap={roadmap}
      arch={arch}
      fixtures={fixtures}
      compute={compute}
      fileBusy={fileCompute.running}
      fileControl={BRIDGE_CAPABILITY ? <FileComputePanel task={fileCompute} preference={preference} otherRunning={compute.running} language={language} /> : null}
      preference={preference}
      onAutoUse={changeAutoUse}
      onCompute={runCompute}
      language={language}
      setLanguage={setLanguage}
    />
  );
}
