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
} from "lucide-react";

const GATE_LABELS = {
  paired: "Paired",
  authenticated: "Authenticated",
  controller_lease: "Controller lease",
  resource_admissible: "Resource admissible",
  provider_ready: "Provider ready",
};

const NAV = [
  { id: "overview", label: "Overview", icon: LayoutGrid },
  { id: "how", label: "How it works", icon: Route },
  { id: "ladder", label: "Trust & control", icon: ListChecks },
  { id: "evidence", label: "Evidence", icon: FileCheck2 },
  { id: "roadmap", label: "Roadmap", icon: Layers },
];

/* ------------------------------------------------------------------ */
/* Primitives                                                          */
/* ------------------------------------------------------------------ */

function Pill({ children, tone = "unavailable", dot = true, testId, className = "" }) {
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
      className={`inline-flex items-center gap-1.5 rounded border px-2.5 py-1 font-mono text-[10px] font-medium uppercase tracking-[0.14em] ${tones[tone]} ${className}`}
    >
      {dot && <span className={`h-1.5 w-1.5 rounded-full ${dots[tone]}`} />}
      {children}
    </span>
  );
}

function StateChip({ state, testId }) {
  const s = (state || "").toUpperCase();
  const isRoadmap = s === "ROADMAP";
  return (
    <Pill
      tone={isRoadmap ? "roadmap" : "unavailable"}
      dot={!isRoadmap}
      testId={testId}
      className="max-w-full truncate"
    >
      <span className="truncate">{isRoadmap ? "Roadmap" : state}</span>
    </Pill>
  );
}

function Card({ children, className = "", testId }) {
  return (
    <div
      data-testid={testId}
      className={`rounded-lg border border-neutral-800 bg-[#111111] bg-grain ${className}`}
    >
      {children}
    </div>
  );
}

function SectionHeading({ index, kicker, title, sub, right }) {
  return (
    <div className="mb-6 flex flex-wrap items-end justify-between gap-4">
      <div>
        <div className="mb-2 flex items-center gap-2.5">
          <span className="font-mono text-[11px] font-semibold tracking-[0.2em] text-[#39FF14]/70">
            {index}
          </span>
          <span className="h-px w-8 bg-neutral-700" />
          <span className="font-mono text-[10px] uppercase tracking-[0.24em] text-neutral-500">
            {kicker}
          </span>
        </div>
        <h2 className="font-display text-2xl font-bold tracking-tight text-white sm:text-[1.7rem]">
          {title}
        </h2>
        {sub && <p className="mt-1.5 max-w-2xl text-sm text-neutral-400">{sub}</p>}
      </div>
      {right}
    </div>
  );
}

function KV({ k, v, testId }) {
  return (
    <div
      className="flex items-center justify-between gap-4 border-t border-neutral-800/70 py-2.5 first:border-t-0"
      data-testid={testId}
    >
      <span className="shrink-0 text-xs text-neutral-500">{k}</span>
      <StateChip state={v} />
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
        <ChevronDown
          className={`h-4 w-4 shrink-0 text-neutral-500 transition-transform ${open ? "rotate-180" : ""}`}
        />
      </button>
      {open && <div className="border-t border-neutral-800 px-5 py-4">{children}</div>}
    </Card>
  );
}

/* ------------------------------------------------------------------ */
/* Sidebar                                                             */
/* ------------------------------------------------------------------ */

function Sidebar({ active, snapshot, onCopy, copied }) {
  return (
    <aside className="fixed inset-y-0 left-0 z-30 hidden w-60 flex-col border-r border-neutral-800 bg-[#0a0a0a] bg-grain lg:flex">
      <div className="flex items-center gap-3 border-b border-neutral-800 px-5 py-5">
        <div className="flex h-9 w-9 items-center justify-center rounded border border-[#39FF14]/40 bg-[#39FF14]/5 font-display text-sm font-black text-[#39FF14] text-glow-green">
          PB
        </div>
        <div className="leading-none">
          <div className="font-display text-sm font-bold tracking-wide text-white">PhoneBoost</div>
          <div className="mt-1 font-mono text-[9px] uppercase tracking-[0.2em] text-neutral-500">
            Control Center
          </div>
        </div>
      </div>

      <nav className="flex-1 px-3 py-5" data-testid="side-nav">
        <div className="mb-3 px-2 font-mono text-[9px] uppercase tracking-[0.24em] text-neutral-600">
          Sections
        </div>
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
                  <span
                    className={`absolute left-0 top-1/2 h-4 w-0.5 -translate-y-1/2 rounded-full transition-all ${
                      on ? "bg-[#39FF14] dot-glow-green" : "bg-transparent"
                    }`}
                  />
                  <Icon
                    className={`h-4 w-4 shrink-0 ${on ? "text-[#39FF14]" : "text-neutral-600 group-hover:text-neutral-400"}`}
                    strokeWidth={1.75}
                  />
                  {n.label}
                </a>
              </li>
            );
          })}
        </ul>
      </nav>

      <div className="border-t border-neutral-800 px-4 py-4">
        <div className="mb-2 font-mono text-[9px] uppercase tracking-[0.2em] text-neutral-600">
          Release anchor
        </div>
        <div className="space-y-1 font-mono text-[10px] text-neutral-500">
          <div className="truncate">
            tag <span className="text-neutral-300">{snapshot.release.tag}</span>
          </div>
          <div className="truncate">
            head <span className="text-neutral-300">{snapshot.release.head.slice(0, 10)}</span>
          </div>
        </div>
        <button
          data-testid="copy-repo-anchor"
          onClick={onCopy}
          className="mt-3 inline-flex w-full items-center justify-center gap-1.5 rounded border border-neutral-800 bg-neutral-900/60 px-2 py-1.5 font-mono text-[10px] uppercase tracking-[0.14em] text-neutral-400 transition-colors hover:border-[#39FF14]/40 hover:text-[#8dff77]"
        >
          {copied ? <Check className="h-3 w-3 text-[#39FF14]" /> : <Copy className="h-3 w-3" />}
          {copied ? "Copied" : "Copy anchor"}
        </button>
      </div>
    </aside>
  );
}

/* ------------------------------------------------------------------ */
/* Enable control (truthful — cannot fake LIVE)                         */
/* ------------------------------------------------------------------ */

function EnableControl({ live, expanded, onToggleWhy }) {
  return (
    <div
      className="w-full max-w-sm rounded-xl border border-amber-500/25 bg-[#111111] bg-grain p-5"
      data-testid="enable-phoneboost-panel"
    >
      <div className="flex items-center justify-between gap-4">
        <div className="flex items-center gap-3">
          <div className="flex h-10 w-10 items-center justify-center rounded-lg border border-amber-500/30 bg-amber-500/5">
            <Power className="h-5 w-5 text-amber-400" strokeWidth={1.75} />
          </div>
          <div className="leading-tight">
            <div className="font-display text-sm font-semibold text-white">Enable PhoneBoost</div>
            <div className="font-mono text-[10px] uppercase tracking-[0.18em] text-amber-300/80">
              Off · Unavailable
            </div>
          </div>
        </div>
        {/* Disabled switch — cannot be turned on without a reachable native runtime */}
        <button
          data-testid="enable-phoneboost-control"
          disabled
          aria-disabled="true"
          title="Native runtime not reachable from this hosted browser"
          className="relative h-7 w-12 cursor-not-allowed rounded-full border border-neutral-700 bg-neutral-800/80"
        >
          <span className="absolute left-1 top-1/2 h-5 w-5 -translate-y-1/2 rounded-full bg-neutral-500" />
        </button>
      </div>
      <p className="mt-3 text-xs leading-relaxed text-amber-100/80">
        Native runtime not reachable from this hosted browser.
      </p>
      <button
        data-testid="enable-why-toggle"
        onClick={onToggleWhy}
        className="mt-2 inline-flex items-center gap-1 font-mono text-[10px] uppercase tracking-[0.14em] text-neutral-400 transition-colors hover:text-amber-200"
      >
        {expanded ? "Hide details" : "Why can't I enable it?"}
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
          <p className="mt-3 font-mono text-[9px] uppercase tracking-[0.16em] text-neutral-600">
            No LIVE state is ever fabricated here.
          </p>
        </div>
      )}
    </div>
  );
}

/* ------------------------------------------------------------------ */
/* Hero                                                                */
/* ------------------------------------------------------------------ */

function Hero({ snapshot, live, whyOpen, onToggleWhy }) {
  return (
    <section id="overview" className="reveal relative overflow-hidden border-b border-neutral-800">
      <div className="pointer-events-none absolute -right-24 -top-24 h-72 w-72 rounded-full bg-[#39FF14]/5 blur-3xl" />
      <div className="relative px-6 py-10 sm:px-10 sm:py-14">
        <div className="flex flex-col justify-between gap-8 lg:flex-row lg:items-center">
          <div className="max-w-xl">
            <div className="mb-4 flex items-center gap-2.5">
              <span className="h-1.5 w-1.5 rounded-full bg-[#39FF14] dot-glow-green" />
              <span className="font-mono text-[10px] uppercase tracking-[0.28em] text-neutral-500">
                Secure remote compute node
              </span>
            </div>
            <h1 className="font-display text-5xl font-black tracking-tighter text-white sm:text-6xl">
              Phone<span className="text-[#39FF14] text-glow-green">Boost</span>
            </h1>
            <p className="mt-4 font-display text-lg font-medium tracking-tight text-neutral-100 sm:text-xl">
              Your phone becomes a secure compute node for your Linux PC.
            </p>
            <p className="mt-2 text-sm text-neutral-400">Plug it in. PhoneBoost puts it to work.</p>

            <div className="mt-6 flex flex-wrap items-center gap-2.5" data-testid="hero-status-chips">
              <Pill tone="warn" testId="chip-live">Live · unavailable</Pill>
              <Pill tone="evidence" testId="chip-evidence">Recorded evidence · available</Pill>
            </div>
          </div>

          <EnableControl live={live} expanded={whyOpen} onToggleWhy={onToggleWhy} />
        </div>
      </div>
    </section>
  );
}

/* ------------------------------------------------------------------ */
/* At-a-glance status                                                  */
/* ------------------------------------------------------------------ */

function GlanceCard({ icon: Icon, label, value, tone = "unavailable", note, href, testId }) {
  const Wrapper = href ? "a" : "div";
  return (
    <Wrapper
      href={href}
      data-testid={testId}
      className={`flex flex-col rounded-lg border border-neutral-800 bg-[#111111] bg-grain p-5 ${
        href ? "transition-colors hover:border-[#39FF14]/40" : ""
      }`}
    >
      <div className="mb-3 flex items-center gap-2 text-neutral-500">
        <Icon className="h-4 w-4" strokeWidth={1.75} />
        <span className="font-mono text-[9px] uppercase tracking-[0.18em]">{label}</span>
      </div>
      <div className="mb-2">
        <Pill tone={tone} dot={tone !== "roadmap"}>
          {value}
        </Pill>
      </div>
      {note && <p className="mt-auto text-[11px] leading-relaxed text-neutral-500">{note}</p>}
    </Wrapper>
  );
}

function Glance({ snapshot }) {
  return (
    <section className="reveal px-6 py-10 sm:px-10">
      <div className="mb-5 font-mono text-[10px] uppercase tracking-[0.24em] text-neutral-500">
        At a glance
      </div>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
        <GlanceCard
          testId="glance-availability"
          icon={Gauge}
          label="Is PhoneBoost available?"
          value="Offline in browser"
          tone="warn"
          note="Needs a local phoneboostd runtime; a hosted browser cannot reach it."
        />
        <GlanceCard
          testId="glance-phone"
          icon={Smartphone}
          label="Is a phone contributing?"
          value="No phone connected"
          note="Pair an Android worker over the secure link to contribute capacity."
        />
        <GlanceCard
          testId="glance-trust"
          icon={ShieldCheck}
          label="Trust & control state"
          value="All gates unavailable"
          href="#ladder"
          note="Five independent authority gates — none satisfied without a live bridge."
        />
        <GlanceCard
          testId="glance-evidence"
          icon={FileCheck2}
          label="Where's the proof?"
          value="Evidence available"
          tone="evidence"
          href="#evidence"
          note="Recorded, truth-audited tests and builds from the release repository."
        />
      </div>
    </section>
  );
}

/* ------------------------------------------------------------------ */
/* Simple phone-as-node topology                                       */
/* ------------------------------------------------------------------ */

function TopologyNode({ icon: Icon, tag, tagTone, title, subtitle, state }) {
  const tones = {
    local: "border-sky-500/25",
    remote: "border-neutral-700",
  };
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
            <span className={`rounded border px-2 py-0.5 font-mono text-[9px] uppercase tracking-[0.16em] ${chips[tagTone]}`}>
              {tag}
            </span>
          </div>
          <div className="mt-0.5 font-mono text-[10px] uppercase tracking-[0.16em] text-neutral-500">{subtitle}</div>
        </div>
      </div>
      <StateChip state={state} />
    </div>
  );
}

function SimpleTopology({ snapshot }) {
  const sl = snapshot.secure_link;
  return (
    <section className="reveal px-6 pb-4 sm:px-10">
      <div className="mb-5 font-mono text-[10px] uppercase tracking-[0.24em] text-neutral-500">
        How it connects
      </div>
      <div className="mx-auto max-w-2xl space-y-0">
        <TopologyNode
          icon={Cpu}
          tag="Local"
          tagTone="local"
          title="Linux PC"
          subtitle="x86-64 · orchestrator"
          state={snapshot.computer.runtime.state}
        />
        {/* Secure link connector */}
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
              <div className="font-display text-[13px] font-medium text-amber-100">Secure link</div>
              <div className="font-mono text-[9px] uppercase tracking-[0.16em] text-neutral-500">
                Noise XX → IK · PBMUX · fail-closed
              </div>
            </div>
            <StateChip state={sl.session.state} />
          </div>
        </div>
        <TopologyNode
          icon={Smartphone}
          tag="Remote node"
          tagTone="remote"
          title="Android phone"
          subtitle="ARM64 · trusted worker"
          state={snapshot.phone.endpoint.state}
        />
      </div>
      <p className="mx-auto mt-4 max-w-2xl text-center text-[11px] text-neutral-500">
        The phone works as a separate remote node. Its resources are never merged into the Linux host.
      </p>
    </section>
  );
}

/* ------------------------------------------------------------------ */
/* Capabilities (user value)                                           */
/* ------------------------------------------------------------------ */

function CapabilityCard({ icon: Icon, title, desc, tone, statusLabel }) {
  return (
    <Card className="p-5">
      <div className="mb-3 flex items-center justify-between">
        <div className="flex h-9 w-9 items-center justify-center rounded-lg border border-neutral-800 bg-neutral-900">
          <Icon className="h-4 w-4 text-neutral-300" strokeWidth={1.75} />
        </div>
        <Pill tone={tone} dot={tone !== "roadmap"}>
          {statusLabel}
        </Pill>
      </div>
      <div className="font-display text-[15px] font-semibold text-white">{title}</div>
      <p className="mt-1.5 text-xs leading-relaxed text-neutral-400">{desc}</p>
    </Card>
  );
}

function Capabilities() {
  return (
    <section className="reveal px-6 py-10 sm:px-10">
      <div className="mb-5 font-mono text-[10px] uppercase tracking-[0.24em] text-neutral-500">
        What the phone can do
      </div>
      <div className="grid grid-cols-1 gap-4 sm:grid-cols-2 xl:grid-cols-4">
        <CapabilityCard
          icon={Radio}
          title="Secure link"
          desc="Authenticated, fail-closed channel between PC and phone."
          tone="unavailable"
          statusLabel="Implemented · not established"
        />
        <CapabilityCard
          icon={Boxes}
          title="Remote capacity"
          desc="Bounded, volatile storage held on the remote node only."
          tone="roadmap"
          statusLabel="Roadmap"
        />
        <CapabilityCard
          icon={Cpu}
          title="Remote compute"
          desc="Explicit remote jobs, always worker-authoritative."
          tone="roadmap"
          statusLabel="Roadmap"
        />
        <CapabilityCard
          icon={FileCheck2}
          title="Recorded evidence"
          desc="Truth-audited tests and builds from the release repo."
          tone="evidence"
          statusLabel="Available"
        />
      </div>
    </section>
  );
}

/* ------------------------------------------------------------------ */
/* How it works + technical details                                    */
/* ------------------------------------------------------------------ */

function Step({ n, title, desc }) {
  return (
    <div className="flex gap-4">
      <div className="flex h-8 w-8 shrink-0 items-center justify-center rounded-full border border-neutral-700 font-mono text-xs text-neutral-400">
        {n}
      </div>
      <div>
        <div className="font-display text-sm font-semibold text-white">{title}</div>
        <p className="mt-1 text-xs leading-relaxed text-neutral-400">{desc}</p>
      </div>
    </div>
  );
}

function HowItWorks({ snapshot, arch }) {
  const sl = snapshot.secure_link;
  const c = snapshot.computer;
  const p = snapshot.phone;
  return (
    <section id="how" className="reveal border-t border-neutral-800 px-6 py-12 sm:px-10">
      <SectionHeading
        index="01"
        kicker="How it works"
        title="Plug it in. PhoneBoost puts it to work."
        sub="A simple flow, backed by a strict authority model. Deeper protocol detail stays tucked away below."
      />

      <div className="grid grid-cols-1 gap-6 lg:grid-cols-3">
        <Card className="p-6">
          <Step n="1" title="Connect the phone" desc="Pair an Android phone with your Linux PC over a local, secure link." />
        </Card>
        <Card className="p-6">
          <Step n="2" title="Trust is established" desc="Pairing, authentication and a controller lease must all pass before anything runs." />
        </Card>
        <Card className="p-6">
          <Step n="3" title="It gets put to work" desc="When the runtime and authority gates allow, the phone contributes as a remote node." />
        </Card>
      </div>

      {/* Honest technical clarification — secondary */}
      <Card className="mt-6 p-6">
        <div className="flex items-start gap-3">
          <PlugZap className="mt-0.5 h-5 w-5 shrink-0 text-neutral-500" strokeWidth={1.75} />
          <p className="text-sm leading-relaxed text-neutral-400">
            PhoneBoost is explicit remote resource cooperation between two separate machines. It is
            <span className="text-neutral-200"> not</span> RAM extension, swap, or a CPU illusion — Android capacity is
            never transparently merged into the Linux host, and the hosted browser itself cannot perform native
            pairing or control.
          </p>
        </div>
      </Card>

      {/* Technical details — collapsible, off by default */}
      <div className="mt-6 space-y-4">
        <Collapsible title="Secure link details" icon={Radio} testId="collapsible-secure-link">
          <KV k="Transport" v={sl.transport.state} testId="kv-transport" />
          <KV k="Session" v={sl.session.state} testId="kv-session" />
          <KV k="Auth" v={sl.authentication.state} testId="kv-auth" />
          <KV k="Latency" v={sl.latency.state} testId="kv-latency" />
        </Collapsible>

        <Collapsible title="Local host & remote worker" icon={Terminal} testId="collapsible-endpoints">
          <div className="grid grid-cols-1 gap-6 md:grid-cols-2">
            <div>
              <div className="mb-2 flex items-center gap-2">
                <Cpu className="h-4 w-4 text-sky-300" strokeWidth={1.75} />
                <span className="font-display text-[13px] font-semibold text-white">{c.label}</span>
              </div>
              <KV k="phoneboostd" v={c.runtime.state} testId="kv-phoneboostd" />
              <KV k="Local API" v={c.local_api.state} testId="kv-local-api" />
              <p className="mt-2 text-[11px] text-neutral-500">{c.note}</p>
            </div>
            <div>
              <div className="mb-2 flex items-center gap-2">
                <Smartphone className="h-4 w-4 text-neutral-300" strokeWidth={1.75} />
                <span className="font-display text-[13px] font-semibold text-white">{p.label}</span>
              </div>
              <KV k="Endpoint" v={p.endpoint.state} testId="kv-endpoint" />
              <KV k="Worker" v={p.worker.state} testId="kv-worker" />
              <KV k="Incarnation" v={p.incarnation.state} testId="kv-incarnation" />
              <p className="mt-2 text-[11px] text-neutral-500">{p.health.note}</p>
            </div>
          </div>
        </Collapsible>

        <Collapsible title="Remote capability" icon={Boxes} testId="collapsible-remote-capability">
          <KV k="Admitted capacity" v={snapshot.remote_capability.admitted_capacity.state} testId="kv-admitted" />
          <KV k="Reserved" v={snapshot.remote_capability.reserved.state} testId="kv-reserved" />
          <KV k="RemoteBuffer" v={snapshot.remote_capability.active_remote_buffer.state} testId="kv-remotebuffer" />
          <KV k="Remote job" v={snapshot.remote_capability.active_remote_job.state} testId="kv-remote-job" />
          <p className="mt-2 text-[11px] text-neutral-500">{snapshot.remote_capability.note}</p>
        </Collapsible>

        <Collapsible
          title="Architecture layers"
          icon={Layers}
          testId="collapsible-architecture"
          right={<Pill tone="evidence">Repository truth</Pill>}
        >
          <ol className="space-y-2">
            {arch.layers.map((l, i) => (
              <li
                key={l.layer}
                data-testid={`arch-${i}`}
                className="grid grid-cols-1 items-start gap-2 rounded border border-neutral-800/70 bg-neutral-900/30 p-3 md:grid-cols-[150px_120px_1fr]"
              >
                <div className="font-display text-[13px] font-medium text-neutral-100">{l.layer}</div>
                <div className="font-mono text-[10px] uppercase tracking-[0.14em] text-neutral-500">{l.role}</div>
                <div className="break-all font-mono text-[11px] text-neutral-400">{l.detail}</div>
              </li>
            ))}
          </ol>
        </Collapsible>

        <Collapsible title="Security" icon={ShieldCheck} testId="collapsible-security" defaultOpen>
          <div className="flex items-start gap-3">
            <ShieldCheck className="mt-0.5 h-5 w-5 shrink-0 text-[#39FF14]" strokeWidth={1.75} />
            <div>
              <p className="text-sm leading-relaxed text-neutral-200">{snapshot.security_plain_language}</p>
              <p className="mt-2 break-words font-mono text-[11px] leading-relaxed text-neutral-500">
                Full 256-bit peer IDs (<span className="text-neutral-300">SHA-256(static_public_key)</span>), Noise XX
                first pair with QR-01A SAS, Noise IK reconnect, PBMUX authenticated framing, fail-closed dispatch,
                worker-authoritative ResourceGuard. Session material, private keys and SAS values are never exposed by
                this UI.
              </p>
            </div>
          </div>
        </Collapsible>
      </div>
    </section>
  );
}

/* ------------------------------------------------------------------ */
/* Five-gate ladder                                                    */
/* ------------------------------------------------------------------ */

function GateLadder({ gates }) {
  return (
    <section id="ladder" className="reveal border-t border-neutral-800 px-6 py-12 sm:px-10">
      <SectionHeading
        index="02"
        kicker="Trust & control"
        title="Five independent gates"
        sub="Each gate is a distinct authority check — not a sequence and not a progress bar. A peer ID is identity only; it does not authenticate a session, grant a lease, or create capacity."
        right={<Pill tone="warn" testId="ladder-status">All unavailable · no live bridge</Pill>}
      />
      <ol data-testid="gate-ladder" className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3">
        {gates.map((g, i) => (
          <li
            key={g.id}
            data-testid={`gate-${g.id}`}
            className="group flex flex-col rounded-lg border border-neutral-800 bg-[#111111] bg-grain p-5 transition-colors hover:border-neutral-700"
          >
            <div className="mb-3 flex items-center justify-between">
              <span className="font-mono text-lg font-semibold tracking-tight text-neutral-700">
                {String(i + 1).padStart(2, "0")}
              </span>
              <Pill tone="unavailable" testId={`gate-${g.id}-badge`}>
                {g.state}
              </Pill>
            </div>
            <div className="mb-2 flex items-center gap-2">
              <span className="h-2 w-2 rounded-full border border-neutral-600 bg-transparent" />
              <span className="font-display text-[15px] font-semibold text-white">
                {GATE_LABELS[g.id] || g.label}
              </span>
            </div>
            <p className="text-xs leading-relaxed text-neutral-400">{g.explanation}</p>
            <p className="mt-3 border-t border-neutral-800/70 pt-2.5 font-mono text-[10px] italic text-neutral-600">
              {g.reason}
            </p>
          </li>
        ))}
      </ol>
    </section>
  );
}

/* ------------------------------------------------------------------ */
/* Evidence                                                            */
/* ------------------------------------------------------------------ */

function EvidenceGrid({ evidence, fixtures, snapshot, onOpen }) {
  return (
    <section id="evidence" className="reveal border-t border-neutral-800 px-6 py-12 sm:px-10">
      <SectionHeading
        index="03"
        kicker="Recorded evidence"
        title="Truth-audited technical proof"
        sub="Every card is anchored to a checked-in file in the release repository. Nothing here is live telemetry — it is verified, recorded evidence."
        right={
          <div className="hidden items-center gap-4 font-mono text-[10px] uppercase tracking-[0.14em] text-neutral-500 md:flex">
            <span className="inline-flex items-center gap-1.5">
              <GitBranch className="h-3.5 w-3.5" /> {snapshot.release.head.slice(0, 10)}
            </span>
            {fixtures && (
              <span>
                fixtures <span className="text-neutral-300">{fixtures.file_count}</span>
              </span>
            )}
          </div>
        }
      />
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
        {evidence.map((c) => (
          <button
            key={c.id}
            data-testid={`evidence-card-${c.id}`}
            onClick={() => onOpen(c.id)}
            className="ring-glow-green group relative flex flex-col overflow-hidden rounded-lg border border-neutral-800 bg-[#111111] bg-grain p-5 text-left transition-colors hover:border-[#39FF14]/40"
          >
            <span className="absolute inset-x-0 top-0 h-px bg-gradient-to-r from-transparent via-[#39FF14]/40 to-transparent opacity-0 transition-opacity group-hover:opacity-100" />
            <div className="mb-3 flex items-center justify-between">
              <Pill tone="evidence">Recorded evidence</Pill>
              <span className="font-mono text-[9px] uppercase tracking-[0.16em] text-neutral-600">{c.kind}</span>
            </div>
            <h3 className="font-display text-[15px] font-semibold leading-snug text-white">{c.title}</h3>
            <p className="mt-2 flex-1 text-xs leading-relaxed text-neutral-400">{c.summary}</p>
            <div className="mt-4 flex items-center justify-between gap-3 border-t border-neutral-800/70 pt-3">
              <span className="truncate font-mono text-[10px] text-neutral-600">{c.source}</span>
              <span className="inline-flex items-center gap-1 font-mono text-[10px] uppercase tracking-[0.14em] text-neutral-500 transition-colors group-hover:text-[#8dff77]">
                Open <ChevronRight className="h-3 w-3" />
              </span>
            </div>
          </button>
        ))}
      </div>
    </section>
  );
}

function EvidenceDrawer({ open, onClose, api, id }) {
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
        .then((r) => {
          setDetail(r.data);
          setLoading(false);
        })
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
          <Pill tone="evidence">Recorded evidence</Pill>
          <button
            data-testid="evidence-drawer-close"
            onClick={onClose}
            className="rounded border border-neutral-800 p-1.5 text-neutral-500 transition-colors hover:border-neutral-600 hover:text-neutral-200"
          >
            <X className="h-4 w-4" />
          </button>
        </div>
        {loading && (
          <div className="font-mono text-xs uppercase tracking-widest text-neutral-500">Loading evidence…</div>
        )}
        {detail && (
          <>
            <h3 className="font-display text-xl font-bold tracking-tight text-white">{detail.card.title}</h3>
            <p className="mt-2 break-all font-mono text-[11px] text-neutral-500">
              source · <span className="text-neutral-300">{detail.card.source}</span>
            </p>
            <p className="mt-4 text-sm leading-relaxed text-neutral-300">{detail.card.summary}</p>
            {detail.detail && (
              <>
                <div className="mt-6 font-mono text-[10px] uppercase tracking-[0.16em] text-neutral-500">
                  Structured detail
                </div>
                <pre className="mt-2 max-h-72 overflow-auto rounded border border-neutral-800 bg-black p-4 font-mono text-[11px] leading-relaxed text-neutral-300">
                  {JSON.stringify(detail.detail, null, 2)}
                </pre>
              </>
            )}
            {detail.raw && (
              <>
                <div className="mt-6 font-mono text-[10px] uppercase tracking-[0.16em] text-neutral-500">
                  Raw evidence · redacted at source
                </div>
                <pre className="mt-2 max-h-96 overflow-auto rounded border border-neutral-800 bg-black p-4 font-mono text-[11px] leading-relaxed text-neutral-300">
                  {detail.raw}
                </pre>
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

function Roadmap({ roadmap }) {
  return (
    <section id="roadmap" className="reveal border-t border-neutral-800 px-6 py-12 sm:px-10">
      <SectionHeading
        index="04"
        kicker="Trajectory"
        title="Working now, next, and future"
        sub="A deliberate separation between what is demonstrated today and what remains roadmap. Nothing in Next or Future is presented as implemented."
      />
      <div className="grid grid-cols-1 gap-6 md:grid-cols-3">
        <RoadmapColumn title="Working / demonstrated now" tone="green" items={roadmap.working_now} testId="roadmap-working" />
        <RoadmapColumn title="Next" tone="amber" items={roadmap.next} testId="roadmap-next" />
        <RoadmapColumn title="Future" tone="neutral" items={roadmap.future} testId="roadmap-future" />
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

  useEffect(() => {
    const ids = NAV.map((n) => n.id);
    const observer = new IntersectionObserver(
      (entries) => {
        const visible = entries
          .filter((e) => e.isIntersecting)
          .sort((a, b) => b.intersectionRatio - a.intersectionRatio);
        if (visible[0]) setActive(visible[0].target.id);
      },
      { rootMargin: "-20% 0px -60% 0px", threshold: [0.1, 0.5, 1] }
    );
    ids.forEach((id) => {
      const el = document.getElementById(id);
      if (el) observer.observe(el);
    });
    return () => observer.disconnect();
  }, []);

  const onCopy = () => {
    const r = snapshot.release;
    const text = `PhoneBoost ${r.tag}\nHEAD ${r.head}\nnative baseline ${r.native_baseline}\ntoolchain ${r.toolchain}\nvalidated ${r.validation_date}`;
    const done = () => {
      setCopied(true);
      setTimeout(() => setCopied(false), 1800);
    };
    const fallbackCopy = () => {
      try {
        const ta = document.createElement("textarea");
        ta.value = text;
        ta.style.position = "fixed";
        ta.style.opacity = "0";
        document.body.appendChild(ta);
        ta.select();
        document.execCommand("copy");
        document.body.removeChild(ta);
      } catch {
        /* clipboard unavailable; ignore */
      }
      done();
    };
    try {
      const p = navigator.clipboard?.writeText(text);
      if (p && typeof p.then === "function") p.then(done).catch(fallbackCopy);
      else fallbackCopy();
    } catch {
      fallbackCopy();
    }
  };

  return (
    <div className="min-h-screen bg-[#0a0a0a] text-neutral-200">
      <Sidebar active={active} snapshot={snapshot} onCopy={onCopy} copied={copied} />

      <div className="lg:pl-60">
        {/* Provenance banner (static, honest) */}
        <div className="sticky top-0 z-20 border-b border-neutral-800 bg-[#0a0a0a]/85 backdrop-blur">
          <div className="flex items-center justify-between gap-3 px-6 py-2.5 sm:px-10">
            <div className="flex min-w-0 items-center gap-3">
              <Pill tone="evidence" testId="mode-badge">Recorded evidence</Pill>
              <span className="hidden truncate font-mono text-[10px] text-neutral-500 md:inline">
                native <span className="text-neutral-300">{snapshot.release.native_baseline.slice(0, 12)}</span>
                {" · "}toolchain <span className="text-neutral-300">{snapshot.release.toolchain}</span>
                {" · "}validated <span className="text-neutral-300">{snapshot.release.validation_date}</span>
              </span>
            </div>
            <a
              data-testid="repo-link"
              href={snapshot.release.repo}
              target="_blank"
              rel="noreferrer"
              className="inline-flex shrink-0 items-center gap-1.5 font-mono text-[10px] uppercase tracking-[0.14em] text-neutral-500 transition-colors hover:text-neutral-200"
            >
              <Github className="h-3.5 w-3.5" /> Repo
            </a>
          </div>
        </div>

        <main>
          <Hero snapshot={snapshot} live={live} whyOpen={whyOpen} onToggleWhy={() => setWhyOpen((o) => !o)} />
          <Glance snapshot={snapshot} />
          <SimpleTopology snapshot={snapshot} />
          <Capabilities />
          <HowItWorks snapshot={snapshot} arch={arch} />
          <GateLadder gates={snapshot.gates} />
          <EvidenceGrid evidence={evidence} fixtures={fixtures} snapshot={snapshot} onOpen={setOpenEv} />
          <Roadmap roadmap={roadmap} />

          <footer className="border-t border-neutral-800 px-6 py-10 sm:px-10">
            <div className="flex flex-wrap items-center gap-x-4 gap-y-2 font-mono text-[11px] text-neutral-500">
              <span>
                release <span className="text-neutral-300">{snapshot.release.tag}</span>
              </span>
              <span className="text-neutral-700">·</span>
              <span className="break-all">
                head <span className="text-neutral-300">{snapshot.release.head}</span>
              </span>
              <span className="text-neutral-700">·</span>
              <a
                href={snapshot.release.repo}
                target="_blank"
                rel="noreferrer"
                className="inline-flex items-center gap-1.5 text-neutral-400 transition-colors hover:text-[#8dff77]"
              >
                Repository <ExternalLink className="h-3 w-3" />
              </a>
            </div>
            <p className="mt-3 max-w-3xl text-[11px] leading-relaxed text-neutral-600">
              PhoneBoost is a non-production proof of concept for explicit remote resource cooperation between a Linux
              x86-64 host and an Android ARM64 worker. It is not RAM extension, not swap, not a CPU illusion, and not a
              cloud service. This Control Center never fabricates LIVE state.
            </p>
          </footer>
        </main>
      </div>

      <EvidenceDrawer open={!!openEv} onClose={() => setOpenEv(null)} api={api} id={openEv} />
    </div>
  );
}
