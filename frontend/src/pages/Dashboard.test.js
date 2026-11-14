import { act } from "react";
import { createRoot } from "react-dom/client";
import axios from "axios";
import Dashboard, {
  ConnectionHelp,
  EvidenceDrawer,
  LiveControl,
  Sidebar,
  liveBadgeTone,
  runtimeSummary,
} from "./Dashboard";
import { copyFor } from "../i18n";

jest.mock("axios", () => ({
  __esModule: true,
  default: { get: jest.fn() },
}));

global.IS_REACT_ACT_ENVIRONMENT = true;

function mount(element) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root = createRoot(container);
  act(() => root.render(element));
  return {
    container,
    root,
    unmount() {
      act(() => root.unmount());
      container.remove();
    },
  };
}

const evidenceOne = {
  id: "one",
  title: "First proof",
  summary: "first summary",
  source: "docs/evidence/one.txt",
};
const evidenceTwo = {
  id: "two",
  title: "Second proof",
  summary: "second summary",
  source: "docs/evidence/two.txt",
};

afterEach(() => {
  axios.get.mockReset();
  document.body.innerHTML = "";
});

const missingPhone = {
  provenance: "LIVE", fresh: true,
  runtime: { auto_use: { state: "UNAVAILABLE", reason: "NO_DEVICE" } },
};

test.each([
  ["fr", "Modem USB", "il peut se désactiver", "ne confirme pas la cause"],
  ["en", "USB tethering", "may turn it off", "does not confirm the cause"],
])("missing-phone help is conditional guidance in %s, not a diagnosis or action", (language, usb, warning, caution) => {
  const view = mount(<ConnectionHelp live={missingPhone} enabled language={language} />);
  expect(view.container.textContent).toContain(usb);
  expect(view.container.textContent).toContain(warning);
  expect(view.container.textContent).toContain(caution);
  expect(view.container.textContent).toContain("Wi-Fi");
  expect(view.container.querySelector('[role="note"]')).not.toBeNull();
  expect(view.container.querySelector("button")).toBeNull();
  expect(axios.get).not.toHaveBeenCalled();
  view.unmount();
});

test.each([
  ["OFF", missingPhone, false],
  ["unconfirmed preference", missingPhone, null],
  ["stale snapshot", { ...missingPhone, fresh: false }, true],
  ["recorded snapshot", { ...missingPhone, provenance: "RECORDED" }, true],
  ["absent snapshot", null, true],
  ["missing runtime", { ...missingPhone, runtime: null }, true],
  ["resource refusal", { ...missingPhone, runtime: { auto_use: { reason: "RESOURCE_REFUSED" } } }, true],
  ["healthy despite expired discovery", { ...missingPhone, runtime: { auto_use: { state: "AVAILABLE", reason: "READY" }, discovery_observation: { state: "STALE" } } }, true],
])("connection help is absent for %s", (_label, live, enabled) => {
  const view = mount(<ConnectionHelp live={live} enabled={enabled} language="fr" />);
  expect(view.container.textContent).toBe("");
  view.unmount();
});

test("Dashboard shows connection guidance and removes it on fresh recovery", () => {
  const props = {
    api: { base: null }, snapshot: {}, evidence: [], roadmap: {}, arch: {}, fixtures: {},
    compute: { running: false }, language: "fr", setLanguage: jest.fn(),
    preference: { fresh: true, enabled: true, pending: false },
  };
  const view = mount(<Dashboard {...props} live={missingPhone} />);
  expect(view.container.querySelector('[data-testid="connection-help"]')).not.toBeNull();
  const recovered = { ...missingPhone, runtime: { auto_use: { state: "AVAILABLE", reason: "READY" } } };
  act(() => view.root.render(<Dashboard {...props} live={recovered} />));
  expect(view.container.querySelector('[data-testid="connection-help"]')).toBeNull();
  view.unmount();
});

test("navigation landmark has a navigation label rather than a section label", () => {
  const t = copyFor("en");
  const view = mount(<Sidebar active="overview" language="en" setLanguage={() => {}} t={t} />);

  expect(view.container.querySelector("nav").getAttribute("aria-label")).toBe(t.labels.primaryNavigation);
  expect(view.container.querySelector("nav").getAttribute("aria-label")).not.toBe(t.navigation.overview);
  view.unmount();
});

test("runtime absence and compute failure render distinct localized messages", () => {
  const t = copyFor("fr");
  const view = mount(
    <LiveControl
      live={{ fresh: false, runtime: null }}
      compute={{ running: false, result: null, error: "COMPUTE_ACTION_UNAVAILABLE" }}
      onCompute={() => {}}
      language="fr"
      t={t}
    />
  );

  expect(view.container.textContent).toContain(t.labels.runtimeUnavailable);
  expect(view.container.textContent).toContain(t.labels.computeError);
  expect(view.container.textContent).not.toContain("COMPUTE_ACTION_UNAVAILABLE");
  view.unmount();
});

test("LIVE badge tone follows fresh availability state", () => {
  expect(liveBadgeTone({ fresh: true })).toContain("text-primary");
  expect(liveBadgeTone({ fresh: false })).toContain("text-amber-200");
});

test("participation preference never fabricates runtime readiness", () => {
  expect(runtimeSummary({ fresh: true, runtime: {} }, null)).toEqual({ state: "UNAVAILABLE", ready: false });
  expect(runtimeSummary({ fresh: false, runtime: null }, true)).toEqual({
    state: "OFFLINE",
    ready: false,
  });
  expect(runtimeSummary({ fresh: true, runtime: {
    authenticated_session: { state: "UNAVAILABLE" },
    controller_lease: { state: "UNAVAILABLE" },
    resource_guard_admission_proof: { state: "FAILED" },
    remote_blake3_available: false,
    provider_readiness: { state: "UNAVAILABLE" },
    auto_use: { state: "AVAILABLE", reason: "READY" },
  } }, true)).toEqual({ state: "AVAILABLE", ready: false });
  expect(runtimeSummary({ fresh: true, runtime: {
    authenticated_session: { state: "AUTHENTICATED" },
    controller_lease: { state: "ACTIVE" },
    resource_guard_admission_proof: { state: "FRESH_PASS" },
    remote_blake3_available: true,
    provider_readiness: { state: "AVAILABLE" },
    auto_use: { state: "AVAILABLE", reason: "READY" },
  } }, false)).toEqual({ state: "DISABLED", ready: false });
  expect(runtimeSummary({ fresh: true, runtime: {
    authenticated_session: { state: "AUTHENTICATED" },
    controller_lease: { state: "ACTIVE" },
    resource_guard_admission_proof: { state: "FRESH_PASS" },
    remote_blake3_available: true,
    provider_readiness: { state: "AVAILABLE" },
    auto_use: { state: "AVAILABLE", reason: "READY" },
  } }, true)).toEqual({ state: "READY", ready: true });
});

test("evidence drawer clears old content when its evidence item changes", async () => {
  const t = copyFor("en");
  let resolveFirst;
  axios.get.mockImplementationOnce(() => new Promise((resolve) => { resolveFirst = resolve; }));
  axios.get.mockImplementationOnce(() => new Promise(() => {}));
  const onClose = jest.fn();
  const view = mount(<EvidenceDrawer item={evidenceOne} api={{ base: "/api" }} t={t} onClose={onClose} />);

  await act(async () => resolveFirst({ data: { proof: "first" } }));
  expect(view.container.textContent).toContain("first");

  act(() => view.root.render(<EvidenceDrawer item={evidenceTwo} api={{ base: "/api" }} t={t} onClose={onClose} />));
  expect(view.container.textContent).toContain(t.evidence.loading);
  expect(view.container.textContent).not.toContain('"proof": "first"');
  view.unmount();
});

test("evidence drawer focuses its close control and closes on Escape", () => {
  const t = copyFor("en");
  const trigger = document.createElement("button");
  document.body.appendChild(trigger);
  trigger.focus();
  const onClose = jest.fn();
  const view = mount(<EvidenceDrawer item={evidenceOne} api={{ base: null }} t={t} onClose={onClose} />);
  const close = view.container.querySelector('[data-testid="evidence-drawer-close"]');

  expect(document.activeElement).toBe(close);
  act(() => document.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true })));
  expect(onClose).toHaveBeenCalledTimes(1);
  view.unmount();
  expect(document.activeElement).toBe(trigger);
  trigger.remove();
});
