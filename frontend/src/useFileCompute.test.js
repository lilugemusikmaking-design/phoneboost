import { act } from "react";
import { createRoot } from "react-dom/client";
import axios from "axios";
import useFileCompute from "./useFileCompute";
import FileComputePanel from "./FileComputePanel";
import { FILE_INPUT_LIMIT, normalizeFileResult } from "./liveBridge";

jest.mock("axios", () => ({ __esModule: true, default: { post: jest.fn() } }));
global.IS_REACT_ACT_ENVIRONMENT = true;
let latest; let root; let container;
const on = () => ({ fresh: true, pending: false, enabled: true, observedAt: Date.now() });
const file = () => new File(["abc"], "<private-name>.txt");
const result = (source = "REMOTE_SUCCESS", reason = "READY") => ({ state: "COMPLETED", op_id: "pb.native.blake3/1", input_kind: "PRIVATE_IMPORT_V1", input_bytes: 3, digest_blake3_hex: "a".repeat(64), execution_source: source, auto_use_reason: reason });
function Probe({ token = "token", preference = on(), other = false }) {
  latest = useFileCompute(token, preference, other);
  return token ? <FileComputePanel task={latest} preference={preference} otherRunning={other} language="fr" /> : null;
}
beforeEach(() => {
  container = document.createElement("div"); root = createRoot(container);
  axios.post.mockReset();
});
afterEach(() => { act(() => root.unmount()); container.remove(); });

test("raw File is sent once with capability and bounded wait, with no filename metadata", async () => {
  axios.post.mockResolvedValue({ data: result() });
  act(() => root.render(<Probe />));
  const input = file(); act(() => latest.select(input));
  await act(async () => latest.run());
  expect(axios.post).toHaveBeenCalledTimes(1);
  const [path, body, options] = axios.post.mock.calls[0];
  expect(path).toBe("/bridge/v1/compute/blake3-file"); expect(body).toBe(input);
  expect(options.headers).toEqual({ "X-PhoneBoost-Bridge-Token": "token", "Content-Type": "application/octet-stream" });
  expect(options.timeout).toBe(180000);
  expect(latest.result).toEqual(result());
  expect(container.textContent).toContain("Calcul distant confirmé");
  expect(container.querySelector("private-name")).toBe(null);
});

test.each(["LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE", "LOCAL_FALLBACK_AFTER_AMBIGUOUS_REMOTE"])("%s remains labelled local fallback", async (source) => {
  axios.post.mockResolvedValue({ data: result(source, "TRANSPORT_LOST") });
  act(() => root.render(<Probe />)); act(() => latest.select(file()));
  await act(async () => latest.run());
  expect(container.textContent).toContain("Fallback local confirmé");
  expect(container.textContent).not.toContain("Calcul distant confirmé");
});

test("bad size, hosted, OFF, pending, expired and concurrent actions cannot submit", async () => {
  for (const props of [{ token: null }, { preference: { ...on(), enabled: false } },
    { preference: { ...on(), pending: true } }, { preference: { ...on(), fresh: false } },
    { preference: { ...on(), observedAt: Date.now() - 3001 } }, { other: true }]) {
    act(() => root.render(<Probe {...props} />)); act(() => latest.select(file()));
    await act(async () => latest.run());
  }
  act(() => root.render(<Probe />));
  for (const input of [null, new File([], "empty"), { name: "large", size: FILE_INPUT_LIMIT + 1 }]) {
    act(() => latest.select(input)); await act(async () => latest.run());
  }
  expect(axios.post).not.toHaveBeenCalled();
  act(() => root.render(<Probe token={null} />));
  expect(container.querySelector('input[type="file"]')).toBe(null);
});

test("lost or malformed reply clears previous success and never replays", async () => {
  act(() => root.render(<Probe />)); act(() => latest.select(file()));
  axios.post.mockResolvedValueOnce({ data: result() });
  await act(async () => latest.run()); expect(latest.result).not.toBe(null);
  axios.post.mockRejectedValueOnce(new Error("connection lost"));
  await act(async () => latest.run());
  expect(latest.result).toBe(null); expect(latest.error).toBe("OUTCOME_UNKNOWN");
  expect(container.textContent).toContain("Le calcul peut avoir eu lieu");
  axios.post.mockResolvedValueOnce({ data: { ...result(), input_bytes: 4 } });
  await act(async () => latest.run());
  expect(latest.error).toBe("OUTCOME_UNKNOWN"); expect(axios.post).toHaveBeenCalledTimes(3);
});

test("selection change discards old result without allowing overlapping upload", async () => {
  let release; axios.post.mockReturnValue(new Promise((resolve) => { release = resolve; }));
  act(() => root.render(<Probe />)); act(() => latest.select(file()));
  let pending;
  act(() => { pending = latest.run(); latest.run(); });
  const second = new File(["other"], "second.bin");
  act(() => latest.select(second));
  expect(latest.running).toBe(true); expect(latest.result).toBe(null);
  await act(async () => latest.run()); expect(axios.post).toHaveBeenCalledTimes(1);
  await act(async () => { release({ data: result() }); await pending; });
  expect(latest.file).toBe(second); expect(latest.result).toBe(null); expect(latest.running).toBe(false);
});

test("unmount aborts browser wait only and fresh mount does not replay", async () => {
  let release; axios.post.mockReturnValue(new Promise((resolve) => { release = resolve; }));
  act(() => root.render(<Probe />)); act(() => latest.select(file()));
  let pending; act(() => { pending = latest.run(); });
  const signal = axios.post.mock.calls[0][2].signal;
  act(() => root.render(null)); expect(signal.aborted).toBe(true);
  await act(async () => { release({ data: result() }); await pending; });
  act(() => root.render(<Probe />));
  expect(latest.file).toBe(null); expect(latest.result).toBe(null);
  expect(axios.post).toHaveBeenCalledTimes(1);
});

test("only closed error codes are displayed, never arbitrary server text", async () => {
  act(() => root.render(<Probe />)); act(() => latest.select(file()));
  for (const [body, expected] of [[{ error: "BUSY" }, "BUSY"], [{ error: "secret path" }, "OUTCOME_UNKNOWN"],
    [{ error: "BUSY", path: "/private" }, "OUTCOME_UNKNOWN"]]) {
    axios.post.mockRejectedValueOnce({ response: { data: body } });
    await act(async () => latest.run()); expect(latest.error).toBe(expected);
    expect(container.textContent).not.toContain("/private"); expect(container.textContent).not.toContain("secret path");
  }
});

test("terminal schema is exact and rejects forged source, shape and input identity", () => {
  for (const value of [null, [], {}, { ...result(), state: "PENDING" }, { ...result(), op_id: "shell" },
    { ...result(), input_kind: "PATH" }, { ...result(), input_bytes: 4 }, { ...result(), digest_blake3_hex: "x".repeat(64) },
    { ...result(), execution_source: "LOCAL" }, { ...result(), auto_use_reason: "OFF" }, { ...result(), path: "/private" }]) {
    expect(normalizeFileResult(value, 3)).toBe(null);
  }
  expect(normalizeFileResult(result(), 3)).toEqual(result());
});
