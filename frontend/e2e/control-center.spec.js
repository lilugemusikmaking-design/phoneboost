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
});

test("ON is a user preference and does not imply READY", async ({ page }) => {
  await page.goto("/");
  const control = page.getByTestId("participation-switch");
  await expect(control).toHaveAttribute("aria-checked", "true");
  await expect(page.getByText("OFFLINE", { exact: true }).first()).toBeVisible();
  await expect(page.getByText("READY", { exact: true })).toHaveCount(0);
  await control.click();
  await expect(control).toHaveAttribute("aria-checked", "false");
  await expect(page.getByText("DISABLED", { exact: true }).first()).toBeVisible();
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
