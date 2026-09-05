import {
  consumeBridgeToken,
  expireLiveState,
  normalizeComputeResult,
  normalizeLiveSnapshot,
} from "./liveBridge";

function snapshot(overrides = {}) {
  return {
    provenance: "LIVE",
    observed_at_unix_ms: 10_000,
    max_age_ms: 3000,
    local_daemon: { state: "REACHABLE", runtime_state: "READY", local_api_state: "ACTIVE" },
    discovery: { state: "UNKNOWN", reason: "NOT_EXPOSED_BY_C12" },
    authenticated_session: { state: "AUTHENTICATED", remote_worker_state: "AUTHENTICATED" },
    controller_lease: { state: "UNKNOWN", reason: "NOT_EXPOSED_BY_C12" },
    resource_guard: { state: "UNKNOWN", reason: "NOT_EXPOSED_BY_C12" },
    provider_readiness: { provider: "pb.native.blake3/1", state: "AVAILABLE" },
    auto_use: { state: "AVAILABLE", reason: "READY" },
    remote_blake3_available: true,
    last_execution: null,
    ...overrides,
  };
}

test("capability is accepted only as 256-bit lowercase hex and removed from the URL", () => {
  const replaceState = jest.fn();
  const token = "a".repeat(64);
  expect(
    consumeBridgeToken(
      { hash: `#token=${token}`, pathname: "/", search: "" },
      { replaceState }
    )
  ).toBe(token);
  expect(replaceState).toHaveBeenCalledWith(null, "", "/");
  expect(
    consumeBridgeToken(
      { hash: "#token=not-a-capability", pathname: "/", search: "" },
      { replaceState }
    )
  ).toBeNull();
  expect(
    consumeBridgeToken(
      { hash: `#token=${token}&extra=1`, pathname: "/", search: "" },
      { replaceState }
    )
  ).toBeNull();
  expect(
    consumeBridgeToken(
      { hash: `#token=${token}&token=${token}`, pathname: "/", search: "" },
      { replaceState }
    )
  ).toBeNull();
});

test("fresh native snapshot is LIVE and independently preserves unknown gates", () => {
  const state = normalizeLiveSnapshot(snapshot(), 11_000);
  expect(state.provenance).toBe("LIVE");
  expect(state.runtime.authenticated_session.state).toBe("AUTHENTICATED");
  expect(state.runtime.controller_lease.state).toBe("UNKNOWN");
  expect(state.runtime.resource_guard.state).toBe("UNKNOWN");
});

test("old, malformed, or inconsistent snapshots are never LIVE", () => {
  expect(normalizeLiveSnapshot(snapshot(), 13_001).provenance).toBe("UNAVAILABLE");
  expect(normalizeLiveSnapshot(snapshot({ provenance: "RECORDED_EVIDENCE" }), 10_000).provenance).toBe(
    "UNAVAILABLE"
  );
  expect(
    normalizeLiveSnapshot(snapshot({ provider_readiness: { state: "AVAILABLE" }, remote_blake3_available: false }), 10_000)
      .provenance
  ).toBe("UNAVAILABLE");
  expect(
    normalizeLiveSnapshot(snapshot({ auto_use: { state: "AVAILABLE", reason: "NO_DEVICE" } }), 10_000)
      .provenance
  ).toBe("UNAVAILABLE");
  expect(
    normalizeLiveSnapshot(
      snapshot({ authenticated_session: { state: "AUTHENTICATED", remote_worker_state: "NOT_CONFIGURED" } }),
      10_000
    ).provenance
  ).toBe("UNAVAILABLE");
});

test("a previously fresh snapshot visibly expires", () => {
  const live = normalizeLiveSnapshot(snapshot(), 10_100);
  expect(expireLiveState(live, 12_999).provenance).toBe("LIVE");
  const expired = expireLiveState(live, 13_001);
  expect(expired.provenance).toBe("UNAVAILABLE");
  expect(expired.reason).toBe("STALE_RUNTIME_SNAPSHOT");
});

test.each([
  "REMOTE_SUCCESS",
  "LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE",
  "LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE",
])("compute source %s remains exact", (executionSource) => {
  const result = normalizeComputeResult({
    provenance: "LIVE",
    observed_at_unix_ms: 10_000,
    fixture: "c10-abc-v1",
    input_bytes: 3,
    digest_blake3_hex: "6".repeat(64),
    execution_source: executionSource,
    auto_use_reason: "READY",
  }, 10_000);
  expect(result.execution_source).toBe(executionSource);
});

test("unknown compute source is rejected instead of relabeled", () => {
  expect(
    normalizeComputeResult({
      provenance: "LIVE",
      observed_at_unix_ms: 10_000,
      fixture: "c10-abc-v1",
      input_bytes: 3,
      digest_blake3_hex: "6".repeat(64),
      execution_source: "SUCCESS",
      auto_use_reason: "READY",
    }, 10_000)
  ).toBeNull();
});

test("stale compute result and false remote success reason are rejected", () => {
  const result = {
    provenance: "LIVE",
    observed_at_unix_ms: 10_000,
    fixture: "c10-abc-v1",
    input_bytes: 3,
    digest_blake3_hex: "6".repeat(64),
    execution_source: "REMOTE_SUCCESS",
    auto_use_reason: "READY",
  };
  expect(normalizeComputeResult(result, 13_001)).toBeNull();
  expect(normalizeComputeResult({ ...result, auto_use_reason: "TRANSPORT_LOST" }, 10_000)).toBeNull();
});
