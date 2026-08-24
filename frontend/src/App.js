import { useEffect, useMemo, useState } from "react";
import axios from "axios";
import "@/App.css";
import Dashboard from "@/pages/Dashboard";

const BACKEND_URL = process.env.REACT_APP_BACKEND_URL;
const API = `${BACKEND_URL}/api`;

export default function App() {
  const [snapshot, setSnapshot] = useState(null);
  const [live, setLive] = useState(null);
  const [evidence, setEvidence] = useState([]);
  const [roadmap, setRoadmap] = useState(null);
  const [arch, setArch] = useState(null);
  const [fixtures, setFixtures] = useState(null);
  const [mode, setMode] = useState("RECORDED_EVIDENCE"); // never silently fall back
  const [error, setError] = useState(null);

  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [snap, lv, ev, rm, ar, fx] = await Promise.all([
          axios.get(`${API}/system/snapshot`),
          axios.get(`${API}/live/probe`),
          axios.get(`${API}/evidence/index`),
          axios.get(`${API}/roadmap`),
          axios.get(`${API}/architecture`),
          axios.get(`${API}/fixtures/manifest`),
        ]);
        if (cancelled) return;
        setSnapshot(snap.data);
        setLive(lv.data);
        setEvidence(ev.data.items || []);
        setRoadmap(rm.data);
        setArch(ar.data);
        setFixtures(fx.data);
      } catch (e) {
        if (!cancelled) setError(e?.message || "Failed to load Control Center");
      }
    })();
    return () => { cancelled = true; };
  }, []);

  const api = useMemo(() => ({ base: API }), []);

  if (error) {
    return (
      <div className="min-h-screen bg-[#0b0d10] text-neutral-200 flex items-center justify-center p-8">
        <div data-testid="app-error" className="max-w-lg border border-neutral-800 rounded-lg p-6">
          <div className="text-xs uppercase tracking-widest text-amber-400 mb-2">Control Center unavailable</div>
          <div className="font-mono text-sm text-neutral-300 break-all">{error}</div>
        </div>
      </div>
    );
  }

  if (!snapshot || !live || !roadmap || !arch) {
    return (
      <div className="min-h-screen bg-[#0b0d10] text-neutral-500 flex items-center justify-center">
        <div data-testid="app-loading" className="font-mono text-xs tracking-widest uppercase">Loading recorded evidence…</div>
      </div>
    );
  }

  return (
    <Dashboard
      api={api}
      snapshot={snapshot}
      live={live}
      evidence={evidence}
      roadmap={roadmap}
      arch={arch}
      fixtures={fixtures}
      mode={mode}
      setMode={setMode}
    />
  );
}
