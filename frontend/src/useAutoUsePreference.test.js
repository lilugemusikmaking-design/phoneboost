import { act } from "react";
import { createRoot } from "react-dom/client";
import axios from "axios";
import useAutoUsePreference, { normalizePreference } from "./useAutoUsePreference";

jest.mock("axios", () => ({ __esModule: true, default: { get: jest.fn(), post: jest.fn() } }));
global.IS_REACT_ACT_ENVIRONMENT = true;
let latest;
let root;
let container;
function Probe({ token = "token" }) { latest = useAutoUsePreference(token); return null; }
function deferred() { let resolve; const promise = new Promise((done) => { resolve = done; }); return { promise, resolve }; }
beforeEach(() => {
  jest.useFakeTimers();
  container = document.createElement("div");
  root = createRoot(container);
  axios.get.mockReset(); axios.post.mockReset();
});
afterEach(() => { act(() => root.unmount()); jest.useRealTimers(); container.remove(); });

test("normalization refuses malformed, contradictory and stale preference", () => {
  const now = Date.now();
  expect(normalizePreference({ enabled: false, persistence: "SAVED" }, now).fresh).toBe(true);
  for (const value of [null, [], {}, { enabled: 0, persistence: "SAVED" },
    { enabled: true, persistence: "ERROR" }, { enabled: false, persistence: "DEFAULT" },
    { enabled: true, persistence: "READY" }, { enabled: true, persistence: "SAVED", extra: 1 }]) {
    expect(normalizePreference(value, now).enabled).toBe(null);
  }
  expect(normalizePreference({ enabled: false, persistence: "SAVED" }, now - 3000).fresh).toBe(false);
});

test("no capability never imports browser preference or calls the daemon", async () => {
  window.localStorage.setItem("phoneboost.participation-enabled", "false");
  act(() => root.render(<Probe token={null} />));
  await act(async () => { await latest.refresh(); await latest.change(false); });
  expect(latest.preference.enabled).toBe(null);
  expect(axios.get).not.toHaveBeenCalled(); expect(axios.post).not.toHaveBeenCalled();
  window.localStorage.clear();
});

test("pending mutation invalidates old reads and only confirmed OFF is displayed", async () => {
  const oldRead = deferred(); const write = deferred();
  axios.get.mockReturnValueOnce(oldRead.promise).mockResolvedValue({ data: { enabled: false, persistence: "SAVED" } });
  axios.post.mockReturnValue(write.promise);
  act(() => root.render(<Probe />));
  let reading; let writing;
  act(() => { reading = latest.refresh(); writing = latest.change(false); });
  expect(latest.preference.pending).toBe(true); expect(latest.preference.enabled).toBe(null);
  await act(async () => { oldRead.resolve({ data: { enabled: true, persistence: "SAVED" } }); await reading; });
  expect(latest.preference.pending).toBe(true);
  await act(async () => { write.resolve({ data: { enabled: false, persistence: "SAVED" } }); await writing; });
  expect(latest.preference.enabled).toBe(false); expect(latest.preference.fresh).toBe(true);
  expect(axios.post).toHaveBeenCalledTimes(1);
  expect(axios.post.mock.calls[0][1]).toEqual({ enabled: false });
});

test("failed mutation reconciles by read without replay; ERROR stays explicit", async () => {
  axios.post.mockRejectedValue(new Error("lost reply"));
  axios.get.mockResolvedValue({ data: { enabled: false, persistence: "ERROR" } });
  act(() => root.render(<Probe />));
  await act(async () => latest.change(false));
  expect(latest.preference.persistence).toBe("ERROR");
  expect(latest.preference.enabled).toBe(false);
  act(() => jest.advanceTimersByTime(3100));
  expect(latest.preference.fresh).toBe(false); expect(latest.preference.enabled).toBe(null);
  expect(axios.post).toHaveBeenCalledTimes(1);
});

test("newer reads win and fresh values from another client replace previous preference", async () => {
  const first = deferred(); const second = deferred();
  axios.get.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
  act(() => root.render(<Probe />));
  let readOne; let readTwo;
  act(() => { readOne = latest.refresh(); readTwo = latest.refresh(); });
  await act(async () => { second.resolve({ data: { enabled: false, persistence: "SAVED" } }); await readTwo; });
  await act(async () => { first.resolve({ data: { enabled: true, persistence: "SAVED" } }); await readOne; });
  expect(latest.preference.enabled).toBe(false);
  axios.get.mockResolvedValue({ data: { enabled: true, persistence: "SAVED" } });
  await act(async () => latest.refresh());
  expect(latest.preference.enabled).toBe(true);
});

test("failed reads and unconfirmed mutation responses never create an OFF claim", async () => {
  axios.get.mockRejectedValue(new Error("offline"));
  axios.post.mockResolvedValue({ data: { enabled: true, persistence: "SAVED" } });
  act(() => root.render(<Probe />));
  await act(async () => latest.change(false));
  expect(latest.preference.fresh).toBe(false); expect(latest.preference.enabled).toBe(null);
  expect(axios.post).toHaveBeenCalledTimes(1);
});
