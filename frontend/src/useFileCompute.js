import { useCallback, useEffect, useRef, useState } from "react";
import axios from "axios";
import { FILE_INPUT_LIMIT, normalizeFileResult } from "./liveBridge";

const KNOWN_ERRORS = new Set(["BAD_REQUEST", "CONTENT_LENGTH_REQUIRED", "INPUT_TOO_LARGE", "CONTENT_TYPE_REQUIRED", "UPLOAD_TIMEOUT", "BUSY", "INPUT_STORAGE_UNAVAILABLE", "NATIVE_REQUEST_FAILED", "IMPORT_TIMEOUT", "OUTCOME_UNKNOWN"]);
const empty = { file: null, running: false, result: null, error: null };

export default function useFileCompute(capability, preference, otherRunning) {
  const [state, setState] = useState(empty);
  const selected = useRef(null);
  const generation = useRef(0);
  const pending = useRef(false);
  const alive = useRef(true);
  const abort = useRef(null);
  const select = useCallback((file) => {
    generation.current += 1;
    selected.current = file || null;
    const valid = file && Number.isSafeInteger(file.size) && file.size > 0 && file.size <= FILE_INPUT_LIMIT;
    setState({ file: file || null, running: pending.current, result: null, error: file && !valid ? "INVALID_FILE_SIZE" : null });
  }, []);
  const run = useCallback(async () => {
    const file = selected.current;
    const age = Date.now() - preference.observedAt;
    if (!capability || pending.current || otherRunning || !preference.fresh || preference.pending ||
        preference.enabled !== true || !Number.isFinite(age) || age < 0 || age >= 3000 ||
        !file || !Number.isSafeInteger(file.size) || file.size < 1 || file.size > FILE_INPUT_LIMIT) return;
    const current = ++generation.current;
    pending.current = true;
    abort.current = new AbortController();
    setState({ file, running: true, result: null, error: null });
    try {
      const response = await axios.post("/bridge/v1/compute/blake3-file", file, {
        headers: { "X-PhoneBoost-Bridge-Token": capability, "Content-Type": "application/octet-stream" },
        timeout: 180000, signal: abort.current.signal,
      });
      const result = normalizeFileResult(response.data, file.size);
      if (!result) throw new Error("unconfirmed file result");
      if (alive.current && current === generation.current) setState({ file, running: false, result, error: null });
    } catch (error) {
      const code = error?.response?.data?.error;
      const body = error?.response?.data;
      const safeCode = body && typeof body === "object" && !Array.isArray(body) && Object.keys(body).length === 1 && KNOWN_ERRORS.has(code) ? code : "OUTCOME_UNKNOWN";
      if (alive.current && current === generation.current) setState({ file, running: false, result: null, error: safeCode });
    } finally {
      pending.current = false;
      abort.current = null;
      if (alive.current) setState((value) => ({ ...value, running: false }));
    }
  }, [capability, preference, otherRunning]);
  useEffect(() => {
    alive.current = true;
    return () => {
      alive.current = false; generation.current += 1;
      // Cancels the browser wait only, never claims native operation cancellation.
      abort.current?.abort();
    };
  }, []);
  return { ...state, select, run };
}
