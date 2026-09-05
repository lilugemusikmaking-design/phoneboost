import {
  RECORDED_ARCHITECTURE,
  RECORDED_EVIDENCE,
  RECORDED_FIXTURES,
  RECORDED_ROADMAP,
  RECORDED_SNAPSHOT,
} from "./recordedData";

function objectOrFallback(value, fallback) {
  return value && typeof value === "object" && !Array.isArray(value) ? value : fallback;
}

const GATE_IDS = [
  "paired",
  "authenticated",
  "controller_lease",
  "resource_admissible",
  "provider_ready",
];

function isNonEmptyString(value) {
  return typeof value === "string" && value.length > 0;
}

function hasStringFields(value, fields) {
  return Boolean(objectOrFallback(value, null)) && fields.every((field) => isNonEmptyString(value[field]));
}

function isRecordedState(value, requiresNote = false) {
  const fields = requiresNote ? ["value", "state", "note"] : ["value", "state"];
  return hasStringFields(value, fields);
}

function isRecordedGate(value) {
  return hasStringFields(value, ["id", "label", "explanation", "state", "reason"]);
}

function isRecordedSnapshot(value) {
  if (!objectOrFallback(value, null)) return false;
  const gates = value.gates;
  const gateIds = Array.isArray(gates) ? gates.map((gate) => gate?.id) : [];
  return (
    value.provenance === "RECORDED_EVIDENCE" &&
    isNonEmptyString(value.mode_label) &&
    hasStringFields(value.release, ["product", "tag", "head", "native_baseline", "toolchain", "validation_date", "repo"]) &&
    hasStringFields(value.computer, ["label", "note"]) &&
    isRecordedState(value.computer.runtime) &&
    isRecordedState(value.computer.local_api) &&
    hasStringFields(value.phone, ["label"]) &&
    isRecordedState(value.phone.endpoint) &&
    isRecordedState(value.phone.worker) &&
    isRecordedState(value.phone.incarnation) &&
    isRecordedState(value.phone.health, true) &&
    isRecordedState(value.secure_link?.transport) &&
    isRecordedState(value.secure_link?.session) &&
    isRecordedState(value.secure_link?.authentication) &&
    isRecordedState(value.secure_link?.latency) &&
    Array.isArray(gates) &&
    gates.length === GATE_IDS.length &&
    gates.every(isRecordedGate) &&
    GATE_IDS.every((id) => gateIds.includes(id)) &&
    isRecordedState(value.remote_capability?.admitted_capacity) &&
    isRecordedState(value.remote_capability?.reserved) &&
    isRecordedState(value.remote_capability?.active_remote_buffer) &&
    isRecordedState(value.remote_capability?.active_remote_job) &&
    isNonEmptyString(value.remote_capability?.note) &&
    isRecordedState(value.controller?.lease) &&
    isRecordedState(value.controller?.resource_guard) &&
    isNonEmptyString(value.security_plain_language)
  );
}

function isRecordedEvidence(value) {
  return Array.isArray(value?.items) && value.items.every((item) =>
    objectOrFallback(item, null) && typeof item.id === "string" && typeof item.title === "string"
  );
}

function isRecordedRoadmap(value) {
  return ["working_now", "next", "future"].every((key) => Array.isArray(value?.[key]));
}

function isRecordedArchitecture(value) {
  return Array.isArray(value?.layers);
}

function isRecordedFixtures(value) {
  return Number.isSafeInteger(value?.file_count) && value.file_count >= 0;
}

export function safeRecordedPayloads({ snapshot, evidence, roadmap, arch, fixtures } = {}) {
  return {
    snapshot: isRecordedSnapshot(snapshot) ? snapshot : RECORDED_SNAPSHOT,
    evidence: isRecordedEvidence(evidence) ? evidence.items : RECORDED_EVIDENCE,
    roadmap: isRecordedRoadmap(roadmap) ? roadmap : RECORDED_ROADMAP,
    arch: isRecordedArchitecture(arch) ? arch : RECORDED_ARCHITECTURE,
    fixtures: isRecordedFixtures(fixtures) ? fixtures : RECORDED_FIXTURES,
  };
}
