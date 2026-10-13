import { act } from "react";
import { createRoot } from "react-dom/client";
import axios from "axios";
import App from "./App";

let mockProps;
const mockRefresh = jest.fn().mockResolvedValue(undefined);
const mockChange = jest.fn();
jest.mock("axios", () => ({ __esModule: true, default: { get: jest.fn(), post: jest.fn() } }));
jest.mock("@/useAutoUsePreference", () => ({ __esModule: true, default: () => ({
  preference: { enabled: true, fresh: true, pending: false }, refresh: mockRefresh, change: mockChange,
}) }));
jest.mock("@/liveBridge", () => ({ ...jest.requireActual("@/liveBridge"),
  consumeBridgeToken: () => "test-capability", normalizeLiveSnapshot: (value) => value,
}));
jest.mock("@/pages/Dashboard", () => ({ __esModule: true, default: (props) => {
  mockProps = props; return null;
} }));
global.IS_REACT_ACT_ENVIRONMENT = true;

test("late readiness from before or during a toggle cannot restore READY", async () => {
  jest.useFakeTimers();
  let releaseStatus; let releaseChange;
  axios.get.mockReturnValueOnce(new Promise((resolve) => { releaseStatus = resolve; }));
  mockChange.mockReturnValue(new Promise((resolve) => { releaseChange = resolve; }));
  const container = document.createElement("div");
  const root = createRoot(container);
  try {
    await act(async () => root.render(<App />));
    let changing;
    act(() => { changing = mockProps.onAutoUse(false); });
    await act(async () => { releaseStatus({ data: { fresh: true, runtime: { staleReady: true } } }); });
    expect(mockProps.live.fresh).toBe(false);
    await act(async () => { releaseChange(); await changing; });
    expect(mockProps.live.fresh).toBe(false);
    axios.get.mockResolvedValue({ data: { fresh: true, runtime: { newEpoch: true } } });
    await act(async () => { jest.advanceTimersByTime(2000); });
    expect(mockProps.live.runtime).toEqual({ newEpoch: true });
  } finally {
    act(() => root.unmount()); container.remove(); jest.useRealTimers();
  }
});
