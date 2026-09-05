import { useEffect, useRef, useState } from "react";
import axios from "axios";
import {
  Cpu,
  Smartphone,
  ShieldCheck,
  ChevronRight,
  ChevronDown,
  Github,
  GitBranch,
  X,
  ExternalLink,
  Radio,
  Copy,
  Check,
  Layers,
  ListChecks,
  FileCheck2,
  Route,
  LayoutGrid,
  Terminal,
  Power,
  PlugZap,
  Boxes,
  Gauge,
  Recycle,
  Leaf,
  ArrowRight,
  Ban,
} from "lucide-react";
import { STRINGS, GATE_CONTENT, stateLabel } from "../i18n";

const NAV = [
  { id: "overview", icon: LayoutGrid },
  { id: "why", icon: Recycle },
  { id: "how", icon: Route },
  { id: "ladder", icon: ListChecks },
  { id: "evidence", icon: FileCheck2 },
  { id: "roadmap", icon: Layers },
];

/* ------------------------------------------------------------------ */
/* Primitives                                                          */
/* ------------------------------------------------------------------ */

function Pill({ children, tone = "unavailable", dot = true, testId, className = "", title }) {
  const tones = {
    unavailable: "border-neutral-700/70 bg-neutral-800/40 text-neutral-400",
    warn: "border-amber-500/30 bg-amber-500/10 text-amber-300",
    evidence: "border-[#39FF14]/40 bg-[#39FF14]/10 text-[#8dff77]",
    roadmap: "border-neutral-700/60 bg-transparent text-neutral-500 border-dashed",
    neutral: "border-neutral-700/70 bg-neutral-800/40 text-neutral-300",
  };
  const dots = {
    unavailable: "bg-neutral-500",
    warn: "bg-amber-400",
    evidence: "bg-[#39FF14] dot-glow-green",
    roadmap: "bg-neutral-600",
    neutral: "bg-neutral-400",
  };
  return (
    <span
      data-testid={testId}
      title={title}
      className={`inline-flex items-center gap-1.5 rounded border px-2.5 py-1 font-mono text-[10px] font-medium uppercase tracking-[0.14em] ${tones[tone]} ${className}`}
    >
      {dot && <span className={`h-1.5 w-1.5 rounded-full ${dots[tone]}`} />}
      {children}
    </span>
  );
}

// Shows a friendly bilingual label; keeps the canonical runtime state on hover.
function StateChip({ state, lang, testId }) {
  const s = (state || "").toUpperCase();
  const isRoadmap = s === "ROADMAP";
  return (
    <Pill
      tone={isRoadmap ? "roadmap" : "unavailable"}
      dot={!isRoadmap}
      testId={testId}
      title={state}
      className="max-w-full truncate"
    >
      <span className="truncate">{stateLabel(state, lang)}</span>
    </Pill>
  );
}

function Card({ children, className = "", testId }) {
  return (
    <div data-testid={testId} className={`rounded-lg border border-neutral-800 bg-[#111111] bg-grain ${className}`}>
      {children}
    </div>
  );
}

function SectionHeading({ index, kicker, title, sub, right }) {
  return (
    <div className="mb-6 flex flex-wrap items-end justify-between gap-4">
      <div>
        <div className="mb-2 flex items-center gap-2.5">
          <span className="font-mono text-[11px] font-semibold tracking-[0.2em] text-[#39FF14]/70">{index}</span>
          <span className="h-px w-8 bg-neutral-700" />
          <span className="font-mono text-[10px] uppercase tracking-[0.24em] text-neutral-500">{kicker}</span>
        </div>
        <h2 className="font-display text-2xl font-bold tracking-tight text-white sm:text-[1.7rem]">{title}</h2>
        {sub && <p className="mt-1.5 max-w-2xl text-sm text-neutral-400">{sub}</p>}
      </div>
      {right}
    </div>
  );
}

function KV({ k, v, lang, testId }) {
  return (
    <div className="flex items-center justify-between gap-4 border-t border-neutral-800/70 py-2.5 first:border-t-0" data-testid={testId}>
      <span className="shrink-0 text-xs text-neutral-500">{k}</span>
      <StateChip state={v} lang={lang} />
    </div>
  );
}

function Collapsible({ title, icon: Icon, testId, right, defaultOpen = false, children }) {
  const [open, setOpen] = useState(defaultOpen);
  return (
    <Card className="overflow-hidden">
      <button
        data-testid={testId}
        onClick={() => setOpen((o) => !o)}
        className="flex w-full items-center justify-between gap-3 px-5 py-4 text-left transition-colors hover:bg-white/[0.02]"
      >
        <div className="flex items-center gap-2.5">
          {Icon && <Icon className="h-4 w-4 text-neutral-500" strokeWidth={1.75} />}
          <span className="font-display text-sm font-semibold text-white">{title}</span>
          {right}
        </div>
        <ChevronDown className={`h-4 w-4 shrink-0 text-neutral-500 transition-transform ${open ? "rotate-180" : ""}`} />
      </button>
      {open && <div className="border-t border-neutral-800 px-5 py-4">{children}</div>}
    </Card>
  );
}

function LanguageToggle({ lang, setLang, testId, prefix }) {
  return (
    <div className="inline-flex rounded-md border border-neutral-800 bg-neutral-900/60 p-0.5" data-testid={testId}>
      {["fr", "en"].map((l) => (
        <button
          key={l}
          data-testid={`${prefix}-lang-${l}`}
          onClick={() => setLang(l)}
          className={`rounded px-2.5 py-1 font-mono text-[10px] font-semibold uppercase tracking-[0.14em] transition-colors ${
            lang === l ? "bg-[#39FF14]/10 text-[#8dff77]" : "text-neutral-500 hover:text-neutral-300"
          }`}
        >
          {l}
        </button>
      ))}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Sidebar                                                             */
/* ------------------------------------------------------------------ */

function Sidebar({ active, snapshot, onCopy, copied, t, lang, setLang }) {
  return (
    <aside className="fixed inset-y-0 left-0 z-30 hidden w-60 flex-col border-r border-neutral-800 bg-[#0a0a0a] bg-grain lg:flex">
      <div className="flex items-center gap-3 border-b border-neutral-800 px-5 py-5">
        <div className="flex h-9 w-9 items-center justify-center rounded border border-[#39FF14]/40 bg-[#39FF14]/5 font-display text-sm font-black text-[#39FF14] text-glow-green">
          PB
        </div>
        <div className="leading-none">
          <div className="font-display text-sm font-bold tracking-wide text-white">PhoneBoost</div>
          <div className="mt-1 font-mono text-[9px] uppercase tracking-[0.2em] text-neutral-500">{t.brand_sub}</div>
        </div>
      </div>

      <div className="border-b border-neutral-800 px-4 py-3">
        <LanguageToggle lang={lang} setLang={setLang} testId="side-lang-toggle" prefix="side" />
      </div>

      <nav className="flex-1 px-3 py-5" data-testid="side-nav">
        <ul className="space-y-1">
          {NAV.map((n) => {
            const on = active === n.id;
            const Icon = n.icon;
            return (
              <li key={n.id}>
                <a
                  href={`#${n.id}`}
                  data-testid={`nav-${n.id}`}
                  className={`group relative flex items-center gap-3 rounded-md px-3 py-2 text-[13px] transition-colors ${
                    on ? "bg-neutral-800/60 text-white" : "text-neutral-500 hover:bg-neutral-800/30 hover:text-neutral-300"
                  }`}
                >
                  <span className={`absolute left-0 top-1/2 h-4 w-0.5 -translate-y-1/2 rounded-full transition-all ${on ? "bg-[#39FF14] dot-glow-green" : "bg-transparent"}`} />
                  <Icon className={`h-4 w-4 shrink-0 ${on ? "text-[#39FF14]" : "text-neutral-600 group-hover:text-neutral-400"}`} strokeWidth={1.75} />
                  {t.nav[n.id]}
                </a>
              </li>
            );
          })}
        </ul>
      </nav>

      <div className="border-t border-neutral-800 px-4 py-4">
        <div className="mb-2 font-mono text-[9px] uppercase tracking-[0.2em] text-neutral-600">{t.footer_master}</div>
        <div className="space-y-1 font-mono text-[10px] text-neutral-500">
          <div className="truncate">tag <span className="text-neutral-300">{snapshot.release.tag}</span></div>
          <div className="truncate">head <span className="text-neutral-300">{snapshot.release.head.slice(0, 10)}</span></div>
        </div>
        <button
          data-testid="copy-repo-anchor"
          onClick={onCopy}
          className="mt-3 inline-flex w-full items-center justify-center gap-1.5 rounded border border-neutral-800 bg-neutral-900/60 px-2 py-1.5 font-mono text-[10px] uppercase tracking-[0.14em] text-neutral-400 transition-colors hover:border-[#39FF14]/40 hover:text-[#8dff77]"
        >
          {copied ? <Check className="h-3 w-3 text-[#39FF14]" /> : <Copy className="h-3 w-3" />}
          {copied ? "OK" : "Copy anchor"}
        </button>
      </div>
    </aside>
  );
}

/* ------------------------------------------------------------------ */
/* Hero (business-first)                                               */
/* ------------------------------------------------------------------ */

function Hero({ t }) {
  return (
    <section id="overview" className="reveal relative overflow-hidden border-b border-neutral-800">
      <div className="pointer-events-none absolute -right-24 -top-24 h-72 w-72 rounded-full bg-[#39FF14]/5 blur-3xl" />
      <div className="relative px-6 py-12 sm:px-10 sm:py-16">
        <div className="max-w-3xl">
          <div className="mb-4 flex items-center gap-2.5">
            <span className="h-1.5 w-1.5 rounded-full bg-[#39FF14] dot-glow-green" />
            <span className="font-mono text-[10px] uppercase tracking-[0.28em] text-neutral-500">{t.hero_kicker}</span>
          </div>
          <div className="font-display text-4xl font-black tracking-tighter text-white sm:text-5xl">PhoneBoost</div>
          <h1 className="mt-4 max-w-2xl font-display text-3xl font-bold leading-tight tracking-tight text-white sm:text-[2.6rem] sm:leading-[1.1]">
            {t.hero_headline_a} <span className="text-[#39FF14] text-glow-green">{t.hero_headline_b}</span>
          </h1>
          <p className="mt-5 max-w-2xl text-[15px] leading-relaxed text-neutral-300">{t.hero_para}</p>
          <p className="mt-3 max-w-2xl font-display text-base font-medium text-neutral-200">{t.hero_support}</p>

          <div className="mt-8 flex flex-wrap items-center gap-3">
            <a
              href="#why"
              data-testid="cta-primary"
              className="inline-flex items-center gap-2 rounded-md border border-[#39FF14]/40 bg-[#39FF14]/10 px-5 py-2.5 font-display text-sm font-semibold text-[#c7ffb8] transition-colors hover:bg-[#39FF14]/20"
            >
              {t.cta_primary} <ArrowRight className="h-4 w-4" strokeWidth={2} />
            </a>
            <a
              href="#how"
              data-testid="cta-secondary"
              className="inline-flex items-center gap-2 rounded-md border border-neutral-800 bg-neutral-900/60 px-5 py-2.5 font-display text-sm font-medium text-neutral-300 transition-colors hover:border-neutral-600 hover:text-white"
            >
              {t.cta_secondary}
            </a>
          </div>

          <div className="mt-8 flex flex-wrap items-center gap-2.5" data-testid="hero-status-chips">
            <Pill tone="warn" testId="chip-live">{t.chip_live}</Pill>
            <Pill tone="evidence" testId="chip-evidence">{t.chip_evidence}</Pill>
          </div>

          <p className="mt-6 max-w-2xl text-xs leading-relaxed text-neutral-500">{t.audience_note}</p>
        </div>
      </div>
    </section>
  );
}

/* ------------------------------------------------------------------ */
/* Why (business value)                                                */
/* ------------------------------------------------------------------ */

function Why({ t }) {
  const icons = [Recycle, Gauge, Smartphone, ListChecks, ShieldCheck, ShieldCheck];
  return (
    <section id="why" className="reveal border-t border-neutral-800 px-6 py-12 sm:px-10">
      <SectionHeading index="01" kicker={t.why_kicker} title={t.why_title} sub={t.why_problem} />
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-3">
        {t.why_points.map((p, i) => {
          const Icon = icons[i] || Recycle;
          return (
            <Card key={p.title} className="p-6" testId={`why-point-${i}`}>
              <div className="mb-3 flex h-9 w-9 items-center justify-center rounded-lg border border-neutral-800 bg-neutral-900">
                <Icon className="h-4 w-4 text-[#8dff77]" strokeWidth={1.75} />
              </div>
              <div className="font-display text-[15px] font-semibold leading-snug text-white">{p.title}</div>
              <p className="mt-2 text-xs leading-relaxed text-neutral-400">{p.desc}</p>
            </Card>
          );
        })}
      </div>
      <div className="mt-6 flex items-start gap-3 rounded-lg border border-[#39FF14]/20 bg-[#39FF14]/[0.04] p-5" data-testid="why-env">
        <Leaf className="mt-0.5 h-5 w-5 shrink-0 text-[#39FF14]" strokeWidth={1.75} />
        <p className="text-sm leading-relaxed text-neutral-200">{t.why_env}</p>
      </div>
    </section>
  );
}

/* ------------------------------------------------------------------ */
/* Enable control (truthful — cannot fake LIVE)                        */
/* ------------------------------------------------------------------ */

function EnableControl({ live, expanded, onToggleWhy, t }) {
  return (
    <div className="w-full rounded-xl border border-amber-500/25 bg-[#111111] bg-grain p-5" data-testid="enable-phoneboost-panel">
      <div className="flex items-center justify-between gap-4">
        <div className="flex items-center gap-3">
          <div className="flex h-10 w-10 items-center justify-center rounded-lg border border-amber-500/30 bg-amber-500/5">
            <Power className="h-5 w-5 text-amber-400" strokeWidth={1.75} />
          </div>
          <div className="leading-tight">
            <div className="font-display text-sm font-semibold text-white">{t.enable_title}</div>
            <div className="font-mono text-[10px] uppercase tracking-[0.18em] text-amber-300/80">{t.enable_status}</div>
          </div>
        </div>
        <button
          data-testid="enable-phoneboost-control"
          disabled
          aria-disabled="true"
          title={t.enable_reason}
          className="relative h-7 w-12 cursor-not-allowed rounded-full border border-neutral-700 bg-neutral-800/80"
        >
          <span className="absolute left-1 top-1/2 h-5 w-5 -translate-y-1/2 rounded-full bg-neutral-500" />
        </button>
      </div>
      <p className="mt-3 text-xs leading-relaxed text-amber-100/80">{t.enable_reason}</p>
      <button
        data-testid="enable-why-toggle"
        onClick={onToggleWhy}
        className="mt-2 inline-flex items-center gap-1 font-mono text-[10px] uppercase tracking-[0.14em] text-neutral-400 transition-colors hover:text-amber-200"
      >
        {expanded ? t.enable_hide : t.enable_why}
        <ChevronDown className={`h-3 w-3 transition-transform ${expanded ? "rotate-180" : ""}`} />
      </button>
      {expanded && (
        <div data-testid="live-curtain" className="mt-3 border-t border-neutral-800 pt-3">
          <p className="text-xs leading-relaxed text-amber-100/70">{live.reason}</p>
          <ul className="mt-2 space-y-1.5">
            {(live.requirements || []).map((r) => (
              <li key={r} className="flex gap-2 text-[11px] text-amber-100/60">
                <span className="mt-1.5 h-1 w-1 shrink-0 rounded-full bg-amber-400/60" />
                {r}
              </li>
            ))}
          </ul>
          <p className="mt-3 font-mono text-[9px] uppercase tracking-[0.16em] text-neutral-600">{t.enable_no_fake}</p>
        </div>
      )}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Topology + glance + capabilities + limits (How it works)           */
/* ------------------------------------------------------------------ */

function TopologyNode({ icon: Icon, tag, tagTone, title, subtitle, state, lang }) {
  const tones = { local: "border-sky-500/25", remote: "border-neutral-700" };
  const chips = {
    local: "text-sky-300 border-sky-500/30 bg-sky-500/5",
    remote: "text-neutral-300 border-neutral-700 bg-neutral-800/40",
  };
  return (
    <div className={`flex items-center justify-between gap-4 rounded-lg border ${tones[tagTone]} bg-[#111111] bg-grain p-5`}>
      <div className="flex items-center gap-3">
        <div className="flex h-10 w-10 items-center justify-center rounded-lg border border-neutral-800 bg-neutral-900">
          <Icon className="h-5 w-5 text-neutral-200" strokeWidth={1.75} />
        </div>
        <div>
          <div className="flex items-center gap-2">
            <span className="font-display text-[15px] font-semibold text-white">{title}</span>
            <span className={`rounded border px-2 py-0.5 font-mono text-[9px] uppercase tracking-[0.16em] ${chips[tagTone]}`}>{tag}</span>
          </div>
          <div className="mt-0.5 font-mono text-[10px] uppercase tracking-[0.16em] text-neutral-500">{subtitle}</div>
        </div>
      </div>
      <StateChip state={state} lang={lang} />
    </div>
  );
}

function VConnector({ icon: Icon }) {
  return (
    <div className="flex items-stretch gap-4 py-1 pl-5">
      <div className="flex w-10 flex-col items-center">
        <span className="h-4 w-px bg-neutral-700" />
        <div className="flex h-8 w-8 items-center justify-center rounded-full border border-amber-500/30 bg-amber-500/5">
          <Icon className="h-4 w-4 text-amber-400/80" strokeWidth={1.75} />
        </div>
        <span className="h-4 w-px bg-neutral-700" />
      </div>
      <div className="flex-1" />
    </div>
  );
}

function How({ snapshot, live, whyOpen, onToggleWhy, t, lang }) {
  const g = t.glance;
  return (
    <section id="how" className="reveal border-t border-neutral-800 px-6 py-12 sm:px-10">
      <SectionHeading index="02" kicker={t.how_kicker} title={t.how_title} sub={t.how_intro} />

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-[1.3fr_1fr]">
        {/* Topology */}
        <div>
          <div className="mb-4 font-mono text-[10px] uppercase tracking-[0.24em] text-neutral-500">{t.connects_label}</div>
          <TopologyNode icon={Cpu} tag="Local" tagTone="local" title={t.how_node_local} subtitle={t.how_node_local_sub} state={snapshot.computer.runtime.state} lang={lang} />
          <div className="flex items-stretch gap-4 py-1 pl-5">
            <div className="flex w-10 flex-col items-center">
              <span className="h-4 w-px bg-neutral-700" />
              <div className="flex h-8 w-8 items-center justify-center rounded-full border border-amber-500/30 bg-amber-500/5">
                <Radio className="h-4 w-4 text-amber-400/80" strokeWidth={1.75} />
              </div>
              <span className="h-4 w-px bg-neutral-700" />
            </div>
            <div className="flex flex-1 items-center justify-between gap-3 rounded-lg border border-amber-500/20 bg-amber-500/[0.03] px-4 py-2.5">
              <div>
                <div className="font-display text-[13px] font-medium text-amber-100">{t.how_link}</div>
                <div className="font-mono text-[9px] uppercase tracking-[0.16em] text-neutral-500">{t.how_link_sub}</div>
              </div>
              <StateChip state={snapshot.secure_link.session.state} lang={lang} />
            </div>
          </div>
          <TopologyNode icon={Smartphone} tag="Remote" tagTone="remote" title={t.how_node_remote} subtitle={t.how_node_remote_sub} state={snapshot.phone.endpoint.state} lang={lang} />
          <VConnector icon={ChevronDown} />
          <div className="rounded-lg border border-[#39FF14]/20 bg-[#39FF14]/[0.03] p-5">
            <div className="flex items-center gap-2">
              <ShieldCheck className="h-4 w-4 text-[#39FF14]" strokeWidth={1.75} />
              <span className="font-display text-[13px] font-semibold text-white">{t.how_decision_title}</span>
            </div>
            <p className="mt-2 text-xs leading-relaxed text-neutral-400">{t.how_decision_sub}</p>
          </div>
          <p className="mt-4 text-[11px] leading-relaxed text-neutral-500">{t.how_separation}</p>
        </div>

        {/* Live status (Enable) */}
        <div className="flex flex-col gap-4">
          <EnableControl live={live} expanded={whyOpen} onToggleWhy={onToggleWhy} t={t} />
          {/* What PhoneBoost does NOT do */}
          <Card className="p-6" testId="limits-card">
            <div className="mb-1 font-mono text-[9px] uppercase tracking-[0.22em] text-neutral-500">{t.limits_kicker}</div>
            <h3 className="font-display text-base font-semibold text-white">{t.limits_title}</h3>
            <p className="mt-1 text-xs text-neutral-500">{t.limits_sub}</p>
            <ul className="mt-4 space-y-2.5">
              {t.limits.map((l) => (
                <li key={l} className="flex items-start gap-2.5 text-[13px] leading-relaxed text-neutral-300">
                  <Ban className="mt-0.5 h-4 w-4 shrink-0 text-amber-400/80" strokeWidth={1.75} />
                  {l}
                </li>
              ))}
            </ul>
          </Card>
        </div>
      </div>

      {/* At a glance */}
      <div className="mt-10">
        <div className="mb-4 font-mono text-[10px] uppercase tracking-[0.24em] text-neutral-500">{t.glance_label}</div>
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
          <GlanceCard testId="glance-availability" icon={Gauge} label={g.availability.label} value={g.availability.value} tone="warn" note={g.availability.note} />
          <GlanceCard testId="glance-phone" icon={Smartphone} label={g.phone.label} value={g.phone.value} note={g.phone.note} />
          <GlanceCard testId="glance-trust" icon={ShieldCheck} label={g.trust.label} value={g.trust.value} href="#ladder" note={g.trust.note} />
          <GlanceCard testId="glance-evidence" icon={FileCheck2} label={g.evidence.label} value={g.evidence.value} tone="evidence" href="#evidence" note={g.evidence.note} />
        </div>
      </div>

      {/* Capabilities */}
      <div className="mt-10">
        <div className="mb-4 font-mono text-[10px] uppercase tracking-[0.24em] text-neutral-500">{t.capabilities_label}</div>
        <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
          {t.capabilities.map((c, i) => {
            const Icon = [Radio, Boxes, Cpu, FileCheck2][i] || Radio;
            return (
              <Card key={c.title} className="p-5" testId={`capability-${i}`}>
                <div className="mb-3 flex items-center justify-between">
                  <div className="flex h-9 w-9 items-center justify-center rounded-lg border border-neutral-800 bg-neutral-900">
                    <Icon className="h-4 w-4 text-neutral-300" strokeWidth={1.75} />
                  </div>
                  <Pill tone={c.tone} dot={c.tone !== "roadmap"}>{c.status}</Pill>
                </div>
                <div className="font-display text-[15px] font-semibold text-white">{c.title}</div>
                <p className="mt-1.5 text-xs leading-relaxed text-neutral-400">{c.desc}</p>
              </Card>
            );
          })}
        </div>
      </div>
    </section>
  );
}

function GlanceCard({ icon: Icon, label, value, tone = "unavailable", note, href, testId }) {
  const Wrapper = href ? "a" : "div";
  return (
    <Wrapper
      href={href}
      data-testid={testId}
      className={`flex flex-col rounded-lg border border-neutral-800 bg-[#111111] bg-grain p-5 ${href ? "transition-colors hover:border-[#39FF14]/40" : ""}`}
    >
      <div className="mb-3 flex items-center gap-2 text-neutral-500">
        <Icon className="h-4 w-4" strokeWidth={1.75} />
        <span className="font-mono text-[9px] uppercase tracking-[0.18em]">{label}</span>
      </div>
      <div className="mb-2">
        <Pill tone={tone} dot={tone !== "roadmap"}>{value}</Pill>
      </div>
      {note && <p className="mt-auto text-[11px] leading-relaxed text-neutral-500">{note}</p>}
    </Wrapper>
  );
}

/* ------------------------------------------------------------------ */
/* Five-gate ladder                                                    */
/* ------------------------------------------------------------------ */

function GateLadder({ gates, t, lang }) {
  return (
    <section id="ladder" className="reveal border-t border-neutral-800 px-6 py-12 sm:px-10">
      <SectionHeading
        index="03"
        kicker={t.ladder_kicker}
        title={t.ladder_title}
        sub={t.ladder_sub}
        right={<Pill tone="warn" testId="ladder-status">{t.ladder_status}</Pill>}
      />
      <ol data-testid="gate-ladder" className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
        {gates.map((gate, i) => {
          const content = GATE_CONTENT[gate.id];
          const name = content ? content.name[lang] : gate.label;
          const explanation = content ? content.explanation[lang] : gate.explanation;
          return (
            <li key={gate.id} data-testid={`gate-${gate.id}`} className="group flex flex-col rounded-lg border border-neutral-800 bg-[#111111] bg-grain p-5 transition-colors hover:border-neutral-700">
              <div className="mb-3 flex items-center justify-between">
                <span className="font-mono text-lg font-semibold tracking-tight text-neutral-700">{String(i + 1).padStart(2, "0")}</span>
                <Pill tone="unavailable" testId={`gate-${gate.id}-badge`} title={gate.state}>{stateLabel(gate.state, lang)}</Pill>
              </div>
              <div className="mb-2 flex items-center gap-2">
                <span className="h-2 w-2 rounded-full border border-neutral-600 bg-transparent" />
                <span className="font-display text-[15px] font-semibold text-white">{name}</span>
              </div>
              <p className="text-xs leading-relaxed text-neutral-400">{explanation}</p>
              <p className="mt-3 border-t border-neutral-800/70 pt-2.5 font-mono text-[10px] italic text-neutral-600">{t.gate_reason}</p>
            </li>
          );
        })}
      </ol>
    </section>
  );
}

/* ------------------------------------------------------------------ */
/* Evidence + technical details                                        */
/* ------------------------------------------------------------------ */

function EvidenceSection({ evidence, fixtures, snapshot, arch, onOpen, t, lang }) {
  const sl = snapshot.secure_link;
  const c = snapshot.computer;
  const p = snapshot.phone;
  return (
    <section id="evidence" className="reveal border-t border-neutral-800 px-6 py-12 sm:px-10">
      <SectionHeading
        index="04"
        kicker={t.evidence_kicker}
        title={t.evidence_title}
        sub={t.evidence_sub}
        right={
          <div className="hidden items-center gap-4 font-mono text-[10px] uppercase tracking-[0.14em] text-neutral-500 md:flex">
            <span className="inline-flex items-center gap-1.5"><GitBranch className="h-3.5 w-3.5" /> {snapshot.release.head.slice(0, 10)}</span>
            {fixtures && <span>{t.evidence_fixtures} <span className="text-neutral-300">{fixtures.file_count}</span></span>}
          </div>
        }
      />
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
        {evidence.map((card) => (
          <button
            key={card.id}
            data-testid={`evidence-card-${card.id}`}
            onClick={() => onOpen(card.id)}
            className="ring-glow-green group relative flex flex-col overflow-hidden rounded-lg border border-neutral-800 bg-[#111111] bg-grain p-5 text-left transition-colors hover:border-[#39FF14]/40"
          >
            <span className="absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-[#39FF14]/40 to-transparent opacity-0 transition-opacity group-hover:opacity-100" />
            <div className="mb-3 flex items-center justify-between">
              <Pill tone="evidence">{t.evidence_recorded}</Pill>
              <span className="font-mono text-[9px] uppercase tracking-[0.16em] text-neutral-600">{card.kind}</span>
            </div>
            <h3 className="font-display text-[15px] font-semibold leading-snug text-white">{card.title}</h3>
            <p className="mt-2 flex-1 text-xs leading-relaxed text-neutral-400">{card.summary}</p>
            <div className="mt-4 flex items-center justify-between gap-3 border-t border-neutral-800/70 pt-3">
              <span className="truncate font-mono text-[10px] text-neutral-600">{card.source}</span>
              <span className="inline-flex items-center gap-1 font-mono text-[10px] uppercase tracking-[0.14em] text-neutral-500 transition-colors group-hover:text-[#8dff77]">
                {t.evidence_open} <ChevronRight className="h-3 w-3" />
              </span>
            </div>
          </button>
        ))}
      </div>

      {/* Technical details */}
      <div className="mt-10">
        <div className="mb-4 font-mono text-[10px] uppercase tracking-[0.24em] text-neutral-500">{t.tech_label}</div>
        <div className="space-y-4">
          <Collapsible title={t.tech_secure_link} icon={Radio} testId="collapsible-secure-link">
            <KV k="Transport" v={sl.transport.state} lang={lang} testId="kv-transport" />
            <KV k="Session" v={sl.session.state} lang={lang} testId="kv-session" />
            <KV k="Auth" v={sl.authentication.state} lang={lang} testId="kv-auth" />
            <KV k="Latency" v={sl.latency.state} lang={lang} testId="kv-latency" />
          </Collapsible>

          <Collapsible title={t.tech_endpoints} icon={Terminal} testId="collapsible-endpoints">
            <div className="grid grid-cols-1 gap-6 md:grid-cols-2">
              <div>
                <div className="mb-2 flex items-center gap-2">
                  <Cpu className="h-4 w-4 text-sky-300" strokeWidth={1.75} />
                  <span className="font-display text-[13px] font-semibold text-white">{c.label}</span>
                </div>
                <KV k="phoneboostd" v={c.runtime.state} lang={lang} testId="kv-phoneboostd" />
                <KV k="Local API" v={c.local_api.state} lang={lang} testId="kv-local-api" />
                <p className="mt-2 text-[11px] text-neutral-500">{c.note}</p>
              </div>
              <div>
                <div className="mb-2 flex items-center gap-2">
                  <Smartphone className="h-4 w-4 text-neutral-300" strokeWidth={1.75} />
                  <span className="font-display text-[13px] font-semibold text-white">{p.label}</span>
                </div>
                <KV k="Endpoint" v={p.endpoint.state} lang={lang} testId="kv-endpoint" />
                <KV k="Worker" v={p.worker.state} lang={lang} testId="kv-worker" />
                <KV k="Incarnation" v={p.incarnation.state} lang={lang} testId="kv-incarnation" />
                <p className="mt-2 text-[11px] text-neutral-500">{p.health.note}</p>
              </div>
            </div>
          </Collapsible>

          <Collapsible title={t.tech_remote_capability} icon={Boxes} testId="collapsible-remote-capability">
            <KV k="Admitted capacity" v={snapshot.remote_capability.admitted_capacity.state} lang={lang} testId="kv-admitted" />
            <KV k="Reserved" v={snapshot.remote_capability.reserved.state} lang={lang} testId="kv-reserved" />
            <KV k="RemoteBuffer" v={snapshot.remote_capability.active_remote_buffer.state} lang={lang} testId="kv-remotebuffer" />
            <KV k="Remote job" v={snapshot.remote_capability.active_remote_job.state} lang={lang} testId="kv-remote-job" />
            <p className="mt-2 text-[11px] text-neutral-500">{snapshot.remote_capability.note}</p>
          </Collapsible>

          <Collapsible title={t.tech_architecture} icon={Layers} testId="collapsible-architecture" right={<Pill tone="evidence">{t.tech_repo_truth}</Pill>}>
            <ol className="space-y-2">
              {arch.layers.map((l, i) => (
                <li key={l.layer} data-testid={`arch-${i}`} className="grid grid-cols-1 items-start gap-2 rounded border border-neutral-800/70 bg-neutral-900/30 p-3 md:grid-cols-[150px_120px_1fr]">
                  <div className="font-display text-[13px] font-medium text-neutral-100">{l.layer}</div>
                  <div className="font-mono text-[10px] uppercase tracking-[0.14em] text-neutral-500">{l.role}</div>
                  <div className="break-all font-mono text-[11px] text-neutral-400">{l.detail}</div>
                </li>
              ))}
            </ol>
          </Collapsible>

          <Collapsible title={t.tech_security} icon={ShieldCheck} testId="collapsible-security" defaultOpen>
            <div className="flex items-start gap-3">
              <ShieldCheck className="mt-0.5 h-5 w-5 shrink-0 text-[#39FF14]" strokeWidth={1.75} />
              <div>
                <p className="text-sm leading-relaxed text-neutral-200">{snapshot.security_plain_language}</p>
                <p className="mt-2 break-words font-mono text-[11px] leading-relaxed text-neutral-500">
                  Full 256-bit peer IDs (<span className="text-neutral-300">SHA-256(static_public_key)</span>), Noise XX
                  first pair with QR-01A SAS, Noise IK reconnect, PBMUX authenticated framing, fail-closed dispatch,
                  worker-authoritative ResourceGuard.
                </p>
              </div>
            </div>
          </Collapsible>
        </div>
      </div>
    </section>
  );
}

function EvidenceDrawer({ open, onClose, api, id, t }) {
  const [detail, setDetail] = useState(null);
  const [loading, setLoading] = useState(false);
  const loadedFor = useRef(null);

  useEffect(() => {
    if (open && id && loadedFor.current !== id) {
      loadedFor.current = id;
      setLoading(true);
      setDetail(null);
      axios
        .get(`${api.base}/evidence/${id}`)
        .then((r) => { setDetail(r.data); setLoading(false); })
        .catch(() => setLoading(false));
    }
    if (!open) loadedFor.current = null;
  }, [open, id, api.base]);

  if (!open) return null;
  return (
    <div className="fixed inset-0 z-50 flex" data-testid="evidence-drawer">
      <div className="flex-1 bg-black/70 backdrop-blur-sm" onClick={onClose} />
      <aside className="reveal w-full max-w-xl overflow-y-auto border-l border-neutral-800 bg-[#0a0a0a] bg-grain p-7">
        <div className="mb-5 flex items-center justify-between">
          <Pill tone="evidence">{t.evidence_recorded}</Pill>
          <button data-testid="evidence-drawer-close" onClick={onClose} className="rounded border border-neutral-800 p-1.5 text-neutral-500 transition-colors hover:border-neutral-600 hover:text-neutral-200">
            <X className="h-4 w-4" />
          </button>
        </div>
        {loading && <div className="font-mono text-xs uppercase tracking-widest text-neutral-500">{t.drawer_loading}</div>}
        {detail && (
          <>
            <h3 className="font-display text-xl font-bold tracking-tight text-white">{detail.card.title}</h3>
            <p className="mt-2 break-all font-mono text-[11px] text-neutral-500">{t.drawer_source} · <span className="text-neutral-300">{detail.card.source}</span></p>
            <p className="mt-4 text-sm leading-relaxed text-neutral-300">{detail.card.summary}</p>
            {detail.detail && (
              <>
                <div className="mt-6 font-mono text-[10px] uppercase tracking-[0.16em] text-neutral-500">{t.drawer_structured}</div>
                <pre className="mt-2 max-h-72 overflow-auto rounded border border-neutral-800 bg-black p-4 font-mono text-[11px] leading-relaxed text-neutral-300">{JSON.stringify(detail.detail, null, 2)}</pre>
              </>
            )}
            {detail.raw && (
              <>
                <div className="mt-6 font-mono text-[10px] uppercase tracking-[0.16em] text-neutral-500">{t.drawer_raw}</div>
                <pre className="mt-2 max-h-96 overflow-auto rounded border border-neutral-800 bg-black p-4 font-mono text-[11px] leading-relaxed text-neutral-300">{detail.raw}</pre>
              </>
            )}
          </>
        )}
      </aside>
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Roadmap                                                             */
/* ------------------------------------------------------------------ */

function RoadmapColumn({ title, tone, items, testId }) {
  const styles = {
    green: { head: "text-[#8dff77]", dot: "bg-[#39FF14] dot-glow-green", text: "text-neutral-300" },
    amber: { head: "text-amber-300", dot: "bg-amber-400", text: "text-neutral-300" },
    neutral: { head: "text-neutral-400", dot: "bg-neutral-600", text: "text-neutral-500" },
  }[tone];
  return (
    <Card className="p-6">
      <div className="mb-4 flex items-center gap-2">
        <span className={`h-2 w-2 rounded-full ${styles.dot}`} />
        <h3 className={`font-mono text-[11px] font-semibold uppercase tracking-[0.16em] ${styles.head}`}>{title}</h3>
      </div>
      <ul className="space-y-2.5" data-testid={testId}>
        {items.map((x) => (
          <li key={x} className={`flex gap-2.5 text-[13px] leading-relaxed ${styles.text}`}>
            <span className="mt-2 h-1 w-1 shrink-0 rounded-full bg-neutral-700" />
            {x}
          </li>
        ))}
      </ul>
    </Card>
  );
}

function Roadmap({ roadmap, t }) {
  return (
    <section id="roadmap" className="reveal border-t border-neutral-800 px-6 py-12 sm:px-10">
      <SectionHeading index="05" kicker={t.roadmap_kicker} title={t.roadmap_title} sub={t.roadmap_sub} />
      <div className="grid grid-cols-1 gap-6 md:grid-cols-3">
        <RoadmapColumn title={t.roadmap_working} tone="green" items={roadmap.working_now} testId="roadmap-working" />
        <RoadmapColumn title={t.roadmap_next} tone="amber" items={roadmap.next} testId="roadmap-next" />
        <RoadmapColumn title={t.roadmap_future} tone="neutral" items={roadmap.future} testId="roadmap-future" />
      </div>
    </section>
  );
}

/* ------------------------------------------------------------------ */
/* Dashboard                                                           */
/* ------------------------------------------------------------------ */

export default function Dashboard({ api, snapshot, live, evidence, roadmap, arch, fixtures }) {
  const [openEv, setOpenEv] = useState(null);
  const [active, setActive] = useState("overview");
  const [copied, setCopied] = useState(false);
  const [whyOpen, setWhyOpen] = useState(false);
  const [lang, setLang] = useState(() => {
    const saved = typeof localStorage !== "undefined" ? localStorage.getItem("pb-lang") : null;
    return saved === "en" || saved === "fr" ? saved : "fr";
  });
  const t = STRINGS[lang];

  useEffect(() => {
    if (typeof localStorage !== "undefined") localStorage.setItem("pb-lang", lang);
    if (typeof document !== "undefined") document.documentElement.lang = lang;
  }, [lang]);

  useEffect(() => {
    const ids = NAV.map((n) => n.id);
    const observer = new IntersectionObserver(
      (entries) => {
        const visible = entries.filter((e) => e.isIntersecting).sort((a, b) => b.intersectionRatio - a.intersectionRatio);
        if (visible[0]) setActive(visible[0].target.id);
      },
      { rootMargin: "-20% 0px -60% 0px", threshold: [0.1, 0.5, 1] }
    );
    ids.forEach((id) => { const el = document.getElementById(id); if (el) observer.observe(el); });
    return () => observer.disconnect();
  }, []);

  const onCopy = () => {
    const r = snapshot.release;
    const text = `PhoneBoost ${r.tag}\nHEAD ${r.head}\nnative baseline ${r.native_baseline}\ntoolchain ${r.toolchain}\nvalidated ${r.validation_date}`;
    const done = () => { setCopied(true); setTimeout(() => setCopied(false), 1800); };
    const fallbackCopy = () => {
      try {
        const ta = document.createElement("textarea");
        ta.value = text; ta.style.position = "fixed"; ta.style.opacity = "0";
        document.body.appendChild(ta); ta.select(); document.execCommand("copy"); document.body.removeChild(ta);
      } catch { /* clipboard unavailable */ }
      done();
    };
    try {
      const pr = navigator.clipboard?.writeText(text);
      if (pr && typeof pr.then === "function") pr.then(done).catch(fallbackCopy);
      else fallbackCopy();
    } catch { fallbackCopy(); }
  };

  return (
    <div className="min-h-screen bg-[#0a0a0a] text-neutral-200">
      <Sidebar active={active} snapshot={snapshot} onCopy={onCopy} copied={copied} t={t} lang={lang} setLang={setLang} />

      <div className="lg:pl-60">
        {/* Provenance banner */}
        <div className="sticky top-0 z-20 border-b border-neutral-800 bg-[#0a0a0a]/85 backdrop-blur">
          <div className="flex items-center justify-between gap-3 px-6 py-2.5 sm:px-10">
            <div className="flex min-w-0 items-center gap-3">
              <Pill tone="evidence" testId="mode-badge">{t.banner_recorded}</Pill>
              <span className="hidden truncate font-mono text-[10px] text-neutral-500 md:inline">
                master <span className="text-neutral-300">{snapshot.release.head.slice(0, 10)}</span>
                {" · "}baseline <span className="text-neutral-300">{snapshot.release.native_baseline.slice(0, 10)}</span>
                {" · "}validated <span className="text-neutral-300">{snapshot.release.validation_date}</span>
              </span>
            </div>
            <div className="flex items-center gap-3">
              <span className="hidden font-mono text-[9px] uppercase tracking-[0.2em] text-neutral-600 sm:inline">{t.banner_poc}</span>
              <div className="lg:hidden"><LanguageToggle lang={lang} setLang={setLang} testId="banner-lang-toggle" prefix="banner" /></div>
              <a data-testid="repo-link" href={snapshot.release.repo} target="_blank" rel="noreferrer" className="inline-flex shrink-0 items-center gap-1.5 font-mono text-[10px] uppercase tracking-[0.14em] text-neutral-500 transition-colors hover:text-neutral-200">
                <Github className="h-3.5 w-3.5" /> {t.footer_repo}
              </a>
            </div>
          </div>
        </div>

        <main>
          <Hero t={t} />
          <Why t={t} />
          <How snapshot={snapshot} live={live} whyOpen={whyOpen} onToggleWhy={() => setWhyOpen((o) => !o)} t={t} lang={lang} />
          <GateLadder gates={snapshot.gates} t={t} lang={lang} />
          <EvidenceSection evidence={evidence} fixtures={fixtures} snapshot={snapshot} arch={arch} onOpen={setOpenEv} t={t} lang={lang} />
          <Roadmap roadmap={roadmap} t={t} />

          <footer className="border-t border-neutral-800 px-6 py-10 sm:px-10">
            <div className="flex flex-wrap items-center gap-x-4 gap-y-2 font-mono text-[11px] text-neutral-500">
              <span>{t.footer_release} <span className="text-neutral-300">{snapshot.release.tag}</span></span>
              <span className="text-neutral-700">·</span>
              <span className="break-all">{t.footer_master} <span className="text-neutral-300">{snapshot.release.head.slice(0, 12)}</span></span>
              <span className="text-neutral-700">·</span>
              <a href={snapshot.release.repo} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1.5 text-neutral-400 transition-colors hover:text-[#8dff77]">
                {t.footer_repo} <ExternalLink className="h-3 w-3" />
              </a>
            </div>
            <p className="mt-3 max-w-3xl text-[11px] leading-relaxed text-neutral-600">{t.footer_disclaimer}</p>
          </footer>
        </main>
      </div>

      <EvidenceDrawer open={!!openEv} onClose={() => setOpenEv(null)} api={api} id={openEv} t={t} />
    </div>
  );
}
