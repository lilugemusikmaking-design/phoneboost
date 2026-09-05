import { act } from "react";
import { createRoot } from "react-dom/client";
import axios from "axios";
import {
  EvidenceDrawer,
  LiveControl,
  Sidebar,
  liveBadgeTone,
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
