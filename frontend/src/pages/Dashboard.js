import { useEffect, useRef, useState } from "react";
import axios from "axios";
import {
  Activity, ChevronDown, ChevronRight, CirclePower, Cpu, FileCheck2,
  Gauge, HardDrive, Home, Info, Link2, ListChecks, Radio, Server,
  ShieldCheck, Smartphone, X,
} from "lucide-react";
import { copyFor, GATE_COPY, stateLabel } from "../i18n";

const PARTICIPATION_KEY = "phoneboost.participation-enabled";
const NAV = ["overview", "resources", "activity", "evidence"];
const UI_COPY = {
  fr: {
    subtitle: "Hôte Linux ↔ worker Android", eyebrow: "Calcul distribué · un numérique plus durable",
    enabledBody: "Ce téléphone peut aider votre ordinateur lorsque le runtime l’autorise.",
    disabledBody: "La participation de ce poste est désactivée par votre préférence.",
    helper: "Participation autorisée par défaut", runtime: "Statut runtime", unavailable: "Indisponible",
    offline: "Hors ligne", worker: "Worker", connection: "Connexion", provider: "Provider",
    meaning: "Ce que cela signifie", advanced: "Détails avancés",
    advancedHint: "Portes de confiance, ressources distantes, preuves et diagnostics",
    local: "Local · hôte Linux", localHint: "État observé sur cette machine",
    remote: "Remote · worker Android", remoteHint: "Objet distant explicite et gouverné",
    gates: "Cinq portes indépendantes", evidence: "Preuves enregistrées",
    evidenceHint: "Historique inspectable, jamais présenté comme télémétrie LIVE",
    nav: ["Centre de contrôle", "Ressources", "Activité", "Preuves"],
    impact: "PETITS APPAREILS\nGRAND IMPACT", on: "ON", off: "OFF", active: "activé", inactive: "désactivé",
    cards: [
      ["Participation autorisée", "ON autorise PhoneBoost à proposer le worker. Il ne promet pas que le worker est prêt."],
      ["Les ressources restent distantes", "Le calcul reste sur le téléphone. RemoteBuffer n’est ni de la RAM locale, ni du swap."],
      ["Vous gardez le contrôle", "Passez sur OFF à tout moment. Les portes runtime restent indépendantes."],
    ],
  },
  en: {
    subtitle: "Linux host ↔ Android worker", eyebrow: "Distributed computing · a greener tomorrow",
    enabledBody: "This phone can help your computer when the runtime permits it.",
    disabledBody: "Participation from this host is disabled by your preference.",
    helper: "Participation enabled by default", runtime: "Runtime status", unavailable: "Unavailable",
    offline: "Offline", worker: "Worker", connection: "Connection", provider: "Provider",
    meaning: "What this means", advanced: "Advanced details",
    advancedHint: "Trust gates, remote resources, evidence and diagnostics",
    local: "Local · Linux host", localHint: "State observed on this machine",
    remote: "Remote · Android worker", remoteHint: "Explicit, governed remote object",
    gates: "Five independent gates", evidence: "Recorded evidence",
    evidenceHint: "Inspectable history, never presented as LIVE telemetry",
    nav: ["Control Center", "Resources", "Activity", "Evidence"],
    impact: "SMALL DEVICES\nBIG IMPACT", on: "ON", off: "OFF", active: "activated", inactive: "disabled",
    cards: [
      ["Participation is enabled", "ON allows PhoneBoost to offer the worker. It does not promise that the worker is ready."],
      ["Resources stay remote", "Computation stays on the phone. RemoteBuffer is neither local RAM nor swap."],
      ["You stay in control", "Switch OFF at any time. Runtime gates remain independent."],
    ],
  },
};

function readParticipation() {
  try { const value = window.localStorage.getItem(PARTICIPATION_KEY); return value === null ? true : value === "true"; }
  catch { return true; }
}

export function runtimeSummary(live, enabled) {
  if (!enabled) return { state: "DISABLED", ready: false };
  if (!live.fresh || !live.runtime) return { state: "OFFLINE", ready: false };
  const runtime = live.runtime;
  const ready = runtime.authenticated_session?.state === "AUTHENTICATED" &&
    runtime.controller_lease?.state === "ACTIVE" &&
    runtime.resource_guard_admission_proof?.state === "FRESH_PASS" &&
    runtime.remote_blake3_available === true &&
    runtime.provider_readiness?.state === "AVAILABLE" &&
    runtime.auto_use?.state === "AVAILABLE" && runtime.auto_use?.reason === "READY";
  return { state: ready ? "READY" : (runtime.auto_use?.state || "UNAVAILABLE"), ready };
}

export function liveBadgeTone(live) {
  return live.fresh ? "border-primary/30 bg-primary/10 text-primary" : "border-amber-500/30 bg-amber-500/10 text-amber-200";
}

function StatePill({ state, language, ready = false }) {
  const canonical = state || "UNAVAILABLE";
  const positive = ready || ["ACTIVE", "AVAILABLE", "AUTHENTICATED", "FRESH_PASS"].includes(canonical);
  return <span className={`inline-flex items-center gap-2 rounded-full border px-3 py-1.5 font-mono text-[10px] font-semibold uppercase tracking-[.14em] ${positive ? "border-primary/25 bg-primary/10 text-primary" : "border-white/10 bg-white/[.035] text-zinc-400"}`} title={canonical}><span className="h-1.5 w-1.5 rounded-full bg-current" />{stateLabel(canonical, language)}</span>;
}

function LanguageToggle({ language, setLanguage, t }) {
  return <div className="inline-flex rounded-xl border border-[#293542] bg-[#0d1319] p-1" aria-label={t.labels.languageSelector}>{["fr", "en"].map((choice) => <button key={choice} type="button" aria-pressed={language === choice} onClick={() => setLanguage(choice)} className={`rounded-lg px-4 py-2 text-sm font-bold uppercase transition ${language === choice ? "bg-primary text-black shadow-[0_0_18px_rgba(99,255,25,.2)]" : "text-zinc-500 hover:text-white"}`}>{choice}</button>)}</div>;
}

export function Sidebar({ active, language, setLanguage, t }) {
  const ui = UI_COPY[language] || UI_COPY.fr;
  const icons = [Home, Smartphone, ListChecks, Gauge];
  return <aside className="fixed inset-y-0 left-0 z-30 hidden w-[244px] flex-col border-r border-[#1a2530] bg-[#070b0f] px-3 py-7 lg:flex">
    <a href="#overview" className="flex items-center gap-3 px-3"><span className="grid h-12 w-12 place-items-center rounded-2xl bg-primary font-display text-xl font-black text-black shadow-[0_0_22px_rgba(99,255,25,.2)]">PB</span><span><b className="block text-lg text-white">PhoneBoost</b><small className="font-mono text-[8px] uppercase tracking-[.22em] text-zinc-600">Distributed computing</small></span></a>
    <nav className="mt-12 space-y-2" aria-label={t.labels.primaryNavigation}>{NAV.map((item, index) => { const Icon = icons[index]; return <a key={item} href={`#${item}`} className={`flex items-center gap-4 rounded-xl border-l-4 px-4 py-3 text-sm transition ${active === item ? "border-primary bg-[#10191e] text-primary" : "border-transparent text-zinc-500 hover:bg-white/[.03] hover:text-zinc-200"}`}><Icon size={19} />{ui.nav[index]}</a>; })}</nav>
    <div className="mt-auto px-4"><div className="mb-5 h-px w-10 bg-[#27333e]" /><p className="whitespace-pre-line font-mono text-[10px] leading-5 tracking-[.25em] text-zinc-600">{ui.impact}</p><span className="mt-4 block h-0.5 w-10 bg-primary" /></div>
    <div className="mt-6 px-3"><LanguageToggle language={language} setLanguage={setLanguage} t={t} /></div>
  </aside>;
}

function ParticipationSwitch({ enabled, setEnabled, ui }) {
  return <button type="button" role="switch" aria-checked={enabled} data-testid="participation-switch" onClick={() => setEnabled(!enabled)} className={`relative h-[76px] w-full max-w-[300px] rounded-full border p-1.5 transition duration-300 ${enabled ? "border-primary bg-primary shadow-[0_0_32px_rgba(99,255,25,.25)]" : "border-[#303b44] bg-[#151c22]"}`}><span className={`absolute inset-y-0 grid place-items-center text-xl font-black transition-all ${enabled ? "left-8 right-[82px] text-black" : "left-[82px] right-8 text-zinc-400"}`}>{enabled ? ui.on : ui.off}</span><span className={`absolute top-1.5 h-[62px] w-[62px] rounded-full border-2 bg-[#0a1115] transition-all duration-300 ${enabled ? "right-1.5 border-[#4cff00]" : "left-1.5 border-[#394650]"}`} /></button>;
}

function SummaryRow({ icon: Icon, label, children }) {
  return <div className="flex items-center gap-4 border-b border-white/[.06] py-3 last:border-0"><Icon className="shrink-0 text-zinc-500" size={22} /><div className="min-w-0"><p className="text-xs text-zinc-500">{label}</p><div className="mt-1 truncate text-sm font-semibold text-zinc-200">{children}</div></div></div>;
}

function Hero({ live, enabled, setEnabled, language, ui, t, setLanguage }) {
  const runtime = live.fresh ? live.runtime : null;
  const summary = runtimeSummary(live, enabled);
  return <section id="overview" className="pt-5"><div className="mb-6 flex flex-wrap items-center justify-between gap-4"><div><h1 className="font-display text-3xl font-semibold tracking-tight text-white sm:text-4xl">PhoneBoost Control Center</h1><p className="mt-1 text-zinc-500">{ui.subtitle}</p></div><div className="flex items-center gap-4"><div className="lg:hidden"><LanguageToggle language={language} setLanguage={setLanguage} t={t} /></div><div data-testid="live-status-badge" className={`inline-flex items-center gap-2 rounded-full border px-4 py-2 font-mono text-[10px] font-semibold uppercase tracking-[.16em] ${liveBadgeTone(live)}`}><Radio size={12} />{live.fresh ? "LIVE" : t.labels.liveUnavailable}</div></div></div>
    <div className="relative overflow-hidden rounded-2xl border border-[#24313b] bg-[#0a1015] p-7 sm:p-9"><div className="pointer-events-none absolute inset-y-0 right-0 w-2/5 opacity-40 [background:linear-gradient(145deg,transparent_25%,#17222a_25%,transparent_48%,#122018_48%,transparent_67%)]" /><div className="relative grid gap-8 xl:grid-cols-[1fr_300px]"><div className="grid gap-8 md:grid-cols-[1fr_300px] md:items-center"><div><p className="font-mono text-[10px] uppercase leading-5 tracking-[.28em] text-zinc-600">{ui.eyebrow}</p><span className="my-5 block h-0.5 w-10 bg-primary" /><h2 className="font-display text-5xl font-semibold leading-[.98] tracking-tight text-white sm:text-6xl">PhoneBoost<br /><span className="text-primary">{enabled ? ui.active : ui.inactive}</span></h2><p className="mt-5 max-w-xl text-lg leading-7 text-zinc-300">{enabled ? ui.enabledBody : ui.disabledBody}</p></div><div className="flex flex-col items-center gap-4"><ParticipationSwitch enabled={enabled} setEnabled={setEnabled} ui={ui} /><StatePill state={summary.state} language={language} ready={summary.ready} /><p className="flex items-center gap-2 text-sm text-zinc-500"><Info size={16} />{ui.helper}</p></div></div><div className="border-t border-[#1d2932] pt-3 xl:border-l xl:border-t-0 xl:pl-8 xl:pt-0"><SummaryRow icon={Server} label={ui.runtime}><StatePill state={summary.state} language={language} ready={summary.ready} /></SummaryRow><SummaryRow icon={Smartphone} label={ui.worker}>{runtime?.authenticated_session?.remote_worker_state || ui.unavailable}</SummaryRow><SummaryRow icon={Link2} label={ui.connection}>{runtime?.authenticated_session?.state || ui.offline}</SummaryRow><SummaryRow icon={Cpu} label={ui.provider}>{runtime?.provider_readiness?.state || ui.unavailable}</SummaryRow></div></div></div>
  </section>;
}

function MeaningCards({ ui }) {
  const icons = [CirclePower, Server, ShieldCheck];
  return <section className="py-7"><h2 className="mb-4 text-2xl font-semibold text-white">{ui.meaning}</h2><div className="grid gap-4 md:grid-cols-3">{ui.cards.map(([title, body], index) => { const Icon = icons[index]; return <article key={title} className="rounded-xl border border-[#202c35] bg-[#0a1015] p-5"><div className="flex gap-4"><Icon className="shrink-0 text-primary" size={28} /><div><h3 className="font-semibold text-zinc-100">{title}</h3><p className="mt-2 text-sm leading-6 text-zinc-500">{body}</p></div></div></article>; })}</div></section>;
}

function ResourcePanels({ live, language, ui }) {
  const runtime = live.fresh ? live.runtime : null;
  const panels = [[ui.local, ui.localHint, HardDrive, [["Daemon", runtime?.local_daemon?.runtime_state], ["API", runtime?.local_daemon?.local_api_state]]], [ui.remote, ui.remoteHint, Smartphone, [["Discovery", runtime?.discovery_observation?.state], ["Authentication", runtime?.authenticated_session?.state], [ui.provider, runtime?.provider_readiness?.state]]]];
  return <section id="resources" className="scroll-mt-6 py-5"><div className="grid gap-4 lg:grid-cols-2">{panels.map(([title, hint, Icon, values]) => <article key={title} className="rounded-xl border border-[#202c35] bg-[#0a1015] p-5"><div className="flex items-center gap-3"><span className="grid h-10 w-10 place-items-center rounded-lg bg-primary/10 text-primary"><Icon size={20} /></span><div><h3 className="font-semibold text-white">{title}</h3><p className="text-xs text-zinc-600">{hint}</p></div></div><div className="mt-5 grid gap-3 sm:grid-cols-2">{values.map(([label, value]) => <div className="rounded-lg border border-white/[.06] bg-black/20 p-3" key={label}><p className="text-xs text-zinc-600">{label}</p><div className="mt-2"><StatePill state={value || "UNAVAILABLE"} language={language} /></div></div>)}</div></article>)}</div></section>;
}

function GateLadder({ live, language, ui }) {
  const runtime = live.fresh ? live.runtime : null;
  const gates = [["paired", { state: "UNKNOWN", reason: "NOT_EXPOSED_BY_C12" }], ["authenticated", runtime?.authenticated_session], ["controller_lease", runtime?.controller_lease], ["resource_admissible", runtime?.resource_guard_admission_proof], ["provider_ready", runtime?.provider_readiness]];
  return <div><h3 className="text-lg font-semibold text-white">{ui.gates}</h3><div className="mt-4 grid gap-3 lg:grid-cols-5">{gates.map(([id, observed], index) => { const current = observed || { state: "UNAVAILABLE", reason: "LIVE_UNAVAILABLE" }; return <div key={id} className="rounded-xl border border-[#202c35] bg-black/20 p-4"><span className="font-mono text-[10px] text-zinc-700">0{index + 1}</span><p className="mt-2 min-h-10 text-sm font-semibold text-zinc-200">{GATE_COPY[id]?.[language] || id}</p><StatePill state={current.state} language={language} /><p className="mt-3 break-words font-mono text-[9px] leading-4 text-zinc-700">{current.reason || "OBSERVED"}</p></div>; })}</div></div>;
}

export function LiveControl({ live, compute, onCompute, language, t }) {
  const runtime = live.fresh ? live.runtime : null;
  return <div className="rounded-xl border border-[#202c35] bg-black/20 p-5"><div className="flex flex-wrap items-center justify-between gap-3"><div><p className="font-mono text-[10px] uppercase tracking-[.16em] text-zinc-600">{t.labels.lockedFixture}</p><h3 className="mt-1 font-semibold text-white">pb.native.blake3/1</h3></div><StatePill state={runtime?.remote_blake3_available ? "AVAILABLE" : "UNAVAILABLE"} language={language} /></div><p className="mt-3 text-sm text-zinc-500">{t.labels.fixedAction}</p><button className="mt-4 rounded-lg bg-primary px-4 py-2 text-sm font-bold text-black disabled:bg-zinc-800 disabled:text-zinc-500" type="button" onClick={onCompute} disabled={!runtime || compute.running} data-testid="run-blake3-control">{compute.running ? t.labels.running : t.labels.runFixture}</button>{!runtime && <p className="mt-3 text-xs text-amber-200">{t.labels.runtimeUnavailable}</p>}{compute.error && <p className="mt-2 text-xs text-amber-200">{t.labels.computeError}</p>}{compute.result && <p className="mt-3 break-all font-mono text-xs text-zinc-400">{compute.result.execution_source} · {compute.result.digest_blake3_hex}</p>}</div>;
}

export function EvidenceDrawer({ item, api, t, onClose }) {
  const [content, setContent] = useState(null); const closeButtonRef = useRef(null); const previouslyFocusedRef = useRef(null);
  useEffect(() => { setContent(null); if (!item) return undefined; previouslyFocusedRef.current = document.activeElement; closeButtonRef.current?.focus(); const onKeyDown = (event) => { if (event.key === "Escape") { event.preventDefault(); onClose(); } }; document.addEventListener("keydown", onKeyDown); return () => { document.removeEventListener("keydown", onKeyDown); previouslyFocusedRef.current?.focus?.(); }; }, [item, onClose]);
  useEffect(() => { let cancelled = false; if (!item || !api.base) return undefined; axios.get(`${api.base}/evidence/${item.id}`).then((response) => { if (!cancelled) setContent(response.data); }).catch(() => { if (!cancelled) setContent(null); }); return () => { cancelled = true; }; }, [api.base, item]);
  if (!item) return null;
  return <div className="fixed inset-0 z-50 flex justify-end bg-black/70 p-3 backdrop-blur-sm" role="dialog" aria-modal="true" aria-labelledby="evidence-drawer-title"><div className="h-full w-full max-w-xl overflow-y-auto rounded-2xl border border-[#24313b] bg-[#080d11] p-6"><div className="flex items-start justify-between gap-4"><div><p className="font-mono text-[10px] uppercase tracking-[.16em] text-primary">{t.evidence.kicker}</p><h3 id="evidence-drawer-title" className="mt-2 text-xl font-semibold text-white">{item.title}</h3></div><button ref={closeButtonRef} data-testid="evidence-drawer-close" aria-label={t.labels.close} className="rounded-lg p-2 text-zinc-400 hover:bg-white/10" onClick={onClose} type="button"><X size={18} /></button></div><p className="mt-5 text-sm leading-6 text-zinc-300">{item.summary}</p><p className="mt-5 font-mono text-xs text-zinc-500">{t.evidence.source}: {item.source}</p>{api.base && <pre className="mt-5 overflow-x-auto rounded-xl border border-white/10 bg-black/30 p-4 text-xs text-zinc-400">{content ? JSON.stringify(content, null, 2) : t.evidence.loading}</pre>}</div></div>;
}

function EvidenceList({ evidence, fixtures, api, t, ui }) {
  const [selected, setSelected] = useState(null);
  return <div id="evidence" className="scroll-mt-6 pt-6"><div className="mb-4 flex items-center justify-between"><div><h3 className="text-lg font-semibold text-white">{ui.evidence}</h3><p className="mt-1 text-sm text-zinc-600">{ui.evidenceHint}</p></div><span className="font-mono text-[10px] text-zinc-700">{fixtures.file_count ?? "—"} fixtures</span></div><div className="grid gap-3 md:grid-cols-2">{evidence.slice(0, 6).map((item) => <button key={item.id} type="button" onClick={() => setSelected(item)} className="flex items-center justify-between rounded-xl border border-[#202c35] bg-black/20 p-4 text-left hover:border-primary/30"><span className="flex min-w-0 items-center gap-3"><FileCheck2 className="shrink-0 text-primary" size={18} /><span className="truncate text-sm text-zinc-300">{item.title}</span></span><ChevronRight className="shrink-0 text-zinc-700" size={16} /></button>)}</div><EvidenceDrawer item={selected} api={api} t={t} onClose={() => setSelected(null)} /></div>;
}

export default function Dashboard({ api, snapshot, live, evidence, roadmap, arch, fixtures, compute, onCompute, language, setLanguage }) {
  const [enabled, setEnabledState] = useState(readParticipation); const t = copyFor(language); const ui = UI_COPY[language] || UI_COPY.fr;
  const setEnabled = (value) => { setEnabledState(value); try { window.localStorage.setItem(PARTICIPATION_KEY, String(value)); } catch { /* in-memory preference remains */ } };
  return <div className="min-h-screen bg-[#06090d] text-zinc-100"><Sidebar active="overview" language={language} setLanguage={setLanguage} t={t} /><main className="mx-auto max-w-[1440px] px-5 pb-12 lg:ml-[244px] lg:px-8"><Hero live={live} enabled={enabled} setEnabled={setEnabled} language={language} setLanguage={setLanguage} ui={ui} t={t} /><MeaningCards ui={ui} /><ResourcePanels live={live} language={language} ui={ui} /><section id="activity" className="scroll-mt-6 py-5"><details data-testid="advanced-details" className="group rounded-xl border border-[#202c35] bg-[#0a1015] p-5"><summary className="flex cursor-pointer list-none items-center justify-between"><span className="flex items-center gap-4"><span className="grid h-11 w-11 place-items-center rounded-lg bg-white/[.04] text-zinc-400"><Activity size={22} /></span><span><b className="block text-white">{ui.advanced}</b><small className="text-zinc-600">{ui.advancedHint}</small></span></span><ChevronDown className="text-zinc-500 transition group-open:rotate-180" /></summary><div className="mt-6 space-y-6 border-t border-white/[.06] pt-6"><GateLadder live={live} snapshot={snapshot} language={language} ui={ui} /><LiveControl live={live} compute={compute} onCompute={onCompute} language={language} t={t} /><EvidenceList evidence={evidence} fixtures={fixtures} api={api} t={t} ui={ui} /><div className="grid gap-3 md:grid-cols-2"><div className="rounded-xl border border-[#202c35] bg-black/20 p-4 text-sm text-zinc-400">{(arch.layers || []).length} architecture layers · explicit remote boundary</div><div className="rounded-xl border border-[#202c35] bg-black/20 p-4 text-sm text-zinc-400">{(roadmap.next || []).length} roadmap items · separate from proven behavior</div></div></div></details></section><footer className="mt-5 border-t border-[#18222b] py-7 text-xs leading-5 text-zinc-700">{t.footer}</footer></main></div>;
}
