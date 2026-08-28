# Emergent competition handoff

## Objective

Build a restrained PhoneBoost control center for judges without changing or
misrepresenting the native core. Inspect the repository and
`IMPLEMENTATION_EVIDENCE.md` before authoring the presentation layer. Native
protocol constants, Noise behavior, authority boundaries, and fail-closed
semantics are fixed inputs, not UI design material.

PhoneBoost means explicit cooperation between a Linux x86-64 host and an
Android ARM64 worker. Describe remote capacity as a service or remote object.
Never say or imply that phone RAM becomes Linux RAM, swap grows, or ARM CPU is
pooled transparently.

## Required provenance model

Every displayed fact must carry one of these sources:

- **LIVE** — obtained from a genuinely reachable native runtime endpoint.
- **RECORDED EVIDENCE** — loaded from checked-in tests, fixtures, protocol
  vectors, or physical-run evidence.
- **ROADMAP** — clearly labeled as not implemented.

Use a data-source abstraction with no silent fallback:

```ts
type Provenance = "LIVE" | "RECORDED_EVIDENCE" | "ROADMAP";

interface PhoneBoostDataSource {
  readonly provenance: Provenance;
  getSystemSnapshot(): Promise<SystemSnapshot>;
  getEvidenceIndex(): Promise<EvidenceItem[]>;
}

class LiveRuntime implements PhoneBoostDataSource { /* native bridge only */ }
class RecordedEvidence implements PhoneBoostDataSource { /* checked-in facts */ }
```

If `LiveRuntime` is unreachable, show an unavailable/offline state and offer a
separately labeled evidence view. Never make `RecordedEvidence` implement or
masquerade as the live runtime, and never generate plausible sample values.
Browser code must not imply it can pair, acquire a lease, reserve resources, or
run native work unless a real Linux/Android bridge performs and confirms that
operation.

## State model the UI must preserve

Show these as separate gates, in this order:

1. **Paired** — durable static-key trust exists after XX, SAS comparison, mutual
   confirmation, and successful commit.
2. **Authenticated** — the current Noise session proves the pinned peer for
   this connection.
3. **Controller lease** — one authenticated controller holds a current lease
   for the current worker incarnation.
4. **Resource admissible** — fresh Android-local health and `ResourceGuard`
   policy permit a specific reservation.
5. **Provider ready** — a concrete provider has committed resources and can
   accept the requested operation.

Do not collapse these gates into a single green “connected” or “ready” badge.
A peer ID is identity only; displaying one must never imply authentication or
lease authority. Local host capacity and remote Android capacity must remain
visually distinct.

## Recommended control-center structure

- **Overview:** host runtime, Android endpoint, transport, provenance badge,
  and a five-gate state ladder. Empty/unavailable states are valid outcomes.
- **Resources:** local observations and remote Android observations in separate
  panels. Describe available memory as an observation, not memory gained by
  Linux. Show reservation/admission only when reported by `ResourceGuard`.
- **Security:** plain-language explanation — “The devices compare a six-digit
  code once; later connections verify the saved device key.” Put Noise mode,
  full peer ID, PBMUX sequence, worker incarnation, and lease details behind a
  judge-facing details panel.
- **Evidence:** current test totals, C07 fixture/oracle results, Android build
  results, and links to checked-in evidence. Mark this entire view
  `RECORDED EVIDENCE`.
- **Roadmap:** production authorization seam, lease-command execution,
  `RemoteBuffer`, compute providers, and any benchmark/capacity claims.

Prefer a serious, compact visual system: neutral surfaces, one restrained
accent, readable state labels, and evidence-first details. Avoid sci-fi glow,
animated network theater, decorative device swarms, or numbers with no source.
Installer polish is secondary to truthful state and evidence presentation.

## Hard prohibitions

- Do not invent connected devices, `READY` state, throughput, latency, memory
  gains, health values, security state, lease state, or provider availability.
- Do not add mock native behavior and present it as live. Fixtures may be shown
  only as `RECORDED EVIDENCE`.
- Do not change locked protocol fields/constants, Noise prologue/SAS behavior,
  PBMUX semantics, worker authority, or volatile-loss rules.
- Do not expose private keys, session material, SAS values, fixture secrets, or
  unredacted diagnostics.
- Do not add accounts, payments, a cloud control plane, AI product features, or
  unrelated platform architecture for competition polish.
- Do not claim the C05-to-C07 authorization bridge, lease mutation path,
  `RemoteBuffer`, or compute providers are complete.

## Judge-demo acceptance checks

- The selected data source and provenance are visible at all times.
- Disconnecting or losing the bridge produces an explicit unavailable/offline
  state; it never switches to generated data.
- Paired, authenticated, leased, admissible, and provider-ready states can be
  independently absent.
- Local and remote resources cannot be mistaken for one address space.
- Every metric links to a live timestamp/source or a checked-in evidence item.
- Technical evidence is inspectable without overwhelming the default view.
- The demo remains truthful when no phone is attached and no lease exists.
