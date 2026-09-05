import { useEffect, useRef, useState } from "react";
import axios from "axios";
import {
  ArrowRight,
  Check,
  ChevronDown,
  ChevronRight,
  FileCheck2,
  Github,
  Layers,
  Leaf,
  Radio,
  ShieldCheck,
  X,
} from "lucide-react";
import { copyFor, GATE_COPY, stateLabel } from "../i18n";

const NAV = ["overview", "why", "runtime", "ladder", "evidence", "roadmap"];

export function liveBadgeTone(live) {
  return live.fresh
    ? "border-primary/30 bg-primary/10 text-primary"
    : "border-amber-500/30 bg-amber-500/10 text-amber-200";
}

function CanonicalState({ state, language, className = "" }) {
  const canonical = state || "UNAVAILABLE";
  const translated = stateLabel(canonical, language);
  const tone = ["READY", "ACTIVE", "AVAILABLE", "AUTHENTICATED", "REMOTE_SUCCESS"].includes(canonical)
    ? "border-primary/40 bg-primary/10 text-primary"
    : canonical === "UNKNOWN"
      ? "border-zinc-700 bg-zinc-900 text-zinc-300"
      : "border-amber-500/40 bg-amber-500/10 text-amber-200";
  return (
    <span className={`inline-flex items-center gap-1.5 rounded-full border px-2.5 py-1 font-mono text-[10px] font-semibold uppercase tracking-[0.12em] ${tone} ${className}`} title={canonical}>
      <span className="h-1.5 w-1.5 rounded-full bg-current" />
      <span>{translated}</span>
      {translated !== canonical && <span className="text-[9px] opacity-70">· {canonical}</span>}
    </span>
  );
}

function Card({ children, className = "" }) {
  return <div className={`rounded-2xl border border-white/10 bg-zinc-950/75 p-5 shadow-2xl shadow-black/20 ${className}`}>{children}</div>;
}

function SectionHeading({ kicker, title, body }) {
  return (
    <div className="mb-8 max-w-3xl">
      <p className="mb-3 font-mono text-xs uppercase tracking-[0.22em] text-primary">{kicker}</p>
      <h2 className="font-display text-3xl font-semibold tracking-tight text-white sm:text-4xl">{title}</h2>
      <p className="mt-3 text-base leading-7 text-zinc-400">{body}</p>
    </div>
  );
}

function LanguageToggle({ language, setLanguage, t }) {
  return (
    <div className="inline-flex rounded-lg border border-white/10 bg-zinc-900 p-1" aria-label={t.labels.languageSelector}>
      {["fr", "en"].map((choice) => (
        <button
          className={`rounded-md px-2.5 py-1 font-mono text-xs font-bold uppercase tracking-wider transition ${language === choice ? "bg-primary text-black" : "text-zinc-400 hover:text-white"}`}
          key={choice}
          onClick={() => setLanguage(choice)}
          type="button"
          aria-pressed={language === choice}
        >
          {choice}
        </button>
      ))}
    </div>
  );
}

export function Sidebar({ active, language, setLanguage, t }) {
  return (
    <aside className="fixed inset-x-0 top-0 z-30 border-b border-white/10 bg-zinc-950/90 px-4 py-3 backdrop-blur lg:inset-y-0 lg:right-auto lg:w-72 lg:border-b-0 lg:border-r lg:px-6 lg:py-8">
      <div className="flex items-center justify-between lg:block">
        <a href="#overview" className="group flex items-center gap-3">
          <span className="grid h-9 w-9 place-items-center rounded-xl bg-primary font-display text-lg font-bold text-black">PB</span>
          <span>
            <span className="block font-display text-lg font-semibold tracking-tight text-white">PhoneBoost</span>
            <span className="hidden font-mono text-[10px] uppercase tracking-[0.16em] text-zinc-500 sm:block">{t.controlCenter}</span>
          </span>
        </a>
        <LanguageToggle language={language} setLanguage={setLanguage} t={t} />
      </div>
      <nav className="mt-6 hidden space-y-1 lg:block" aria-label={t.labels.primaryNavigation}>
        {NAV.map((item, index) => (
          <a
            className={`flex items-center justify-between rounded-lg px-3 py-2.5 text-sm transition ${active === item ? "bg-primary/10 text-primary" : "text-zinc-400 hover:bg-white/5 hover:text-white"}`}
            href={`#${item}`}
            key={item}
          >
            <span>{t.navigation[item]}</span>
            <span className="font-mono text-[10px] text-zinc-600">0{index + 1}</span>
          </a>
        ))}
      </nav>
      <div className="mt-auto hidden rounded-xl border border-white/10 bg-white/[0.03] p-4 lg:block">
        <p className="font-mono text-[10px] uppercase tracking-[0.14em] text-zinc-500">Local-first</p>
        <p className="mt-2 text-sm leading-5 text-zinc-400">Linux x86-64 ↔ Android ARM64</p>
      </div>
    </aside>
  );
}

export function LiveControl({ live, compute, onCompute, language, t }) {
  const runtime = live.fresh ? live.runtime : null;
  const canCompute = Boolean(runtime && !compute.running);
  const source = compute.result?.execution_source;
  return (
    <Card className="relative overflow-hidden">
      <div className="absolute right-0 top-0 h-28 w-28 rounded-full bg-primary/10 blur-3xl" />
      <div className="relative flex flex-col gap-5">
        <div className="flex flex-wrap items-start justify-between gap-3">
          <div>
            <p className="font-mono text-[10px] uppercase tracking-[0.18em] text-zinc-500">{t.labels.lockedFixture}</p>
            <h3 className="mt-1 font-display text-xl font-semibold text-white">pb.native.blake3/1</h3>
          </div>
          <CanonicalState state={runtime?.remote_blake3_available ? "AVAILABLE" : "UNAVAILABLE"} language={language} />
        </div>
        <p className="text-sm leading-6 text-zinc-400">{t.labels.fixedAction}</p>
        <button
          className="inline-flex w-full items-center justify-center gap-2 rounded-xl bg-primary px-4 py-3 text-sm font-bold text-black transition hover:bg-primary/90 disabled:cursor-not-allowed disabled:bg-zinc-800 disabled:text-zinc-500"
          type="button"
          onClick={onCompute}
          disabled={!canCompute}
          data-testid="run-blake3-control"
        >
          {compute.running ? t.labels.running : t.labels.runFixture}
          {!compute.running && <ArrowRight size={16} />}
        </button>
        {!runtime && <p className="text-xs leading-5 text-amber-200">{t.labels.runtimeUnavailable}</p>}
        {compute.error && <p className="text-xs leading-5 text-amber-200">{t.labels.computeError}</p>}
        {compute.result && (
          <div className="rounded-xl border border-white/10 bg-black/30 p-3">
            <p className="font-mono text-[10px] uppercase tracking-[0.14em] text-zinc-500">{t.labels.lastAction}</p>
            <div className="mt-2 flex flex-wrap items-center gap-2">
              <CanonicalState state={source} language={language} />
              <span className="font-mono text-xs text-zinc-400" title={compute.result.auto_use_reason}>{compute.result.auto_use_reason}</span>
            </div>
            <p className="mt-2 break-all font-mono text-xs leading-5 text-zinc-300">{compute.result.digest_blake3_hex}</p>
          </div>
        )}
      </div>
    </Card>
  );
}

function Hero({ live, compute, onCompute, language, t }) {
  const liveLabel = live.fresh ? t.labels.liveFresh : t.labels.liveUnavailable;
  return (
    <section id="overview" className="grid gap-8 pt-24 lg:grid-cols-[minmax(0,1.35fr)_minmax(340px,.65fr)] lg:pt-8">
      <div className="reveal">
        <div data-testid="live-status-badge" className={`mb-6 inline-flex items-center gap-2 rounded-full border px-3 py-1.5 font-mono text-[10px] font-semibold uppercase tracking-[0.14em] ${liveBadgeTone(live)}`}>
          <Radio size={12} />
          {liveLabel}
        </div>
        <p className="font-mono text-xs uppercase tracking-[0.22em] text-zinc-500">{t.hero.kicker}</p>
        <h1 className="mt-4 max-w-4xl font-display text-5xl font-semibold leading-[0.98] tracking-tight text-white sm:text-6xl xl:text-7xl">
          {t.hero.titleLead} <span className="text-primary text-glow-green">{t.hero.titleAccent}</span>
        </h1>
        <p className="mt-6 max-w-2xl text-lg leading-8 text-zinc-300">{t.hero.body}</p>
        <p className="mt-4 max-w-2xl text-sm leading-6 text-zinc-500">{t.hero.support}</p>
        <div className="mt-8 flex flex-wrap gap-3">
          <a className="inline-flex items-center gap-2 rounded-xl bg-primary px-5 py-3 text-sm font-bold text-black transition hover:bg-primary/90" href="#why">
            {t.hero.primary} <ArrowRight size={16} />
          </a>
          <a className="inline-flex items-center gap-2 rounded-xl border border-white/15 px-5 py-3 text-sm font-semibold text-white transition hover:border-primary/50" href="#runtime">
            {t.hero.secondary}
          </a>
        </div>
      </div>
      <div className="reveal" style={{ animationDelay: "90ms" }}>
        <LiveControl live={live} compute={compute} onCompute={onCompute} language={language} t={t} />
      </div>
    </section>
  );
}

function WhyPhoneBoost({ t }) {
  return (
    <section id="why" className="border-t border-white/10 py-20">
      <SectionHeading kicker={t.why.kicker} title={t.why.title} body={t.why.body} />
      <div className="grid gap-4 md:grid-cols-2">
        {t.why.points.map((point) => (
          <Card key={point} className="flex items-start gap-3 ring-glow-green">
            <span className="mt-0.5 grid h-7 w-7 shrink-0 place-items-center rounded-full bg-primary/10 text-primary"><Check size={15} /></span>
            <p className="text-sm leading-6 text-zinc-300">{point}</p>
          </Card>
        ))}
      </div>
      <div className="mt-4 flex gap-3 rounded-xl border border-amber-500/20 bg-amber-500/5 p-4 text-sm leading-6 text-amber-100">
        <Leaf className="mt-0.5 shrink-0 text-amber-300" size={18} />
        <p>{t.why.limit}</p>
      </div>
    </section>
  );
}

function Runtime({ live, snapshot, arch, language, t }) {
  const runtime = live.fresh ? live.runtime : null;
  const current = runtime || {};
  const fields = [
    [t.runtime.daemon, current.local_daemon?.runtime_state || "UNAVAILABLE"],
    [t.runtime.authenticated, current.authenticated_session?.state || "UNAVAILABLE"],
    [t.runtime.provider, current.remote_blake3_available ? "AVAILABLE" : "UNAVAILABLE"],
    [t.runtime.autoUse, current.auto_use?.state || "UNAVAILABLE"],
  ];
  return (
    <section id="runtime" className="border-t border-white/10 py-20">
      <SectionHeading kicker={t.runtime.kicker} title={t.runtime.title} body={t.runtime.body} />
      <div className="grid gap-4 xl:grid-cols-3">
        <Card className="xl:col-span-2">
          <div className="mb-5 flex items-center justify-between gap-3">
            <p className="font-mono text-[10px] uppercase tracking-[0.16em] text-zinc-500">{live.fresh ? t.gates.fresh : t.gates.unavailable}</p>
            <CanonicalState state={live.fresh ? "AVAILABLE" : "UNAVAILABLE"} language={language} />
          </div>
          {live.fresh ? (
            <div className="grid gap-3 sm:grid-cols-2">
              {fields.map(([label, state]) => (
                <div className="rounded-xl border border-white/10 bg-black/25 p-4" key={label}>
                  <p className="text-xs text-zinc-500">{label}</p>
                  <div className="mt-2"><CanonicalState state={state} language={language} /></div>
                </div>
              ))}
            </div>
          ) : (
            <p className="text-sm leading-6 text-zinc-400">{t.labels.runtimeUnavailable}</p>
          )}
          <div className="mt-5 rounded-xl border border-white/10 bg-white/[0.025] p-4">
            <p className="font-mono text-[10px] uppercase tracking-[0.16em] text-zinc-500">{t.runtime.topology}</p>
            <p className="mt-2 text-sm leading-6 text-zinc-400">{t.runtime.separateNode}</p>
          </div>
        </Card>
        <Card>
          <div className="flex items-center gap-2 text-primary"><Layers size={17} /><p className="font-mono text-[10px] uppercase tracking-[0.16em]">Architecture</p></div>
          <ol className="mt-4 space-y-3">
            {(arch.layers || []).map((layer) => (
              <li className="border-l border-white/10 pl-3" key={layer.layer}>
                <p className="text-sm font-semibold text-zinc-200">{layer.layer} <span className="font-normal text-zinc-500">· {layer.role}</span></p>
                <p className="mt-1 text-xs leading-5 text-zinc-500">{layer.detail}</p>
              </li>
            ))}
          </ol>
        </Card>
      </div>
      <p className="mt-3 text-xs text-zinc-600">{snapshot.provenance === "RECORDED_EVIDENCE" ? t.labels.recordedEvidence : ""}</p>
    </section>
  );
}

function GateLadder({ live, snapshot, language, t }) {
  const runtime = live.fresh ? live.runtime : null;
  const observed = {
    paired: { state: "UNKNOWN", reason: "NOT_EXPOSED_BY_C12" },
    authenticated: { state: runtime?.authenticated_session?.state || "UNAVAILABLE", reason: runtime?.authenticated_session?.remote_worker_state || "UNAVAILABLE" },
    controller_lease: { state: "UNKNOWN", reason: "NOT_EXPOSED_BY_C12" },
    resource_admissible: { state: "UNKNOWN", reason: "NOT_EXPOSED_BY_C12" },
    provider_ready: { state: runtime?.remote_blake3_available ? "AVAILABLE" : "UNAVAILABLE", reason: "pb.native.blake3/1" },
  };
  return (
    <section id="ladder" className="border-t border-white/10 py-20">
      <SectionHeading kicker={t.gates.kicker} title={t.gates.title} body={t.gates.body} />
      <div className="grid gap-3">
        {(snapshot.gates || []).map((gate, index) => {
          const current = live.fresh ? observed[gate.id] || { state: "UNKNOWN", reason: "NOT_EXPOSED_BY_C12" } : { state: "UNAVAILABLE", reason: "NO_FRESH_LIVE_SNAPSHOT" };
          return (
            <Card key={gate.id} className="flex flex-col gap-3 sm:flex-row sm:items-center sm:justify-between">
              <div className="flex items-start gap-4">
                <span className="grid h-8 w-8 shrink-0 place-items-center rounded-lg border border-white/10 font-mono text-xs text-zinc-500">0{index + 1}</span>
                <div>
                  <p className="font-semibold text-zinc-100">{GATE_COPY[gate.id]?.[language] || gate.label}</p>
                  <p className="mt-1 text-sm leading-6 text-zinc-500">{gate.explanation}</p>
                  <p className="mt-1 font-mono text-[10px] uppercase tracking-[0.1em] text-zinc-600">{current.reason}</p>
                </div>
              </div>
              <CanonicalState state={current.state} language={language} className="shrink-0" />
            </Card>
          );
        })}
      </div>
      <details className="mt-4 rounded-xl border border-white/10 bg-white/[0.02] p-4">
        <summary className="cursor-pointer list-none text-sm font-semibold text-zinc-200"><span className="inline-flex items-center gap-2"><ShieldCheck size={16} className="text-primary" />{t.labels.unknownGates}<ChevronDown size={15} /></span></summary>
        <p className="mt-3 text-sm leading-6 text-zinc-400">{t.labels.boundaryCopy}</p>
      </details>
    </section>
  );
}

export function EvidenceDrawer({ item, api, t, onClose }) {
  const [content, setContent] = useState(null);
  const closeButtonRef = useRef(null);
  const previouslyFocusedRef = useRef(null);

  useEffect(() => {
    setContent(null);
    if (!item) return undefined;
    previouslyFocusedRef.current = document.activeElement;
    closeButtonRef.current?.focus();
    const onKeyDown = (event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        onClose();
      }
    };
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("keydown", onKeyDown);
      previouslyFocusedRef.current?.focus?.();
    };
  }, [item, onClose]);

  useEffect(() => {
    let cancelled = false;
    if (!item || !api.base) return undefined;
    axios.get(`${api.base}/evidence/${item.id}`).then((response) => {
      if (!cancelled) setContent(response.data);
    }).catch(() => {
      if (!cancelled) setContent(null);
    });
    return () => { cancelled = true; };
  }, [api.base, item]);
  if (!item) return null;
  return (
    <div className="fixed inset-0 z-50 flex justify-end bg-black/60 p-3 backdrop-blur-sm" role="dialog" aria-modal="true" aria-labelledby="evidence-drawer-title">
      <div className="h-full w-full max-w-xl overflow-y-auto rounded-2xl border border-white/10 bg-zinc-950 p-6 shadow-2xl">
        <div className="flex items-start justify-between gap-4"><div><p className="font-mono text-[10px] uppercase tracking-[0.16em] text-primary">{t.evidence.kicker}</p><h3 id="evidence-drawer-title" className="mt-2 text-xl font-semibold text-white">{item.title}</h3></div><button ref={closeButtonRef} data-testid="evidence-drawer-close" aria-label={t.labels.close} className="rounded-lg p-2 text-zinc-400 hover:bg-white/10 hover:text-white" onClick={onClose} type="button"><X size={18} /></button></div>
        <p className="mt-5 text-sm leading-6 text-zinc-300">{item.summary}</p>
        <p className="mt-5 font-mono text-xs text-zinc-500">{t.evidence.source}: {item.source}</p>
        {api.base && <pre className="mt-5 overflow-x-auto rounded-xl border border-white/10 bg-black/30 p-4 text-xs leading-5 text-zinc-400">{content ? JSON.stringify(content, null, 2) : t.evidence.loading}</pre>}
      </div>
    </div>
  );
}

function Evidence({ evidence, fixtures, api, t }) {
  const [selected, setSelected] = useState(null);
  return (
    <section id="evidence" className="border-t border-white/10 py-20">
      <SectionHeading kicker={t.evidence.kicker} title={t.evidence.title} body={t.evidence.body} />
      <div className="mb-4 flex flex-wrap gap-2 font-mono text-[10px] uppercase tracking-[0.12em] text-zinc-500"><span className="rounded-full border border-white/10 px-3 py-1">{t.labels.recordedEvidence}</span><span className="rounded-full border border-white/10 px-3 py-1">fixtures: {fixtures.file_count ?? "UNKNOWN"}</span></div>
      <div className="grid gap-4 md:grid-cols-2 xl:grid-cols-3">
        {evidence.map((item) => (
          <Card key={item.id} className="flex flex-col justify-between ring-glow-green">
            <div><div className="flex items-center gap-2 text-primary"><FileCheck2 size={16} /><span className="font-mono text-[10px] uppercase tracking-[0.14em]">{item.kind}</span></div><h3 className="mt-4 font-display text-lg font-semibold text-white">{item.title}</h3><p className="mt-3 text-sm leading-6 text-zinc-400">{item.summary}</p></div>
            <button className="mt-5 inline-flex items-center gap-2 self-start text-sm font-semibold text-primary hover:text-white" type="button" onClick={() => setSelected(item)}>{t.evidence.open}<ChevronRight size={16} /></button>
          </Card>
        ))}
      </div>
      <EvidenceDrawer item={selected} api={api} t={t} onClose={() => setSelected(null)} />
    </section>
  );
}

function Roadmap({ roadmap, t }) {
  const columns = [
    [t.roadmap.working, roadmap.working_now || [], "text-primary"],
    [t.roadmap.next, roadmap.next || [], "text-amber-300"],
    [t.roadmap.future, roadmap.future || [], "text-zinc-400"],
  ];
  return (
    <section id="roadmap" className="border-t border-white/10 py-20">
      <SectionHeading kicker={t.roadmap.kicker} title={t.roadmap.title} body={t.roadmap.body} />
      <div className="grid gap-4 lg:grid-cols-3">
        {columns.map(([title, entries, color]) => (
          <Card key={title}><p className={`font-mono text-[10px] uppercase tracking-[0.16em] ${color}`}>{title}</p><ul className="mt-5 space-y-4">{entries.length ? entries.map((entry) => <li className="flex gap-3 text-sm leading-6 text-zinc-400" key={entry}><span className={`mt-2 h-1.5 w-1.5 shrink-0 rounded-full ${color.replace("text-", "bg-")}`} />{entry}</li>) : <li className="text-sm text-zinc-600">—</li>}</ul></Card>
        ))}
      </div>
    </section>
  );
}

export default function Dashboard({ api, snapshot, live, evidence, roadmap, arch, fixtures, compute, onCompute, language, setLanguage }) {
  const [active, setActive] = useState("overview");
  const sectionRef = useRef(null);
  const t = copyFor(language);
  useEffect(() => {
    const observer = new IntersectionObserver((entries) => {
      const visible = entries.filter((entry) => entry.isIntersecting).sort((a, b) => b.intersectionRatio - a.intersectionRatio)[0];
      if (visible) setActive(visible.target.id);
    }, { rootMargin: "-25% 0px -60%" });
    const sections = NAV.map((id) => document.getElementById(id)).filter(Boolean);
    sections.forEach((section) => observer.observe(section));
    return () => observer.disconnect();
  }, []);
  return (
    <div className="min-h-screen bg-grain bg-[#0a0a0a] text-zinc-100">
      <Sidebar active={active} language={language} setLanguage={setLanguage} t={t} />
      <main ref={sectionRef} className="mx-auto max-w-7xl px-5 pb-10 lg:ml-72 lg:px-12 xl:px-16">
        <Hero live={live} compute={compute} onCompute={onCompute} language={language} t={t} />
        <WhyPhoneBoost t={t} />
        <Runtime live={live} snapshot={snapshot} arch={arch} language={language} t={t} />
        <GateLadder live={live} snapshot={snapshot} language={language} t={t} />
        <Evidence evidence={evidence} fixtures={fixtures} api={api} t={t} />
        <Roadmap roadmap={roadmap} t={t} />
        <footer className="border-t border-white/10 py-8 text-sm leading-6 text-zinc-500"><div className="flex flex-col justify-between gap-4 sm:flex-row"><p className="max-w-3xl">{t.footer}</p><a className="inline-flex shrink-0 items-center gap-2 text-zinc-400 hover:text-primary" href={snapshot.release?.repo} target="_blank" rel="noreferrer"><Github size={16} />Repository</a></div></footer>
      </main>
    </div>
  );
}
