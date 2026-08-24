import { useState } from "react";
import axios from "axios";
import { Cpu, Smartphone, ShieldCheck, ChevronRight, Github, Lock, GitBranch, X, ExternalLink } from "lucide-react";

const GATE_LABELS = {
  paired: "Paired",
  authenticated: "Authenticated",
  controller_lease: "Controller lease",
  resource_admissible: "Resource admissible",
  provider_ready: "Provider ready",
};

function Badge({ children, tone = "neutral", testId }) {
  const tones = {
    neutral: "bg-neutral-800 text-neutral-300 border-neutral-700",
    warn: "bg-amber-500/10 text-amber-300 border-amber-500/30",
    danger: "bg-rose-500/10 text-rose-300 border-rose-500/30",
    info: "bg-sky-500/10 text-sky-300 border-sky-500/30",
    good: "bg-emerald-500/10 text-emerald-300 border-emerald-500/30",
    live: "bg-transparent text-neutral-400 border-neutral-700",
  };
  return (
    <span data-testid={testId} className={`inline-flex items-center gap-1.5 rounded border px-2 py-0.5 text-[10px] font-medium uppercase tracking-widest ${tones[tone]}`}>
      {children}
    </span>
  );
}

function Card({ title, right, children, testId }) {
  return (
    <section data-testid={testId} className="rounded-lg border border-neutral-800 bg-[#0f1216]">
      <header className="flex items-center justify-between border-b border-neutral-800 px-5 py-3">
        <h2 className="text-[11px] font-semibold uppercase tracking-[0.18em] text-neutral-400">{title}</h2>
        {right}
      </header>
      <div className="px-5 py-4">{children}</div>
    </section>
  );
}

function KV({ k, v, mono = true, testId }) {
  return (
    <div className="flex items-baseline justify-between gap-4 py-1.5" data-testid={testId}>
      <span className="text-xs text-neutral-500">{k}</span>
      <span className={`text-xs text-neutral-200 text-right ${mono ? "font-mono" : ""}`}>{v}</span>
    </div>
  );
}

function ModeToggle({ mode, setMode, liveReachable }) {
  return (
    <div className="inline-flex rounded border border-neutral-800 p-0.5 text-[11px] font-medium">
      <button
        data-testid="mode-toggle-live"
        onClick={() => setMode("LIVE")}
        className={`px-3 py-1 rounded uppercase tracking-widest ${mode === "LIVE" ? "bg-neutral-800 text-neutral-100" : "text-neutral-500 hover:text-neutral-300"}`}
      >
        Live{liveReachable ? "" : " ·"}
        {!liveReachable && <span className="ml-1 text-amber-400">unavailable</span>}
      </button>
      <button
        data-testid="mode-toggle-recorded"
        onClick={() => setMode("RECORDED_EVIDENCE")}
        className={`px-3 py-1 rounded uppercase tracking-widest ${mode === "RECORDED_EVIDENCE" ? "bg-neutral-800 text-neutral-100" : "text-neutral-500 hover:text-neutral-300"}`}
      >
        Recorded evidence
      </button>
    </div>
  );
}

function GateLadder({ gates }) {
  return (
    <ol data-testid="gate-ladder" className="space-y-2">
      {gates.map((g, i) => (
        <li key={g.id} data-testid={`gate-${g.id}`} className="flex items-start gap-3 rounded border border-neutral-800/60 bg-neutral-900/40 p-3">
          <div className="mt-0.5 flex h-5 w-5 items-center justify-center rounded-full border border-neutral-700 text-[10px] text-neutral-400">
            {i + 1}
          </div>
          <div className="flex-1">
            <div className="flex items-center gap-2">
              <span className="text-sm text-neutral-200">{GATE_LABELS[g.id] || g.label}</span>
              <Badge tone="warn" testId={`gate-${g.id}-badge`}>{g.state}</Badge>
            </div>
            <p className="mt-1 text-xs text-neutral-500">{g.explanation}</p>
            <p className="mt-1 text-[11px] text-neutral-600 italic">{g.reason}</p>
          </div>
        </li>
      ))}
    </ol>
  );
}

function EndpointPanel({ title, icon: Icon, testId, children }) {
  return (
    <div data-testid={testId} className="rounded-lg border border-neutral-800 bg-[#0f1216] p-5">
      <div className="mb-3 flex items-center gap-2">
        <Icon className="h-4 w-4 text-neutral-400" />
        <h3 className="text-sm font-semibold text-neutral-200">{title}</h3>
      </div>
      {children}
    </div>
  );
}

function LiveModeCurtain({ live, onSwitch }) {
  return (
    <div data-testid="live-curtain" className="rounded-lg border border-amber-500/30 bg-amber-500/5 p-5">
      <div className="flex items-center gap-2">
        <Lock className="h-4 w-4 text-amber-400" />
        <h3 className="text-sm font-semibold text-amber-200">LIVE mode is unavailable</h3>
      </div>
      <p className="mt-2 text-xs text-amber-100/80">{live.reason}</p>
      <ul className="mt-3 list-disc pl-5 text-[11px] text-amber-100/70 space-y-0.5">
        {(live.requirements || []).map((r) => <li key={r}>{r}</li>)}
      </ul>
      <button
        data-testid="switch-to-recorded"
        onClick={onSwitch}
        className="mt-4 inline-flex items-center gap-1 rounded border border-amber-500/40 px-3 py-1 text-[11px] font-medium uppercase tracking-widest text-amber-200 hover:bg-amber-500/10"
      >
        Switch to recorded evidence <ChevronRight className="h-3 w-3" />
      </button>
      <p className="mt-3 text-[10px] uppercase tracking-widest text-amber-300/70">
        No fallback data is generated. Nothing on this screen is fabricated as LIVE.
      </p>
    </div>
  );
}

function EvidenceDrawer({ open, onClose, api, id }) {
  const [detail, setDetail] = useState(null);
  const [loading, setLoading] = useState(false);
  useState(() => {}); // no-op to appease lint

  // load on open
  if (open && id && !detail && !loading) {
    setLoading(true);
    axios.get(`${api.base}/evidence/${id}`).then((r) => { setDetail(r.data); setLoading(false); }).catch(() => setLoading(false));
  }
  if (!open) return null;
  return (
    <div className="fixed inset-0 z-40 flex" data-testid="evidence-drawer">
      <div className="flex-1 bg-black/60" onClick={() => { onClose(); setDetail(null); }} />
      <aside className="w-full max-w-xl overflow-y-auto border-l border-neutral-800 bg-[#0b0d10] p-6">
        <div className="mb-4 flex items-center justify-between">
          <Badge tone="info">Recorded Evidence</Badge>
          <button data-testid="evidence-drawer-close" onClick={() => { onClose(); setDetail(null); }} className="text-neutral-500 hover:text-neutral-200"><X className="h-4 w-4" /></button>
        </div>
        {!detail && <div className="text-xs font-mono text-neutral-500">Loading…</div>}
        {detail && (
          <>
            <h3 className="text-lg font-semibold text-neutral-100">{detail.card.title}</h3>
            <p className="mt-1 text-xs text-neutral-500">Source: <span className="font-mono">{detail.card.source}</span></p>
            <p className="mt-3 text-sm text-neutral-300">{detail.card.summary}</p>
            {detail.detail && (
              <pre className="mt-4 max-h-72 overflow-auto rounded border border-neutral-800 bg-neutral-950 p-3 text-[11px] leading-relaxed text-neutral-300">{JSON.stringify(detail.detail, null, 2)}</pre>
            )}
            {detail.raw && (
              <>
                <div className="mt-4 text-[10px] uppercase tracking-widest text-neutral-500">Raw evidence (redacted at source)</div>
                <pre className="mt-1 max-h-96 overflow-auto rounded border border-neutral-800 bg-neutral-950 p-3 text-[11px] leading-relaxed text-neutral-300">{detail.raw}</pre>
              </>
            )}
          </>
        )}
      </aside>
    </div>
  );
}

export default function Dashboard({ api, snapshot, live, evidence, roadmap, arch, fixtures, mode, setMode }) {
  const [openEv, setOpenEv] = useState(null);
  const liveReachable = !!live?.reachable;

  return (
    <div className="min-h-screen bg-[#0b0d10] text-neutral-200">
      {/* Header */}
      <header className="border-b border-neutral-800/80 bg-[#0b0d10]/80 backdrop-blur">
        <div className="mx-auto flex max-w-7xl items-center justify-between px-6 py-4">
          <div className="flex items-center gap-3">
            <div className="flex h-8 w-8 items-center justify-center rounded border border-neutral-700 font-mono text-xs text-neutral-300">PB</div>
            <div>
              <div className="text-sm font-semibold tracking-wide text-neutral-100">PhoneBoost</div>
              <div className="text-[10px] uppercase tracking-[0.22em] text-neutral-500">Use the power already in your pocket</div>
            </div>
          </div>
          <div className="flex items-center gap-3">
            <a
              data-testid="repo-link"
              href={snapshot.release.repo}
              target="_blank" rel="noreferrer"
              className="hidden md:inline-flex items-center gap-1 rounded border border-neutral-800 px-2 py-1 text-[11px] text-neutral-400 hover:text-neutral-200"
            >
              <Github className="h-3 w-3" /> {snapshot.release.tag}
            </a>
            <ModeToggle mode={mode} setMode={setMode} liveReachable={liveReachable} />
          </div>
        </div>
      </header>

      {/* Provenance banner */}
      <div className={`border-b border-neutral-800 ${mode === "LIVE" ? "bg-amber-500/5" : "bg-neutral-900/40"}`}>
        <div className="mx-auto flex max-w-7xl items-center justify-between px-6 py-2">
          <div className="flex items-center gap-3">
            <Badge tone={mode === "LIVE" ? "warn" : "info"} testId="mode-badge">
              {mode === "LIVE" ? "LIVE · unavailable" : "Recorded evidence"}
            </Badge>
            <span className="text-[11px] text-neutral-500">
              Native baseline <span className="font-mono text-neutral-300">{snapshot.release.native_baseline.slice(0, 12)}</span>
              {" · "}toolchain <span className="font-mono text-neutral-300">{snapshot.release.toolchain}</span>
              {" · "}validated <span className="font-mono text-neutral-300">{snapshot.release.validation_date}</span>
            </span>
          </div>
          <div className="hidden md:block text-[10px] uppercase tracking-widest text-neutral-600">Non-production proof of concept</div>
        </div>
      </div>

      <main className="mx-auto max-w-7xl px-6 py-6 space-y-6">
        {/* Hero row: two endpoints */}
        <section className="grid grid-cols-1 gap-4 md:grid-cols-[1fr_auto_1fr]">
          <EndpointPanel title={snapshot.computer.label} icon={Cpu} testId="panel-computer">
            <KV k="phoneboostd" v={snapshot.computer.runtime.state} testId="kv-phoneboostd" />
            <KV k="Local API" v={snapshot.computer.local_api.state} testId="kv-local-api" />
            <p className="mt-2 text-[11px] text-neutral-500">{snapshot.computer.note}</p>
          </EndpointPanel>
          <div className="hidden md:flex items-center justify-center">
            <div className="flex flex-col items-center text-neutral-500">
              <div className="h-px w-16 bg-neutral-800" />
              <div className="my-2 text-[10px] uppercase tracking-widest">Secure link</div>
              <Badge tone="warn" testId="badge-secure-link">{snapshot.secure_link.session.state}</Badge>
              <div className="mt-2 text-[10px] font-mono text-neutral-600">Noise XX → IK · PBMUX</div>
              <div className="h-px w-16 bg-neutral-800 mt-2" />
            </div>
          </div>
          <EndpointPanel title={snapshot.phone.label} icon={Smartphone} testId="panel-phone">
            <KV k="Endpoint" v={snapshot.phone.endpoint.state} testId="kv-endpoint" />
            <KV k="Worker" v={snapshot.phone.worker.state} testId="kv-worker" />
            <KV k="Incarnation" v={snapshot.phone.incarnation.state} testId="kv-incarnation" />
            <p className="mt-2 text-[11px] text-neutral-500">{snapshot.phone.health.note}</p>
          </EndpointPanel>
        </section>

        {/* LIVE curtain if mode LIVE */}
        {mode === "LIVE" && !liveReachable && (
          <LiveModeCurtain live={live} onSwitch={() => setMode("RECORDED_EVIDENCE")} />
        )}

        {/* Gate ladder + Secure link + Remote capability */}
        <section className="grid grid-cols-1 gap-4 lg:grid-cols-3">
          <div className="lg:col-span-2">
            <Card title="State ladder — five independent gates" testId="card-gates" right={<Badge tone="info">Recorded evidence</Badge>}>
              <GateLadder gates={snapshot.gates} />
              <p className="mt-3 text-[11px] text-neutral-500">
                A peer ID is <em>identity only</em>. It does not authenticate a session, grant a lease, or create capacity.
              </p>
            </Card>
          </div>
          <div className="space-y-4">
            <Card title="Secure link" testId="card-secure-link">
              <KV k="Transport" v={snapshot.secure_link.transport.state} testId="kv-transport" />
              <KV k="Session" v={snapshot.secure_link.session.state} testId="kv-session" />
              <KV k="Auth" v={snapshot.secure_link.authentication.state} testId="kv-auth" />
              <KV k="Latency" v={snapshot.secure_link.latency.state} testId="kv-latency" />
            </Card>
            <Card title="Remote capability" testId="card-remote-capability">
              <KV k="Admitted capacity" v={snapshot.remote_capability.admitted_capacity.state} testId="kv-admitted" />
              <KV k="Reserved" v={snapshot.remote_capability.reserved.state} testId="kv-reserved" />
              <KV k="RemoteBuffer" v={snapshot.remote_capability.active_remote_buffer.state} testId="kv-remotebuffer" />
              <KV k="Remote job" v={snapshot.remote_capability.active_remote_job.state} testId="kv-remote-job" />
              <p className="mt-2 text-[11px] text-neutral-500">{snapshot.remote_capability.note}</p>
            </Card>
          </div>
        </section>

        {/* Security callout */}
        <Card
          title="Security"
          testId="card-security"
          right={<Badge tone="info">Plain-language first</Badge>}
        >
          <div className="flex items-start gap-3">
            <ShieldCheck className="mt-0.5 h-5 w-5 text-emerald-400" />
            <div>
              <p className="text-sm text-neutral-100">{snapshot.security_plain_language}</p>
              <p className="mt-1 text-[11px] text-neutral-500">
                Full 256-bit peer IDs (<span className="font-mono">SHA-256(static_public_key)</span>), Noise XX first pair with QR-01A SAS,
                Noise IK reconnect, PBMUX authenticated framing, fail-closed dispatch, worker-authoritative ResourceGuard. Session material,
                private keys, and SAS values are never exposed by this UI.
              </p>
            </div>
          </div>
        </Card>

        {/* Evidence grid */}
        <section>
          <div className="mb-3 flex items-center justify-between">
            <h2 className="text-[11px] font-semibold uppercase tracking-[0.22em] text-neutral-400">Technical evidence</h2>
            <div className="flex items-center gap-2 text-[11px] text-neutral-500">
              <GitBranch className="h-3 w-3" /> repo <span className="font-mono">{snapshot.release.head.slice(0, 12)}</span>
              {fixtures && <span className="ml-3">Fixtures manifest: <span className="font-mono text-neutral-300">{fixtures.file_count}</span> files</span>}
            </div>
          </div>
          <div className="grid grid-cols-1 gap-3 md:grid-cols-2 lg:grid-cols-3">
            {evidence.map((c) => (
              <button
                key={c.id}
                data-testid={`evidence-card-${c.id}`}
                onClick={() => setOpenEv(c.id)}
                className="text-left rounded-lg border border-neutral-800 bg-[#0f1216] p-4 hover:border-neutral-700 transition-colors"
              >
                <div className="mb-2 flex items-center justify-between">
                  <Badge tone="info">Recorded evidence</Badge>
                  <span className="text-[10px] uppercase tracking-widest text-neutral-600">{c.kind}</span>
                </div>
                <div className="text-sm font-semibold text-neutral-100">{c.title}</div>
                <p className="mt-1 text-xs text-neutral-400">{c.summary}</p>
                <div className="mt-3 flex items-center justify-between text-[10px] text-neutral-600">
                  <span className="font-mono truncate">{c.source}</span>
                  <ChevronRight className="h-3 w-3" />
                </div>
              </button>
            ))}
          </div>
        </section>

        {/* Architecture */}
        <Card title="Architecture" testId="card-architecture" right={<Badge tone="info">Repository truth</Badge>}>
          <ol className="space-y-2">
            {arch.layers.map((l, i) => (
              <li key={l.layer} data-testid={`arch-${i}`} className="grid grid-cols-1 md:grid-cols-[180px_140px_1fr] items-start gap-3 rounded border border-neutral-800/60 bg-neutral-900/30 p-3">
                <div className="text-sm text-neutral-100">{l.layer}</div>
                <div className="text-[11px] uppercase tracking-widest text-neutral-500">{l.role}</div>
                <div className="text-xs text-neutral-400 font-mono">{l.detail}</div>
              </li>
            ))}
          </ol>
        </Card>

        {/* Roadmap */}
        <Card title="Roadmap" testId="card-roadmap" right={<span className="text-[10px] uppercase tracking-widest text-neutral-500">Working now · Next · Future</span>}>
          <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
            <div>
              <div className="mb-2 text-[10px] uppercase tracking-widest text-emerald-400">Working / Demonstrated now</div>
              <ul className="space-y-1.5" data-testid="roadmap-working">
                {roadmap.working_now.map((x) => <li key={x} className="text-xs text-neutral-300">• {x}</li>)}
              </ul>
            </div>
            <div>
              <div className="mb-2 text-[10px] uppercase tracking-widest text-sky-400">Next</div>
              <ul className="space-y-1.5" data-testid="roadmap-next">
                {roadmap.next.map((x) => <li key={x} className="text-xs text-neutral-300">• {x}</li>)}
              </ul>
            </div>
            <div>
              <div className="mb-2 text-[10px] uppercase tracking-widest text-neutral-500">Future</div>
              <ul className="space-y-1.5" data-testid="roadmap-future">
                {roadmap.future.map((x) => <li key={x} className="text-xs text-neutral-400">• {x}</li>)}
              </ul>
            </div>
          </div>
        </Card>

        {/* Footer */}
        <footer className="pt-4 pb-8 text-[11px] text-neutral-500">
          <div className="flex flex-wrap items-center gap-4">
            <span>Release <span className="font-mono text-neutral-300">{snapshot.release.tag}</span></span>
            <span>·</span>
            <span>HEAD <span className="font-mono text-neutral-300">{snapshot.release.head}</span></span>
            <span>·</span>
            <a href={snapshot.release.repo} target="_blank" rel="noreferrer" className="inline-flex items-center gap-1 text-neutral-400 hover:text-neutral-200">
              Repository <ExternalLink className="h-3 w-3" />
            </a>
          </div>
          <p className="mt-2 max-w-3xl">
            PhoneBoost is a non-production proof of concept for explicit remote resource cooperation between a Linux x86-64 host and an Android ARM64 worker.
            It is not RAM extension, not swap, not a CPU illusion, and not a cloud service. This Control Center never fabricates LIVE state.
          </p>
        </footer>
      </main>

      <EvidenceDrawer open={!!openEv} onClose={() => setOpenEv(null)} api={api} id={openEv} />
    </div>
  );
}
