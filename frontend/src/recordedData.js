const gate = (id, label, explanation) => ({
  id,
  label,
  explanation,
  state: "UNAVAILABLE",
  reason: "No fresh local runtime snapshot; recorded evidence only.",
});

export const RECORDED_SNAPSHOT = {
  provenance: "RECORDED_EVIDENCE",
  mode_label: "Recorded Evidence Mode",
  release: {
    product: "PhoneBoost",
    tag: "master · P0 physical closure",
    head: "b53ea3b84a4085ab45de58385f115f1cbd9176ed",
    native_baseline: "162539c2ec3721f1aa45557900988e2a4291202f",
    toolchain: "Rust 1.98.0",
    validation_date: "2026-09-02",
    repo: "https://github.com/lilugemusikmaking-design/phoneboost",
  },
  computer: {
    label: "Computer (Linux x86-64)",
    runtime: { value: "phoneboostd", state: "NOT_OBSERVED" },
    local_api: { value: "Private 0600 Unix socket + C12", state: "NOT_OBSERVED" },
    note: "Local and Android resources remain separate machines.",
  },
  phone: {
    label: "Android phone (ARM64)",
    endpoint: { value: "Not currently observed", state: "UNKNOWN" },
    worker: { value: "Foreground Rust/JNI worker", state: "UNKNOWN" },
    incarnation: { value: "Not exposed to this view", state: "UNKNOWN" },
    health: {
      value: "Android-local only",
      state: "UNKNOWN",
      note: "No battery, memory, thermal, or latency value is synthesized by this UI.",
    },
  },
  secure_link: {
    transport: { value: "Local IP", state: "UNKNOWN" },
    session: { value: "Noise XX / pinned IK reconnect", state: "UNKNOWN" },
    authentication: { value: "Current session only", state: "UNKNOWN" },
    latency: { value: "Not measured", state: "UNKNOWN" },
  },
  gates: [
    gate("paired", "Paired", "Durable trust after SAS comparison and mutual confirmation."),
    gate("authenticated", "Authenticated", "The current Noise session proves the pinned peer."),
    gate("controller_lease", "Controller lease", "One authenticated controller holds current authority."),
    gate("resource_admissible", "Resource admissible", "Android ResourceGuard approves a specific request."),
    gate("provider_ready", "Provider ready", "The locked native provider can accept this operation."),
  ],
  remote_capability: {
    admitted_capacity: { value: "Not observed", state: "UNKNOWN" },
    reserved: { value: "Not observed", state: "UNKNOWN" },
    active_remote_buffer: { value: "Not exposed by C12", state: "UNKNOWN" },
    active_remote_job: { value: "Not observed", state: "UNKNOWN" },
    note: "The UI does not infer lease or ResourceGuard state from aggregate readiness.",
  },
  controller: {
    lease: { value: "Not exposed by C12", state: "UNKNOWN" },
    resource_guard: { value: "Android authority", state: "UNKNOWN" },
  },
  security_plain_language:
    "The devices compare a six-digit code once; later connections verify the saved device key.",
};

export const RECORDED_EVIDENCE = [
  {
    id: "p0-remote-compute",
    title: "C07–C12 · P0 remote compute physical closure",
    summary:
      "REMOTE_SUCCESS · disconnect UNAVAILABLE · explicit local fallback · NO_FALSE_REMOTE_SUCCESS PASS · authenticated recovery · second REMOTE_SUCCESS",
    provenance: "RECORDED_EVIDENCE",
    kind: "physical",
    source: "docs/evidence/c07-c12-p0-remote-compute-physical.txt",
  },
  {
    id: "p1-live-bridge-local-browser",
    title: "P1 · LIVE Bridge local browser proof",
    summary:
      "LIVE fresh state · REMOTE_SUCCESS · disconnect UNAVAILABLE · LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE · NO_FALSE_REMOTE_SUCCESS PASS · authenticated recovery · second REMOTE_SUCCESS",
    provenance: "RECORDED_EVIDENCE",
    kind: "physical-browser",
    source: "docs/evidence/p1-live-bridge-local-browser-physical.txt",
  },
  {
    id: "workspace-tests",
    title: "Rust workspace tests",
    summary: "352 unit tests + 6 doc-tests PASS at the P0 closure baseline",
    provenance: "RECORDED_EVIDENCE",
    kind: "test-totals",
    source: "docs/competition/IMPLEMENTATION_EVIDENCE.md",
  },
  {
    id: "wire-checkers",
    title: "Locked protocol checkers",
    summary: "C07 PASS · C08/C09 PASS · C10 PASS",
    provenance: "RECORDED_EVIDENCE",
    kind: "protocol-oracle",
    source: "docs/competition/IMPLEMENTATION_EVIDENCE.md",
  },
  {
    id: "android-build",
    title: "Android ARM64 production worker",
    summary: "Release core, isolation scans and debug APK build PASS",
    provenance: "RECORDED_EVIDENCE",
    kind: "build",
    source: "docs/competition/IMPLEMENTATION_EVIDENCE.md",
  },
];

export const RECORDED_ROADMAP = {
  working_now: [
    "Private Linux C12 control API and typed phoneboostctl client",
    "Noise XX pairing and pinned Noise IK production reconnect",
    "Authenticated C07 lease and Android ResourceGuard authority path",
    "Bounded C09 storage and locked pb.native.blake3/1 provider",
    "Physically proven c10-abc-v1 remote compute and truthful fallback",
    "Loopback-only browser bridge implementation with strict live freshness",
    "Physical P1 local browser proof for the locked production BLAKE3 path",
  ],
  next: [],
  future: [
    "Providers beyond the locked BLAKE3 profile",
    "Arbitrary compute inputs",
    "Evidence-backed performance or capacity claims",
  ],
};

export const RECORDED_ARCHITECTURE = {
  provenance: "RECORDED_EVIDENCE",
  layers: [
    { layer: "Browser", role: "Presentation", detail: "Fresh local snapshot or clearly labeled recorded evidence" },
    { layer: "Local bridge", role: "Least privilege", detail: "Loopback-only · capability protected · closed routes" },
    { layer: "phoneboostd / C12", role: "Local authority", detail: "Private Unix socket · peer credentials · bounded framing" },
    { layer: "SecureSession", role: "Transport crypto", detail: "Noise XX first pair · pinned IK reconnect · PBMUX" },
    { layer: "Android worker", role: "Remote authority", detail: "C07 lease · ResourceGuard · C09/C10 provider" },
  ],
};

export const RECORDED_FIXTURES = { provenance: "RECORDED_EVIDENCE", file_count: 31 };

export const HOSTED_LIVE_UNAVAILABLE = {
  provenance: "UNAVAILABLE",
  reachable: false,
  fresh: false,
  reason: "Open the Control Center through the local phoneboost-web-bridge to view LIVE state.",
  runtime: null,
};
