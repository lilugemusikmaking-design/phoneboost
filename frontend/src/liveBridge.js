const TOKEN_PATTERN = /^[0-9a-f]{64}$/;
const EXECUTION_SOURCES = new Set([
  "REMOTE_SUCCESS",
  "LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE",
  "LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE",
]);
const AUTO_USE_STATES = new Set([
  "OFF",
  "DISCOVERING",
  "CONNECTING",
  "AUTHENTICATING",
  "ACQUIRING_AUTHORITY",
  "CHECKING_READINESS",
  "AVAILABLE",
  "DEGRADED",
  "RECONNECTING",
  "UNAVAILABLE",
]);
const AUTO_USE_REASONS = new Set([
  "OFF",
  "NO_DEVICE",
  "NOT_PAIRED",
  "AUTH_FAILED",
  "LEASE_UNAVAILABLE",
  "WORKER_UNHEALTHY",
  "RESOURCE_REFUSED",
  "TRANSPORT_LOST",
  "RECONNECTING",
  "DISCOVERY_BACKEND_UNAVAILABLE",
  "READY",
]);
const DISCOVERY_PAIRS = new Set([
  "FRESH_HINT/C04_CANDIDATE_OBSERVED",
  "NO_HINT/C04_NO_CANDIDATE",
  "BACKEND_UNAVAILABLE/DISCOVERY_BACKEND_UNAVAILABLE",
  "STALE/OBSERVATION_EXPIRED",
  "UNKNOWN/EPOCH_INVALIDATED",
  "UNKNOWN/NOT_OBSERVED",
  "UNKNOWN/NOT_EXPOSED_BY_C12",
]);
const CONTROLLER_LEASE_PAIRS = new Set([
  "ACTIVE/C07_ACK_FRESH",
  "EXPIRED/ACK_TTL_ELAPSED",
  "UNAVAILABLE/C07_ACQUIRE_FAILED",
  "UNAVAILABLE/C07_RENEW_FAILED",
  "UNAVAILABLE/SESSION_INVALIDATED",
  "UNAVAILABLE/IDENTITY_OR_INCARNATION_CHANGED",
  "UNAVAILABLE/AUTO_USE_DISABLED",
  "UNKNOWN/NOT_OBSERVED",
  "UNKNOWN/NOT_EXPOSED_BY_C12",
]);
const ADMISSION_PROOF_PAIRS = new Set([
  "FRESH_PASS/C08_C09_C10_PROBE_PASSED",
  "FAILED/C08_C09_C10_PROBE_FAILED",
  "STALE/PROOF_EXPIRED",
  "UNKNOWN/SESSION_INVALIDATED",
  "UNKNOWN/LEASE_INVALIDATED",
  "UNKNOWN/IDENTITY_OR_INCARNATION_CHANGED",
  "UNKNOWN/AUTO_USE_DISABLED",
  "UNKNOWN/NOT_OBSERVED",
  "UNKNOWN/NOT_EXPOSED_BY_C12",
]);

export function consumeBridgeToken(locationObject, historyObject) {
  const hash = locationObject?.hash || "";
  const params = new URLSearchParams(hash.startsWith("#") ? hash.slice(1) : hash);
  const entries = [...params.entries()];
  const token = entries.length === 1 && entries[0][0] === "token" ? entries[0][1] : null;
  if (params.has("token") && historyObject?.replaceState) {
    historyObject.replaceState(null, "", `${locationObject.pathname || "/"}${locationObject.search || ""}`);
  }
  return token && TOKEN_PATTERN.test(token) ? token : null;
}

export function unavailableLive(reason, previousRuntime = null) {
  return {
    provenance: "UNAVAILABLE",
    reachable: false,
    fresh: false,
    reason,
    runtime: previousRuntime,
  };
}

function stringAt(value, ...path) {
  let current = value;
  for (const segment of path) current = current?.[segment];
  return typeof current === "string" ? current : null;
}

function hasExactPair(value, path, pairs) {
  const state = stringAt(value, ...path, "state");
  const reason = stringAt(value, ...path, "reason");
  return Boolean(state && reason && pairs.has(`${state}/${reason}`));
}

export function normalizeLiveSnapshot(value, now = Date.now()) {
  const observed = value?.observed_at_unix_ms;
  const maxAge = value?.max_age_ms;
  const remoteAvailable = value?.remote_blake3_available;
  const remoteWorkerState = stringAt(value, "authenticated_session", "remote_worker_state");
  const authenticatedState = stringAt(value, "authenticated_session", "state");
  const autoUseState = stringAt(value, "auto_use", "state");
  const autoUseReason = stringAt(value, "auto_use", "reason");
  const requiredStrings = [
    stringAt(value, "local_daemon", "state"),
    stringAt(value, "local_daemon", "runtime_state"),
    stringAt(value, "local_daemon", "local_api_state"),
    stringAt(value, "authenticated_session", "state"),
    stringAt(value, "authenticated_session", "remote_worker_state"),
    stringAt(value, "provider_readiness", "state"),
    stringAt(value, "auto_use", "state"),
    stringAt(value, "auto_use", "reason"),
  ];
  const valid =
    value?.provenance === "LIVE" &&
    Number.isSafeInteger(observed) &&
    Number.isSafeInteger(maxAge) &&
    maxAge > 0 &&
    maxAge <= 3000 &&
    observed <= now + 1000 &&
    now - observed <= maxAge &&
    typeof remoteAvailable === "boolean" &&
    requiredStrings.every(Boolean) &&
    value.local_daemon.state === "REACHABLE" &&
    hasExactPair(value, ["discovery_observation"], DISCOVERY_PAIRS) &&
    hasExactPair(value, ["controller_lease"], CONTROLLER_LEASE_PAIRS) &&
    hasExactPair(value, ["resource_guard_admission_proof"], ADMISSION_PROOF_PAIRS) &&
    authenticatedState === (remoteWorkerState === "AUTHENTICATED" ? "AUTHENTICATED" : "UNAVAILABLE") &&
    AUTO_USE_STATES.has(autoUseState) &&
    AUTO_USE_REASONS.has(autoUseReason) &&
    remoteAvailable === (autoUseState === "AVAILABLE" && autoUseReason === "READY") &&
    value.provider_readiness.provider === "pb.native.blake3/1" &&
    value.provider_readiness.state === (remoteAvailable ? "AVAILABLE" : "UNAVAILABLE");
  return valid
    ? { provenance: "LIVE", reachable: true, fresh: true, reason: null, runtime: value }
    : unavailableLive("INVALID_OR_STALE_RUNTIME_SNAPSHOT");
}

export function expireLiveState(state, now = Date.now()) {
  if (!state?.fresh || !state.runtime) return state;
  const { observed_at_unix_ms: observed, max_age_ms: maxAge } = state.runtime;
  return now - observed <= maxAge
    ? state
    : unavailableLive("STALE_RUNTIME_SNAPSHOT", state.runtime);
}

export function normalizeComputeResult(value, now = Date.now()) {
  const observed = value?.observed_at_unix_ms;
  const source = value?.execution_source;
  const reason = value?.auto_use_reason;
  const valid =
    value?.provenance === "LIVE" &&
    value?.fixture === "c10-abc-v1" &&
    value?.input_bytes === 3 &&
    typeof value?.digest_blake3_hex === "string" &&
    /^[0-9a-f]{64}$/.test(value.digest_blake3_hex) &&
    EXECUTION_SOURCES.has(source) &&
    AUTO_USE_REASONS.has(reason) &&
    (source !== "REMOTE_SUCCESS" || reason === "READY") &&
    Number.isSafeInteger(observed) &&
    observed <= now + 1000 &&
    now - observed <= 3000;
  return valid ? value : null;
}
