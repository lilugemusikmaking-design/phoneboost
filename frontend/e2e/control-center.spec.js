const { test, expect } = require("@playwright/test");

const TOKEN = "a".repeat(64);

function runtime(overrides = {}) {
  return {
    provenance: "LIVE",
    observed_at_unix_ms: Date.now(),
    max_age_ms: 3000,
    local_daemon: { state: "REACHABLE", runtime_state: "READY", local_api_state: "ACTIVE" },
    discovery_observation: { state: "NO_HINT", reason: "C04_NO_CANDIDATE" },
    authenticated_session: { state: "UNAVAILABLE", remote_worker_state: "NOT_CONFIGURED" },
    controller_lease: { state: "UNAVAILABLE", reason: "C07_ACQUIRE_FAILED" },
    resource_guard_admission_proof: { state: "FAILED", reason: "C08_C09_C10_PROBE_FAILED" },
    provider_readiness: { provider: "pb.native.blake3/1", state: "UNAVAILABLE" },
    auto_use: { state: "RECONNECTING", reason: "RECONNECTING" },
    remote_blake3_available: false,
    last_execution: null,
    ...overrides,
  };
}

async function openWithRuntime(page, snapshot) {
  let enabled = true;
  await page.route("**/bridge/v1/auto-use", async (route) => {
    if (route.request().method() === "POST") enabled = route.request().postDataJSON().enabled;
    await route.fulfill({ json: { enabled, persistence: "SAVED" } });
  });
  await page.route("**/bridge/v1/snapshot", async (route) => {
    await route.fulfill({ json: { ...snapshot, observed_at_unix_ms: Date.now() } });
  });
  await page.goto(`/#token=${TOKEN}`);
}

test.beforeEach(async ({ page }) => {
  await page.addInitScript(() => window.localStorage.clear());
});

test("opens the Control Center without page errors", async ({ page }) => {
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "PhoneBoost Control Center" })).toBeVisible();
  await expect(page.getByRole("navigation", { name: "Navigation principale" })).toBeVisible();
  expect(errors).toEqual([]);
  await expect(page.getByTestId("file-compute-panel")).toHaveCount(0);
});

test("selected file sends raw bytes without filename and shows native fallback", async ({ page }, testInfo) => {
  let calls = 0;
  await page.route("**/bridge/v1/compute/blake3-file", async (route) => {
    calls += 1;
    expect(route.request().postDataBuffer().equals(Buffer.from("abc"))).toBe(true);
    expect(route.request().headers()["content-type"]).toBe("application/octet-stream");
    expect(route.request().headers()["x-phoneboost-bridge-token"]).toBe(TOKEN);
    await route.fulfill({ json: { state: "COMPLETED", op_id: "pb.native.blake3/1", input_kind: "PRIVATE_IMPORT_V1",
      input_bytes: 3, digest_blake3_hex: "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85",
      execution_source: "LOCAL_FALLBACK_AFTER_REMOTE_UNAVAILABLE", auto_use_reason: "NO_DEVICE" } });
  });
  await openWithRuntime(page, runtime());
  await page.getByTestId("file-compute-input").setInputFiles({ name: "private-name.txt", mimeType: "text/plain", buffer: Buffer.from("abc") });
  await page.getByTestId("run-file-compute").click();
  await expect(page.getByTestId("file-compute-result")).toContainText("Fallback local confirmé");
  await expect(page.getByTestId("file-compute-result")).not.toContainText("REMOTE_SUCCESS");
  await page.getByTestId("file-compute-panel").screenshot({ path: testInfo.outputPath("file-compute-panel.png") });
  await page.getByTestId("file-compute-input").setInputFiles({ name: "other.txt", mimeType: "text/plain", buffer: Buffer.from("other") });
  await expect(page.getByTestId("file-compute-result")).toHaveCount(0);
  expect(calls).toBe(1);
});

test("file loss of reply stays unknown and reload never resubmits", async ({ page }) => {
  let calls = 0;
  await page.route("**/bridge/v1/compute/blake3-file", async (route) => { calls += 1; await route.abort("failed"); });
  await openWithRuntime(page, runtime());
  await page.getByTestId("file-compute-input").setInputFiles({ name: "input.bin", mimeType: "application/octet-stream", buffer: Buffer.from("abc") });
  await page.getByTestId("run-file-compute").click();
  await expect(page.getByTestId("file-compute-error")).toContainText("OUTCOME_UNKNOWN");
  await expect(page.getByTestId("file-compute-result")).toHaveCount(0);
  await page.reload();
  await expect(page.getByTestId("file-compute-panel")).toHaveCount(0);
  expect(calls).toBe(1);
});

test("daemon-confirmed ON does not imply READY and OFF needs a real command", async ({ page }) => {
  await openWithRuntime(page, runtime());
  const control = page.getByTestId("participation-switch");
  await expect(control).toHaveAttribute("aria-checked", "true");
  await expect(page.getByText("Reconnexion", { exact: true }).first()).toBeVisible();
  await expect(page.getByText("READY", { exact: true })).toHaveCount(0);
  await control.click();
  await expect(control).toHaveAttribute("aria-checked", "false");
  await expect(page.getByText("DISABLED", { exact: true }).first()).toBeVisible();
});

test("without daemon control the switch is unknown, not a localStorage OFF", async ({ page }) => {
  await page.addInitScript(() => window.localStorage.setItem("phoneboost.participation-enabled", "false"));
  await page.goto("/");
  const control = page.getByTestId("participation-switch");
  await expect(control).toBeDisabled();
  await expect(control).toHaveText("?");
  await expect(page.getByText("DISABLED", { exact: true })).toHaveCount(0);
});

test("a reconnecting runtime cannot become READY", async ({ page }) => {
  await openWithRuntime(page, runtime());
  await expect(page.getByText("Reconnexion", { exact: true }).first()).toBeVisible();
  await expect(page.getByText("READY", { exact: true })).toHaveCount(0);
});

test("READY requires authenticated, leased, admitted provider truth", async ({ page }) => {
  await openWithRuntime(page, runtime({
    authenticated_session: { state: "AUTHENTICATED", remote_worker_state: "AUTHENTICATED" },
    controller_lease: { state: "ACTIVE", reason: "C07_ACK_FRESH" },
    resource_guard_admission_proof: { state: "FRESH_PASS", reason: "C08_C09_C10_PROBE_PASSED" },
    provider_readiness: { provider: "pb.native.blake3/1", state: "AVAILABLE" },
    auto_use: { state: "AVAILABLE", reason: "READY" },
    remote_blake3_available: true,
  }));
  await expect(page.getByText("Prêt", { exact: true }).first()).toBeVisible();
});

test("Local and Remote remain visibly separate and honestly described", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByText("Local · hôte Linux")).toBeVisible();
  await expect(page.getByText("Remote · worker Android")).toBeVisible();
  await expect(page.getByText(/RemoteBuffer n’est ni de la RAM locale, ni du swap/)).toBeVisible();
  await expect(page.getByText(/Ce n’est ni une extension de RAM, ni du swap, ni une illusion de CPU/)).toBeVisible();
});

test("advanced details are collapsed and can be toggled", async ({ page }) => {
  await page.goto("/");
  const details = page.getByTestId("advanced-details");
  await expect(details).not.toHaveAttribute("open", "");
  await details.locator("summary").click();
  await expect(details).toHaveAttribute("open", "");
  await expect(page.getByText("Cinq portes indépendantes")).toBeVisible();
  await details.locator("summary").click();
  await expect(details).not.toHaveAttribute("open", "");
});

test("missing runtime data never invents device facts", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByTestId("live-status-badge")).toContainText("LIVE indisponible");
  await expect(page.getByText("Indisponible", { exact: true }).first()).toBeVisible();
  const visible = await page.locator("body").innerText();
  expect(visible).not.toMatch(/Galaxy A15|82%|Local IP|192\.168\.|10\.0\./);
});

test("failure truth remains visible when the bridge is lost", async ({ page }) => {
  await page.route("**/bridge/v1/snapshot", (route) => route.abort("failed"));
  await page.goto(`/#token=${TOKEN}`);
  await expect(page.getByTestId("live-status-badge")).toContainText("LIVE indisponible");
  await expect(page.getByText("OFFLINE", { exact: true }).first()).toBeVisible();
  await expect(page.getByText("READY", { exact: true })).toHaveCount(0);
});

test("implemented navigation targets exist", async ({ page }) => {
  await page.goto("/");
  for (const target of ["overview", "resources", "activity", "evidence"]) {
    await expect(page.locator(`#${target}`)).toHaveCount(1);
  }
});

test("the primary control remains usable at a narrow viewport", async ({ page }) => {
  await page.setViewportSize({ width: 760, height: 900 });
  await page.goto("/");
  await expect(page.getByRole("heading", { name: "PhoneBoost Control Center" })).toBeVisible();
  await expect(page.getByTestId("participation-switch")).toBeVisible();
  await expect(page.getByText("Remote · worker Android")).toBeVisible();
});
