import { safeRecordedPayloads } from "./recordedApi";
import {
  RECORDED_ARCHITECTURE,
  RECORDED_EVIDENCE,
  RECORDED_FIXTURES,
  RECORDED_ROADMAP,
  RECORDED_SNAPSHOT,
} from "./recordedData";

function completeSnapshot() {
  return JSON.parse(JSON.stringify(RECORDED_SNAPSHOT));
}

function validOtherPayloads() {
  const evidence = { items: [{ id: "proof", title: "Proof" }] };
  const roadmap = { working_now: [], next: [], future: [] };
  const arch = { layers: [] };
  const fixtures = { file_count: 0 };
  return { evidence, roadmap, arch, fixtures };
}

function changeAt(snapshot, path, value) {
  let parent = snapshot;
  path.slice(0, -1).forEach((segment) => {
    parent = parent[segment];
  });
  parent[path[path.length - 1]] = value;
}

function removeAt(snapshot, path) {
  let parent = snapshot;
  path.slice(0, -1).forEach((segment) => {
    parent = parent[segment];
  });
  delete parent[path[path.length - 1]];
}

function pathName(path) {
  return path.join(".");
}

const releaseStringPaths = [
  ["release", "product"],
  ["release", "tag"],
  ["release", "head"],
  ["release", "native_baseline"],
  ["release", "toolchain"],
  ["release", "validation_date"],
  ["release", "repo"],
];

const stateStringPaths = [
  ["computer", "runtime", "value"],
  ["computer", "runtime", "state"],
  ["computer", "local_api", "value"],
  ["computer", "local_api", "state"],
  ["phone", "endpoint", "value"],
  ["phone", "endpoint", "state"],
  ["phone", "worker", "value"],
  ["phone", "worker", "state"],
  ["phone", "incarnation", "value"],
  ["phone", "incarnation", "state"],
  ["phone", "health", "value"],
  ["phone", "health", "state"],
  ["phone", "health", "note"],
  ["secure_link", "transport", "value"],
  ["secure_link", "transport", "state"],
  ["secure_link", "session", "value"],
  ["secure_link", "session", "state"],
  ["secure_link", "authentication", "value"],
  ["secure_link", "authentication", "state"],
  ["secure_link", "latency", "value"],
  ["secure_link", "latency", "state"],
  ["remote_capability", "admitted_capacity", "value"],
  ["remote_capability", "admitted_capacity", "state"],
  ["remote_capability", "reserved", "value"],
  ["remote_capability", "reserved", "state"],
  ["remote_capability", "active_remote_buffer", "value"],
  ["remote_capability", "active_remote_buffer", "state"],
  ["remote_capability", "active_remote_job", "value"],
  ["remote_capability", "active_remote_job", "state"],
  ["controller", "lease", "value"],
  ["controller", "lease", "state"],
  ["controller", "resource_guard", "value"],
  ["controller", "resource_guard", "state"],
];

const snapshotStringPaths = [
  ["mode_label"],
  ...releaseStringPaths,
  ["computer", "label"],
  ["computer", "note"],
  ["phone", "label"],
  ...stateStringPaths,
  ["remote_capability", "note"],
  ["security_plain_language"],
];

const objectPaths = [
  ["release"],
  ["computer"],
  ["computer", "runtime"],
  ["computer", "local_api"],
  ["phone"],
  ["phone", "endpoint"],
  ["phone", "worker"],
  ["phone", "incarnation"],
  ["phone", "health"],
  ["secure_link"],
  ["secure_link", "transport"],
  ["secure_link", "session"],
  ["secure_link", "authentication"],
  ["secure_link", "latency"],
  ["remote_capability"],
  ["remote_capability", "admitted_capacity"],
  ["remote_capability", "reserved"],
  ["remote_capability", "active_remote_buffer"],
  ["remote_capability", "active_remote_job"],
  ["controller"],
  ["controller", "lease"],
  ["controller", "resource_guard"],
  ["gates"],
  ["gates", 0],
];

const gateFieldNames = ["id", "label", "explanation", "state", "reason"];
const gateStringPaths = Array.from({ length: 5 }, (_, index) =>
  gateFieldNames.map((field) => ["gates", index, field])
).flat();

const rejectionCases = [
  ["null root", () => null],
  ["array root", () => []],
  ["empty root object", () => ({})],
  ["missing provenance", (snapshot) => removeAt(snapshot, ["provenance"])],
  ["non-string provenance", (snapshot) => changeAt(snapshot, ["provenance"], false)],
  ["non-canonical provenance", (snapshot) => changeAt(snapshot, ["provenance"], "LIVE")],
  ...objectPaths.flatMap((path) => [
    [`missing object ${pathName(path)}`, (snapshot) => removeAt(snapshot, path)],
    [`null object ${pathName(path)}`, (snapshot) => changeAt(snapshot, path, null)],
    [`array object ${pathName(path)}`, (snapshot) => changeAt(snapshot, path, [])],
  ]),
  ...snapshotStringPaths.flatMap((path) => [
    [`missing required string ${pathName(path)}`, (snapshot) => removeAt(snapshot, path)],
    [`wrong type for required string ${pathName(path)}`, (snapshot) => changeAt(snapshot, path, false)],
    [`empty required string ${pathName(path)}`, (snapshot) => changeAt(snapshot, path, "")],
  ]),
  ...gateStringPaths.flatMap((path) => [
    [`missing gate field ${pathName(path)}`, (snapshot) => removeAt(snapshot, path)],
    [`wrong gate field type ${pathName(path)}`, (snapshot) => changeAt(snapshot, path, false)],
    [`empty gate field ${pathName(path)}`, (snapshot) => changeAt(snapshot, path, "")],
  ]),
  ["non-array gates", (snapshot) => changeAt(snapshot, ["gates"], {})],
  ["too few gates", (snapshot) => { snapshot.gates.pop(); }],
  ["too many gates", (snapshot) => { snapshot.gates.push({ ...snapshot.gates[0] }); }],
  ["unknown gate identifier", (snapshot) => changeAt(snapshot, ["gates", 0, "id"], "unknown")],
  ["duplicate gate identifier", (snapshot) => changeAt(snapshot, ["gates", 0, "id"], snapshot.gates[1].id)],
];

test("complete valid recorded snapshot is accepted", () => {
  const snapshot = completeSnapshot();
  const safe = safeRecordedPayloads({ snapshot });

  expect(safe.snapshot).toBe(snapshot);
});

test.each(rejectionCases)("recorded snapshot rejection: %s", (_name, mutate) => {
  const snapshot = completeSnapshot();
  const candidate = mutate(snapshot);

  expect(safeRecordedPayloads({ snapshot: candidate === undefined ? snapshot : candidate }).snapshot).toBe(
    RECORDED_SNAPSHOT
  );
});

test("malformed recorded payloads each retain their own safe fallback", () => {
  const safe = safeRecordedPayloads({
    snapshot: completeSnapshot(),
    evidence: { items: [null] },
    roadmap: { working_now: [] },
    arch: { layers: "not-an-array" },
    fixtures: { file_count: -1 },
  });

  expect(safe.snapshot).not.toBe(RECORDED_SNAPSHOT);
  expect(safe.evidence).toBe(RECORDED_EVIDENCE);
  expect(safe.roadmap).toBe(RECORDED_ROADMAP);
  expect(safe.arch).toBe(RECORDED_ARCHITECTURE);
  expect(safe.fixtures).toBe(RECORDED_FIXTURES);
});

test("other valid recorded payloads remain independently usable", () => {
  const { evidence, roadmap, arch, fixtures } = validOtherPayloads();

  expect(safeRecordedPayloads({ snapshot: null, evidence, roadmap, arch, fixtures })).toEqual({
    snapshot: RECORDED_SNAPSHOT,
    evidence: evidence.items,
    roadmap,
    arch,
    fixtures,
  });
});
