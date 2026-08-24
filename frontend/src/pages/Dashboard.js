import { useEffect, useRef, useState } from "react";
import axios from "axios";
import {
  Cpu,
  Smartphone,
  ShieldCheck,
  ChevronRight,
  Github,
  Lock,
  GitBranch,
  X,
  ExternalLink,
  Radio,
  Copy,
  Check,
  ArrowRight,
  Layers,
  ListChecks,
  FileCheck2,
  Route,
  LayoutGrid,
  Terminal,
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
  { id: "topology", label: "Local ↔ Link ↔ Remote", icon: Route },
  { id: "ladder", label: "State Ladder", icon: ListChecks },
  { id: "evidence", label: "Evidence", icon: FileCheck2 },
  { id: "roadmap", label: "Roadmap", icon: Layers },
];

/* ------------------------------------------------------------------ */
/* Primitives                                                          */
/* ------------------------------------------------------------------ */

function Pill({ children, tone = "unavailable", dot = true, testId, className = "" }) {
  const tones = {
    unavailable:
      "border-neutral-700/70 bg-neutral-800/40 text-neutral-400",
    warn: "border-amber-500/30 bg-amber-500/10 text-amber-300",
    evidence:
      "border-[#39FF14]/40 bg-[#39FF14]/10 text-[#8dff77]",
    roadmap:
      "border-neutral-700/60 bg-transparent text-neutral-500 border-dashed",
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

// Maps any repo state string into a sober, honest presentation.
function StateChip({ state, testId }) {
  const s = (state || "").toUpperCase();
  const isRoadmap = s === "ROADMAP";
  return (
    <Pill
      tone={isRoadmap ? "roadmap" : "unavailable"}
      dot={!isRoadmap}
      testId={testId}
      className="whitespace-nowrap"
    >
      {isRoadmap ? "Roadmap" : state}
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
    <div className="mb-6 flex items-end justify-between gap-4">
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
      <span className="text-xs text-neutral-500">{k}</span>
      <StateChip state={v} />
    </div>
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
          <div className="font-display text-sm font-bold tracking-wide text-white">
            PhoneBoost
          </div>
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
                    on
                      ? "bg-neutral-800/60 text-white"
                      : "text-neutral-500 hover:bg-neutral-800/30 hover:text-neutral-300"
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
/* Header / Hero                                                       */
/* ------------------------------------------------------------------ */

function ModeToggle({ mode, setMode, liveReachable }) {
  return (
    <div className="inline-flex rounded-md border border-neutral-800 bg-neutral-900/60 p-1">
      <button
        data-testid="mode-toggle-recorded"
        onClick={() => setMode("RECORDED_EVIDENCE")}
        className={`rounded px-3 py-1.5 font-mono text-[10px] font-medium uppercase tracking-[0.14em] transition-colors ${
          mode === "RECORDED_EVIDENCE"
            ? "bg-[#39FF14]/10 text-[#8dff77]"
            : "text-neutral-500 hover:text-neutral-300"
        }`}
      >
        Recorded evidence
      </button>
      <button
        data-testid="mode-toggle-live"
        onClick={() => setMode("LIVE")}
        className={`flex items-center gap-1.5 rounded px-3 py-1.5 font-mono text-[10px] font-medium uppercase tracking-[0.14em] transition-colors ${
          mode === "LIVE" ? "bg-amber-500/10 text-amber-300" : "text-neutral-500 hover:text-neutral-300"
        }`}
      >
        Live
        {!liveReachable && <span className="text-amber-400/80">· unavailable</span>}
      </button>
    </div>
  );
}

function Hero({ snapshot, mode, setMode, liveReachable, onCopy, copied }) {
  const r = snapshot.release;
  const provenance = [
    ["tag", r.tag],
    ["head", r.head.slice(0, 12)],
    ["native", r.native_baseline.slice(0, 12)],
    ["toolchain", r.toolchain],
    ["validated", r.validation_date],
  ];
  return (
    <section id="overview" className="reveal relative overflow-hidden border-b border-neutral-800">
      <div className="pointer-events-none absolute -right-24 -top-24 h-72 w-72 rounded-full bg-[#39FF14]/5 blur-3xl" />
      <div className="relative px-6 py-10 sm:px-10 sm:py-14">
        <div className="flex flex-wrap items-start justify-between gap-6">
          <div className="max-w-2xl">
            <div className="mb-4 flex items-center gap-2.5">
              <span className="h-1.5 w-1.5 rounded-full bg-[#39FF14] dot-glow-green" />
              <span className="font-mono text-[10px] uppercase tracking-[0.28em] text-neutral-500">
                Secure remote compute · non-production proof of concept
              </span>
            </div>
            <h1 className="font-display text-5xl font-black tracking-tighter text-white sm:text-6xl">
              Phone<span className="text-[#39FF14] text-glow-green">Boost</span>
            </h1>
            <p className="mt-4 font-display text-lg font-medium tracking-tight text-neutral-200 sm:text-xl">
              Use the power already in your pocket.
            </p>
            <p className="mt-3 max-w-xl text-sm leading-relaxed text-neutral-400">
              Explicit, authenticated cooperation between a Linux x86-64 host and an Android
              ARM64 worker over a fail-closed secure link. Local and remote resources stay
              strictly separate — this is not RAM extension, swap, or a CPU illusion.
            </p>
          </div>

          <div className="flex flex-col items-end gap-3">
            <ModeToggle mode={mode} setMode={setMode} liveReachable={liveReachable} />
            <a
              data-testid="repo-link"
              href={r.repo}
              target="_blank"
              rel="noreferrer"
              className="inline-flex items-center gap-1.5 rounded border border-neutral-800 bg-neutral-900/60 px-3 py-1.5 font-mono text-[10px] uppercase tracking-[0.14em] text-neutral-400 transition-colors hover:border-neutral-600 hover:text-neutral-200"
            >
              <Github className="h-3.5 w-3.5" /> Repository
            </a>
            <button
              data-testid="copy-repo-anchor-hero"
              onClick={onCopy}
              className="inline-flex items-center gap-1.5 font-mono text-[10px] uppercase tracking-[0.14em] text-neutral-500 transition-colors hover:text-[#8dff77]"
            >
              {copied ? <Check className="h-3 w-3 text-[#39FF14]" /> : <Copy className="h-3 w-3" />}
              {copied ? "Anchor copied" : "Copy repo anchor"}
            </button>
          </div>
        </div>

        {/* Provenance strip */}
        <div className="mt-9 flex flex-wrap items-center gap-2.5" data-testid="provenance-strip">
          {provenance.map(([k, v]) => (
            <div
              key={k}
              className="inline-flex items-center gap-2 rounded border border-neutral-800 bg-neutral-900/50 px-3 py-1.5"
            >
              <span className="font-mono text-[9px] uppercase tracking-[0.18em] text-neutral-600">
                {k}
              </span>
              <span className="font-mono text-[11px] text-neutral-300">{v}</span>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

/* ------------------------------------------------------------------ */
/* Topology                                                            */
/* ------------------------------------------------------------------ */

function EndpointColumn({ icon: Icon, kind, title, subtitle, rows, note, accent }) {
  const accents = {
    local: "border-sky-500/20",
    link: "border-amber-500/25",
    remote: "border-neutral-700",
  };
  const chips = {
    local: "text-sky-300 border-sky-500/30 bg-sky-500/5",
    link: "text-amber-300 border-amber-500/30 bg-amber-500/5",
    remote: "text-neutral-300 border-neutral-700 bg-neutral-800/40",
  };
  return (
    <div
      className={`flex flex-col rounded-lg border ${accents[accent]} bg-[#111111] bg-grain p-6`}
    >
      <div className="mb-4 flex items-center justify-between">
        <div className="flex items-center gap-2.5">
          <div className="flex h-8 w-8 items-center justify-center rounded border border-neutral-800 bg-neutral-900">
            <Icon className="h-4 w-4 text-neutral-300" strokeWidth={1.75} />
          </div>
          <div>
            <div className="font-display text-sm font-semibold text-white">{title}</div>
            <div className="font-mono text-[10px] uppercase tracking-[0.16em] text-neutral-500">
              {subtitle}
            </div>
          </div>
        </div>
        <span
          className={`rounded border px-2 py-0.5 font-mono text-[9px] uppercase tracking-[0.16em] ${chips[accent]}`}
        >
          {kind}
        </span>
      </div>
      <div className="flex-1">
        {rows.map((row) => (
          <KV key={row.k} k={row.k} v={row.v} testId={row.testId} />
        ))}
      </div>
      {note && (
        <p className="mt-4 border-t border-neutral-800/70 pt-3 text-[11px] leading-relaxed text-neutral-500">
          {note}
        </p>
      )}
    </div>
  );
}

function Connector({ label }) {
  return (
    <div className="flex flex-row items-center justify-center gap-2 lg:flex-col lg:gap-3">
      <span className="h-px w-8 bg-gradient-to-r from-transparent via-neutral-700 to-neutral-700 lg:h-8 lg:w-px lg:bg-gradient-to-b" />
      <div className="flex h-7 w-7 items-center justify-center rounded-full border border-amber-500/30 bg-amber-500/5">
        <ArrowRight className="h-3.5 w-3.5 text-amber-400/80 lg:rotate-90" strokeWidth={2} />
      </div>
      <span className="hidden font-mono text-[9px] uppercase tracking-[0.16em] text-neutral-600 lg:block">
        {label}
      </span>
      <span className="h-px w-8 bg-gradient-to-l from-transparent via-neutral-700 to-neutral-700 lg:h-8 lg:w-px lg:bg-gradient-to-t" />
    </div>
  );
}

function Topology({ snapshot, arch }) {
  const c = snapshot.computer;
  const p = snapshot.phone;
  const sl = snapshot.secure_link;
  return (
    <section id="topology" className="reveal px-6 py-12 sm:px-10">
      <SectionHeading
        index="01"
        kicker="System topology"
        title="Local host, secure link, remote worker"
        sub="Three independent domains. The secure link is the only controlled bridge — Android capacity is never merged into the Linux host."
        right={<Pill tone="unavailable" testId="topology-mode">Hosted browser · not reachable</Pill>}
      />

      <Card className="p-6">
        <div className="grid grid-cols-1 items-stretch gap-3 lg:grid-cols-[1fr_auto_1fr_auto_1fr]">
          <EndpointColumn
            icon={Cpu}
            accent="local"
            kind="Local"
            title={c.label}
            subtitle="Orchestrator"
            rows={[
              { k: "phoneboostd", v: c.runtime.state, testId: "kv-phoneboostd" },
              { k: "Local API", v: c.local_api.state, testId: "kv-local-api" },
            ]}
            note={c.note}
          />
          <Connector label="Secure link" />
          <EndpointColumn
            icon={Radio}
            accent="link"
            kind="Bridge"
            title="Secure link"
            subtitle="Noise XX → IK · PBMUX"
            rows={[
              { k: "Transport", v: sl.transport.state, testId: "kv-transport" },
              { k: "Session", v: sl.session.state, testId: "kv-session" },
              { k: "Auth", v: sl.authentication.state, testId: "kv-auth" },
              { k: "Latency", v: sl.latency.state, testId: "kv-latency" },
            ]}
            note="Authenticated, sequenced, quota-bounded framing. Fail-closed by default."
          />
          <Connector label="Remote" />
          <EndpointColumn
            icon={Smartphone}
            accent="remote"
            kind="Remote"
            title={p.label}
            subtitle="Trusted worker"
            rows={[
              { k: "Endpoint", v: p.endpoint.state, testId: "kv-endpoint" },
              { k: "Worker", v: p.worker.state, testId: "kv-worker" },
              { k: "Incarnation", v: p.incarnation.state, testId: "kv-incarnation" },
            ]}
            note={p.health.note}
          />
        </div>
      </Card>

      {/* Remote capability + architecture layers */}
      <div className="mt-6 grid grid-cols-1 gap-6 lg:grid-cols-[1fr_1.4fr]">
        <Card className="p-6" testId="card-remote-capability">
          <div className="mb-4 flex items-center gap-2">
            <Layers className="h-4 w-4 text-neutral-500" strokeWidth={1.75} />
            <h3 className="font-display text-sm font-semibold text-white">Remote capability</h3>
          </div>
          <KV k="Admitted capacity" v={snapshot.remote_capability.admitted_capacity.state} testId="kv-admitted" />
          <KV k="Reserved" v={snapshot.remote_capability.reserved.state} testId="kv-reserved" />
          <KV k="RemoteBuffer" v={snapshot.remote_capability.active_remote_buffer.state} testId="kv-remotebuffer" />
          <KV k="Remote job" v={snapshot.remote_capability.active_remote_job.state} testId="kv-remote-job" />
          <p className="mt-4 border-t border-neutral-800/70 pt-3 text-[11px] leading-relaxed text-neutral-500">
            {snapshot.remote_capability.note}
          </p>
        </Card>

        <Card className="p-6" testId="card-architecture">
          <div className="mb-4 flex items-center justify-between">
            <div className="flex items-center gap-2">
              <Terminal className="h-4 w-4 text-neutral-500" strokeWidth={1.75} />
              <h3 className="font-display text-sm font-semibold text-white">Architecture layers</h3>
            </div>
            <Pill tone="evidence">Repository truth</Pill>
          </div>
          <ol className="space-y-2">
            {arch.layers.map((l, i) => (
              <li
                key={l.layer}
                data-testid={`arch-${i}`}
                className="grid grid-cols-1 items-start gap-2 rounded border border-neutral-800/70 bg-neutral-900/30 p-3 md:grid-cols-[150px_120px_1fr]"
              >
                <div className="font-display text-[13px] font-medium text-neutral-100">{l.layer}</div>
                <div className="font-mono text-[10px] uppercase tracking-[0.14em] text-neutral-500">{l.role}</div>
                <div className="font-mono text-[11px] text-neutral-400">{l.detail}</div>
              </li>
            ))}
          </ol>
        </Card>
      </div>

      {/* Security callout */}
      <Card className="mt-6 p-6" testId="card-security">
        <div className="flex flex-col gap-4 md:flex-row md:items-start md:gap-6">
          <div className="flex items-center gap-3">
            <div className="flex h-11 w-11 items-center justify-center rounded-lg border border-[#39FF14]/30 bg-[#39FF14]/5">
              <ShieldCheck className="h-5 w-5 text-[#39FF14]" strokeWidth={1.75} />
            </div>
          </div>
          <div className="flex-1">
            <div className="mb-1 flex items-center gap-3">
              <h3 className="font-display text-base font-semibold text-white">Security, in plain language</h3>
              <Pill tone="evidence">Plain-language first</Pill>
            </div>
            <p className="text-[15px] leading-relaxed text-neutral-200">
              {snapshot.security_plain_language}
            </p>
            <p className="mt-3 font-mono text-[11px] leading-relaxed text-neutral-500">
              Full 256-bit peer IDs (<span className="text-neutral-300">SHA-256(static_public_key)</span>), Noise XX
              first pair with QR-01A SAS, Noise IK reconnect, PBMUX authenticated framing, fail-closed dispatch,
              worker-authoritative ResourceGuard. Session material, private keys and SAS values are never exposed
              by this UI.
            </p>
          </div>
        </div>
      </Card>
    </section>
  );
}

/* ------------------------------------------------------------------ */
/* Five-gate ladder (independent, non-progress)                        */
/* ------------------------------------------------------------------ */

function GateLadder({ gates }) {
  return (
    <section id="ladder" className="reveal border-t border-neutral-800 px-6 py-12 sm:px-10">
      <SectionHeading
        index="02"
        kicker="Authority model"
        title="Five independent gates"
        sub="Each gate is a distinct authority check — not a sequence and not a progress bar. A peer ID is identity only; it does not authenticate a session, grant a lease, or create capacity."
        right={<Pill tone="warn" testId="ladder-status">All unavailable · no live bridge</Pill>}
      />

      <ol
        data-testid="gate-ladder"
        className="grid grid-cols-1 gap-3 md:grid-cols-2 xl:grid-cols-3"
      >
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
              <span className="font-mono text-[9px] uppercase tracking-[0.16em] text-neutral-600">
                {c.kind}
              </span>
            </div>
            <h3 className="font-display text-[15px] font-semibold leading-snug text-white">
              {c.title}
            </h3>
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
      <div
        className="flex-1 bg-black/70 backdrop-blur-sm"
        onClick={onClose}
      />
      <aside className="w-full max-w-xl overflow-y-auto border-l border-neutral-800 bg-[#0a0a0a] bg-grain p-7 reveal">
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
          <div className="font-mono text-xs uppercase tracking-widest text-neutral-500">
            Loading evidence…
          </div>
        )}
        {detail && (
          <>
            <h3 className="font-display text-xl font-bold tracking-tight text-white">
              {detail.card.title}
            </h3>
            <p className="mt-2 font-mono text-[11px] text-neutral-500">
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
        <h3 className={`font-mono text-[11px] font-semibold uppercase tracking-[0.16em] ${styles.head}`}>
          {title}
        </h3>
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
/* LIVE curtain                                                        */
/* ------------------------------------------------------------------ */

function LiveModeCurtain({ live, onSwitch }) {
  return (
    <section className="reveal px-6 py-10 sm:px-10">
      <Card testId="live-curtain" className="border-amber-500/30 p-8">
        <div className="flex items-center gap-2.5">
          <div className="flex h-10 w-10 items-center justify-center rounded-lg border border-amber-500/30 bg-amber-500/5">
            <Lock className="h-5 w-5 text-amber-400" strokeWidth={1.75} />
          </div>
          <div>
            <h3 className="font-display text-lg font-semibold text-amber-200">
              LIVE mode is unavailable
            </h3>
            <p className="font-mono text-[10px] uppercase tracking-[0.16em] text-amber-400/70">
              No data is fabricated
            </p>
          </div>
        </div>
        <p className="mt-4 max-w-2xl text-sm leading-relaxed text-amber-100/80">{live.reason}</p>
        <ul className="mt-4 space-y-1.5">
          {(live.requirements || []).map((r) => (
            <li key={r} className="flex gap-2.5 text-[13px] text-amber-100/70">
              <span className="mt-2 h-1 w-1 shrink-0 rounded-full bg-amber-400/60" />
              {r}
            </li>
          ))}
        </ul>
        <button
          data-testid="switch-to-recorded"
          onClick={onSwitch}
          className="mt-6 inline-flex items-center gap-1.5 rounded border border-[#39FF14]/40 bg-[#39FF14]/5 px-4 py-2 font-mono text-[11px] font-medium uppercase tracking-[0.14em] text-[#8dff77] transition-colors hover:bg-[#39FF14]/10"
        >
          View recorded evidence <ChevronRight className="h-3.5 w-3.5" />
        </button>
      </Card>
    </section>
  );
}

/* ------------------------------------------------------------------ */
/* Dashboard                                                           */
/* ------------------------------------------------------------------ */

export default function Dashboard({ api, snapshot, live, evidence, roadmap, arch, fixtures, mode, setMode }) {
  const [openEv, setOpenEv] = useState(null);
  const [active, setActive] = useState("overview");
  const [copied, setCopied] = useState(false);
  const liveReachable = !!live?.reachable;

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
    try {
      const p = navigator.clipboard?.writeText(text);
      if (p && typeof p.then === "function") {
        p.then(done).catch(fallbackCopy);
      } else {
        fallbackCopy();
      }
    } catch {
      fallbackCopy();
    }
    function fallbackCopy() {
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
        /* clipboard unavailable in this context; ignore */
      }
      done();
    }
  };

  return (
    <div className="min-h-screen bg-[#0a0a0a] text-neutral-200">
      <Sidebar active={active} snapshot={snapshot} onCopy={onCopy} copied={copied} />

      <div className="lg:pl-60">
        {/* Provenance banner */}
        <div
          className={`sticky top-0 z-20 border-b border-neutral-800 backdrop-blur ${
            mode === "LIVE" ? "bg-amber-500/[0.04]" : "bg-[#0a0a0a]/85"
          }`}
        >
          <div className="flex items-center justify-between px-6 py-2.5 sm:px-10">
            <div className="flex items-center gap-3">
              <Pill tone={mode === "LIVE" ? "warn" : "evidence"} testId="mode-badge">
                {mode === "LIVE" ? "Live · unavailable" : "Recorded evidence"}
              </Pill>
              <span className="hidden font-mono text-[10px] text-neutral-500 md:inline">
                native <span className="text-neutral-300">{snapshot.release.native_baseline.slice(0, 12)}</span>
                {" · "}toolchain <span className="text-neutral-300">{snapshot.release.toolchain}</span>
                {" · "}validated <span className="text-neutral-300">{snapshot.release.validation_date}</span>
              </span>
            </div>
            <div className="font-mono text-[9px] uppercase tracking-[0.2em] text-neutral-600">
              Non-production PoC
            </div>
          </div>
        </div>

        <main>
          <Hero
            snapshot={snapshot}
            mode={mode}
            setMode={setMode}
            liveReachable={liveReachable}
            onCopy={onCopy}
            copied={copied}
          />

          {mode === "LIVE" && !liveReachable && (
            <LiveModeCurtain live={live} onSwitch={() => setMode("RECORDED_EVIDENCE")} />
          )}

          <Topology snapshot={snapshot} arch={arch} />
          <GateLadder gates={snapshot.gates} />
          <EvidenceGrid evidence={evidence} fixtures={fixtures} snapshot={snapshot} onOpen={setOpenEv} />
          <Roadmap roadmap={roadmap} />

          {/* Footer */}
          <footer className="border-t border-neutral-800 px-6 py-10 sm:px-10">
            <div className="flex flex-wrap items-center gap-x-4 gap-y-2 font-mono text-[11px] text-neutral-500">
              <span>
                release <span className="text-neutral-300">{snapshot.release.tag}</span>
              </span>
              <span className="text-neutral-700">·</span>
              <span>
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
              PhoneBoost is a non-production proof of concept for explicit remote resource cooperation
              between a Linux x86-64 host and an Android ARM64 worker. It is not RAM extension, not swap,
              not a CPU illusion, and not a cloud service. This Control Center never fabricates LIVE state.
            </p>
          </footer>
        </main>
      </div>

      <EvidenceDrawer open={!!openEv} onClose={() => setOpenEv(null)} api={api} id={openEv} />
    </div>
  );
}
