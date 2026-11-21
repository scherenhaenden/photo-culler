import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { spawn } from "node:child_process";
import { access, mkdir, writeFile } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";
import { setTimeout as delay } from "node:timers/promises";
import { chromium, firefox } from "playwright";
import pngjs from "pngjs";

const GALLERY_PATH = "/api/v1/galleries/demo/photos";
const ERROR_BODY = {
  code: "ui_state_fixture",
  message: "The UI-state smoke fixture returned HTTP 503.",
  request_id: "s02-ui-state-retry",
};
const EXPECTED_THUMBNAIL_SHA256 = "cc20c24844d2fc6fca729fcb66cfe661960f7b56be41a3dd462a0f1b22cfb4f2";
const { PNG } = pngjs;
const EMPTY_BODY = {
  gallery_id: "demo",
  items: [],
  next_cursor: null,
  total: 0,
};
const VIEWPORTS = [
  { name: "1440x900", width: 1440, height: 900 },
  { name: "1100x700", width: 1100, height: 700 },
];

const browserName = process.argv[2] ?? "chromium";
assert.ok(["chromium", "firefox"].includes(browserName), "browser must be chromium or firefox");
const serverExecutable = process.env.PHOTO_CULLER_SERVER;
const webRoot = process.env.PHOTO_CULLER_WEB_ROOT;
const outputDir = resolve(
  process.env.PHOTO_CULLER_UI_STATES_CAPTURE_DIR ?? join("artifacts", "ui-states", browserName),
);
assert.ok(serverExecutable, "PHOTO_CULLER_SERVER must point to the built Rust server");
assert.ok(webRoot, "PHOTO_CULLER_WEB_ROOT must point to the generated Slint WASM bundle");
await access(serverExecutable);
await access(join(webRoot, "bundle.json"));
await mkdir(outputDir, { recursive: true });

const server = spawn(serverExecutable, ["--owned", "--port", "0", "--web-root", webRoot], {
  stdio: ["pipe", "pipe", "pipe"],
});
let serverStderr = "";
server.stderr.setEncoding("utf8");
server.stderr.on("data", (chunk) => { serverStderr += chunk; });
let browser;
let originForManifest = "not-ready";
const captures = [];
const scenarioResults = [];
try {
  const ready = await waitForReady(server);
  const origin = ready.origin;
  originForManifest = origin;
  const browserType = browserName === "chromium" ? chromium : firefox;
  const launchOptions = { headless: browserName === "chromium" };
  if (browserName === "chromium") {
    launchOptions.args = ["--enable-webgl", "--use-gl=angle", "--use-angle=swiftshader"];
  } else {
    launchOptions.firefoxUserPrefs = {
      "webgl.disabled": false,
      "webgl.force-enabled": true,
    };
  }
  browser = await browserType.launch(launchOptions);

  for (const viewport of VIEWPORTS) {
    captures.push(await captureLoading(browser, origin, viewport, outputDir, ready.session_token));
    scenarioResults.push(await captureEmpty(browser, origin, viewport, outputDir, ready.session_token));
    const retryResult = await captureErrorAndRetry(browser, origin, viewport, outputDir, ready.session_token);
    captures.push(...retryResult.captures);
    scenarioResults.push(retryResult.result);
  }

  const manifest = {
    schema_version: 1,
    harness: "migration/demo/smoke/ui-states/smoke.mjs",
    browser: browserName,
    browser_version: browser.version(),
    origin: "Rust Axum server on loopback; actual Slint WASM assets; gallery responses intercepted only for deterministic UI states",
    server_origin: originForManifest,
    source_sha: process.env.GITHUB_SHA ?? process.env.PHOTO_CULLER_UI_STATES_SOURCE_SHA ?? "not-provided",
    fixture: {
      gallery_path: GALLERY_PATH,
      loading: "hold the gallery request until the loading canvas screenshot is captured, then continue to the real Rust server",
      empty: EMPTY_BODY,
      error: { http_status: 503, body: ERROR_BODY },
      retry: "first gallery request returns fixture HTTP 503; click the visible Slint retry control; second request waits for a loading capture and then continues to the real Rust server",
    },
    viewports: VIEWPORTS,
    state_results: scenarioResults,
    captures,
  };
  const manifestPath = join(outputDir, "manifest.json");
  await writeFile(manifestPath, `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(JSON.stringify({ manifest: manifestPath, ...manifest }, null, 2));
} catch (error) {
  const detail = serverStderr.trim();
  if (detail) error.message += `\nRust server stderr:\n${detail}`;
  const failedManifest = {
    schema_version: 1,
    harness: "migration/demo/smoke/ui-states/smoke.mjs",
    browser: browserName,
    browser_version: browser?.version() ?? "not-launched",
    source_sha: process.env.GITHUB_SHA ?? process.env.PHOTO_CULLER_UI_STATES_SOURCE_SHA ?? "not-provided",
    server_origin: originForManifest,
    result: "failed",
    failure: error.message,
    captures,
    state_results: scenarioResults,
  };
  await writeFile(join(outputDir, "manifest.json"), `${JSON.stringify(failedManifest, null, 2)}\n`);
  throw error;
} finally {
  await browser?.close();
  await stopServer(server);
}

async function captureLoading(browser, origin, viewport, outputDir, sessionToken) {
  const page = await newPage(browser, viewport);
  const requestSeen = deferred();
  const releaseRequest = deferred();
  let galleryRequests = 0;
  await page.route(`**${GALLERY_PATH}**`, async (route) => {
    galleryRequests += 1;
    requestSeen.resolve();
    await releaseRequest.promise;
    await route.continue();
  });
  try {
    await navigateAndPair(page, origin, sessionToken);
    await withTimeout(requestSeen.promise, 45_000, "WASM did not request the gallery for loading capture");
    await waitForState(page, "loading");
    assert.equal(galleryRequests, 1, "the loading state must be held by the first real gallery request");
    const capture = await saveState(page, viewport, outputDir, "loading", {
      gallery_requests: galleryRequests,
      response_held: true,
    });
    releaseRequest.resolve();
    await waitForState(page, "ready");
    await assertReadyTransport(page, origin);
    return capture;
  } finally {
    releaseRequest.resolve();
    await page.close();
  }
}

async function captureEmpty(browser, origin, viewport, outputDir, sessionToken) {
  const page = await newPage(browser, viewport);
  let galleryRequests = 0;
  await page.route(`**${GALLERY_PATH}**`, async (route) => {
    galleryRequests += 1;
    await route.fulfill({
      status: 200,
      contentType: "application/json; charset=utf-8",
      body: JSON.stringify(EMPTY_BODY),
    });
  });
  try {
    await navigateAndPair(page, origin, sessionToken);
    await waitForState(page, "empty");
    assert.equal(galleryRequests, 1, "empty state must use exactly one deterministic gallery response");
    const statusText = await statusTextOf(page);
    assert.match(statusText, /contains no photos/i, "empty state must announce why the Library is empty");
    await assertCanvasVisible(page);
    const capture = await saveState(page, viewport, outputDir, "empty", {
      gallery_requests: galleryRequests,
      gallery_response: EMPTY_BODY,
    });
    await assertAppTransport(page, origin);
    return capture;
  } finally {
    await page.close();
  }
}

async function captureErrorAndRetry(browser, origin, viewport, outputDir, sessionToken) {
  const page = await newPage(browser, viewport);
  let galleryRequests = 0;
  const initialRequestSeen = deferred();
  const releaseInitial = deferred();
  const retryRequestSeen = deferred();
  const releaseRetry = deferred();
  await page.route(`**${GALLERY_PATH}**`, async (route) => {
    galleryRequests += 1;
    if (galleryRequests === 1) {
      initialRequestSeen.resolve();
      await releaseInitial.promise;
      await route.fulfill({
        status: 503,
        contentType: "application/json; charset=utf-8",
        body: JSON.stringify(ERROR_BODY),
      });
      return;
    }
    if (galleryRequests === 2) {
      retryRequestSeen.resolve();
      await releaseRetry.promise;
      await route.continue();
      return;
    }
    await route.continue();
  });

  try {
    await navigateAndPair(page, origin, sessionToken);
    await withTimeout(initialRequestSeen.promise, 45_000, "WASM did not request the gallery for error/retry capture");
    await waitForState(page, "loading");
    releaseInitial.resolve();
    await waitForState(page, "error");
    assert.equal(galleryRequests, 1, "initial error state must use the fixture HTTP 503");
    const errorText = await statusTextOf(page);
    assert.match(errorText, /503/, "accessible status must expose the REST failure");
    assert.match(errorText, /ui_state_fixture/, "accessible status must preserve the typed REST error code");
    assert.match(errorText, /s02-ui-state-retry/, "accessible status must preserve the REST request id");
    await assertCanvasVisible(page);
    const errorCapture = await saveState(page, viewport, outputDir, "error", {
      gallery_requests: galleryRequests,
      gallery_response: { status: 503, body: ERROR_BODY },
    });

    const retryClick = await clickVisibleRetry(page, viewport);
    await withTimeout(retryRequestSeen.promise, 10_000, "visible retry control did not issue a second gallery request");
    await waitForState(page, "loading");
    assert.equal(galleryRequests, 2, "retry must issue exactly one new gallery request");
    const retryLoadingCapture = await saveState(page, viewport, outputDir, "retry-loading", {
      gallery_requests: galleryRequests,
      click: retryClick,
      response_held: true,
    });

    releaseRetry.resolve();
    await waitForState(page, "ready");
    const readyCapture = await saveState(page, viewport, outputDir, "retry-ready", {
      gallery_requests: galleryRequests,
      click: retryClick,
      recovery_source: "real Rust Axum gallery and JPEG routes",
    });
    const history = await stateHistory(page);
    assertSubsequence(history, ["loading", "error", "loading", "ready"], "retry state transition");
    await assertReadyTransport(page, origin);
    return {
      captures: [errorCapture, retryLoadingCapture, readyCapture],
      result: {
        viewport: viewport.name,
        initial_error_status: 503,
        retry_requests: galleryRequests,
        retry_click: retryClick,
        final_state: "ready",
        final_status_text: await statusTextOf(page),
        state_history: history,
        canvas_visible: true,
        real_gallery_and_thumbnail_returned: true,
      },
    };
  } finally {
    releaseInitial.resolve();
    releaseRetry.resolve();
    await page.close();
  }
}

async function newPage(browser, viewport) {
  const page = await browser.newPage({
    viewport: { width: viewport.width, height: viewport.height },
    deviceScaleFactor: 1,
  });
  page.setDefaultTimeout(15_000);
  await page.addInitScript(() => {
    Object.defineProperty(window, "__photoCullerStateHistory", { value: [] });
  });
  return page;
}

async function navigateAndPair(page, origin, sessionToken) {
  const wasmLoaded = page.waitForResponse((response) => {
    const url = new URL(response.url());
    return url.pathname === "/photo_culler_slint_bg.wasm" && response.status() === 200;
  });
  await page.goto(origin, { waitUntil: "domcontentloaded" });
  await wasmLoaded;
  const pairingCode = await issuePairingCode(origin, sessionToken);
  await page.waitForTimeout(250);
  await page.mouse.click(
    Math.round((page.viewportSize()?.width ?? 1440) / 2),
    Math.round((page.viewportSize()?.height ?? 900) / 2),
  );
  await page.keyboard.type(pairingCode);
  const paired = page.waitForResponse((response) => {
    const url = new URL(response.url());
    return url.pathname === "/api/v1/session/pair" && response.request().method() === "POST";
  });
  await page.keyboard.press("Enter");
  assert.equal((await paired).status(), 200, "the browser must pair through the real Rust session route");
}

async function issuePairingCode(origin, sessionToken) {
  assert.match(sessionToken, /^[0-9a-f]{64}$/i, "owned Rust server must return its private session token");
  const response = await fetch(`${origin}/api/v1/session/pairing-codes`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${sessionToken}`,
      origin,
    },
  });
  assert.equal(response.status, 200, "the owned session must issue a one-time UI pairing code");
  const payload = await response.json();
  assert.match(payload.code, /^[0-9a-f]{64}$/i, "Rust server must issue a one-time pairing code");
  return payload.code;
}

async function waitForState(page, expected) {
  const deadline = Date.now() + 45_000;
  const html = page.locator("html");
  while (Date.now() < deadline) {
    if (await html.getAttribute("data-photo-culler-state") === expected) break;
    await delay(100);
  }
  assert.equal(
    await html.getAttribute("data-photo-culler-state"),
    expected,
    `document did not reach ${expected} state within 45000 ms`,
  );
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  const status = page.locator("#application-status");
  assert.equal(await status.getAttribute("data-state"), expected, `aria-live state must be ${expected}`);
  await page.evaluate((state) => {
    const history = window.__photoCullerStateHistory;
    if (history.at(-1) !== state) history.push(state);
  }, expected);
  await assertCanvasVisible(page);
}

function assertCanvasVisible(page) {
  return page.locator("#canvas").isVisible().then((visible) => {
    assert.equal(visible, true, "the actual Slint canvas must remain visible during application states");
  });
}

async function statusTextOf(page) {
  return (await page.locator("#application-status").textContent())?.trim() ?? "";
}

async function stateHistory(page) {
  return page.evaluate(() => [...window.__photoCullerStateHistory]);
}

async function saveState(page, viewport, outputDir, state, details) {
  await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(() => requestAnimationFrame(resolve))));
  const filename = `${viewport.name}-${state}.png`;
  const path = join(outputDir, filename);
  await mkdir(dirname(path), { recursive: true });
  const png = await page.screenshot({ path, fullPage: false });
  assert.ok(png.length > 1024, `${filename} must contain the rendered Slint canvas`);
  const decoded = PNG.sync.read(png);
  const dimensions = { width: decoded.width, height: decoded.height };
  assert.deepEqual(dimensions, { width: viewport.width, height: viewport.height }, `${filename} must match its viewport`);
  const gallerySignalPixels = countGallerySignalPixels(decoded, viewport.width, viewport.height);
  assert.ok(
    gallerySignalPixels >= 40,
    `${filename} must contain visible Slint-rendered pixels in the gallery state region; found ${gallerySignalPixels} signal pixels`,
  );
  const canvas = await page.locator("#canvas").evaluate((element) => {
    const rect = element.getBoundingClientRect();
    return {
      visible: rect.width > 0 && rect.height > 0 && getComputedStyle(element).display !== "none",
      css_width: rect.width,
      css_height: rect.height,
      backing_width: element.width,
      backing_height: element.height,
    };
  });
  assert.equal(canvas.visible, true, `${filename} requires a visible Slint canvas`);
  return {
    file: filename,
    viewport: viewport.name,
    state,
    width: dimensions.width,
    height: dimensions.height,
    sha256: createHash("sha256").update(png).digest("hex"),
    canvas,
    aria_live_text: await statusTextOf(page),
    state_history: await stateHistory(page),
    details: { ...details, gallery_signal_pixels: gallerySignalPixels },
  };
}

async function assertAppTransport(page, origin) {
  const entries = await page.evaluate((expectedOrigin) => {
    const resources = window.performance.getEntriesByType("resource").map((entry) => {
      const url = new URL(entry.name);
      return { origin: url.origin, path: url.pathname };
    });
    for (const path of ["/api/v1/health", "/photo_culler_slint.js", "/photo_culler_slint_bg.wasm"]) {
      if (!resources.some((entry) => entry.path === path && entry.origin === expectedOrigin)) {
        throw new Error(`missing same-origin Rust/WASM request ${path}`);
      }
    }
    return resources;
  }, origin);
  return entries;
}

async function assertReadyTransport(page, origin) {
  const entries = await assertAppTransport(page, origin);
  const state = await page.locator("#application-status").getAttribute("data-state");
  assert.equal(state, "ready", "the app must reach ready after the retry/real response");
  const hasThumbnail = entries.some((entry) => entry.path === "/api/v1/photos/demo-001/thumbnail/800" && entry.origin === origin);
  assert.equal(hasThumbnail, true, "ready state must display the JPEG returned by the real Rust origin");
  const thumbnail = await page.evaluate(async (path) => {
    const response = await fetch(path, { cache: "no-store" });
    const bytes = new Uint8Array(await response.arrayBuffer());
    const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", bytes));
    return {
      status: response.status,
      content_type: response.headers.get("content-type"),
      byte_length: bytes.length,
      prefix: [...bytes.slice(0, 2)],
      sha256: [...digest].map((byte) => byte.toString(16).padStart(2, "0")).join(""),
    };
  }, "/api/v1/photos/demo-001/thumbnail/800");
  assert.equal(thumbnail.status, 200, "the Rust thumbnail route must return HTTP 200");
  assert.match(thumbnail.content_type ?? "", /^image\/jpeg(?:\s*;|$)/i, "the Rust thumbnail must use image/jpeg");
  assert.ok(thumbnail.byte_length > 4, "the Rust thumbnail response must contain JPEG bytes");
  assert.deepEqual(thumbnail.prefix, [0xff, 0xd8], "the thumbnail must start with the JPEG SOI marker");
  assert.equal(thumbnail.sha256, EXPECTED_THUMBNAIL_SHA256, "the Rust origin must serve the bundled demo JPEG");
}

function countGallerySignalPixels(image, width, height) {
  const xStart = Math.min(450, width - 1);
  const xEnd = Math.max(xStart + 1, width - 16);
  const yStart = Math.min(100, height - 1);
  const yEnd = Math.max(yStart + 1, height - 80);
  let count = 0;
  for (let y = yStart; y < yEnd; y += 1) {
    for (let x = xStart; x < xEnd; x += 1) {
      const offset = (y * width + x) * 4;
      if (image.data[offset + 3] > 240 && image.data[offset] + image.data[offset + 1] + image.data[offset + 2] >= 160) {
        count += 1;
      }
    }
  }
  return count;
}

async function clickVisibleRetry(page, viewport) {
  const status = page.locator("#application-status");
  const galleryCenterX = 220 + 230 + Math.floor((viewport.width - 220 - 230) / 2);
  const xCandidates = [...new Set([220 + 230 + 60, galleryCenterX])];
  const attempted = [];
  // The retry action is a Slint control drawn inside the canvas rather than a
  // DOM button. Scan only the gallery canvas column, where the error panel owns
  // all pointer input, and stop at the first visible state transition.
  for (let y = 54; y < viewport.height; y += 8) {
    for (const x of xCandidates) {
      attempted.push({ x, y });
      await page.mouse.click(x, y);
      await page.evaluate(() => new Promise((resolve) => requestAnimationFrame(resolve)));
      if (await status.getAttribute("data-state") !== "error") {
        return { x, y, method: "canvas pointer scan" };
      }
    }
  }
  throw new Error(`could not activate the visible Slint retry action; attempted=${JSON.stringify(attempted)}`);
}

function assertSubsequence(actual, expected, label) {
  let index = 0;
  for (const state of actual) {
    if (state === expected[index]) index += 1;
    if (index === expected.length) return;
  }
  assert.fail(`${label} did not reach ${expected.join(" -> ")}; observed=${actual.join(" -> ")}`);
}

function deferred() {
  let resolve;
  const promise = new Promise((done) => { resolve = done; });
  return { promise, resolve };
}

async function withTimeout(promise, timeoutMs, message) {
  let timer;
  try {
    return await Promise.race([
      promise,
      new Promise((_, reject) => {
        timer = setTimeout(() => reject(new Error(message)), timeoutMs);
      }),
    ]);
  } finally {
    clearTimeout(timer);
  }
}

function waitForReady(child) {
  return new Promise((resolve, reject) => {
    let stdout = "";
    const timeout = setTimeout(() => reject(new Error("Rust server did not announce owned readiness")), 10_000);
    const finish = (error, value) => {
      clearTimeout(timeout);
      if (error) reject(error);
      else resolve(value);
    };
    child.stdout.setEncoding("utf8");
    child.stdout.on("data", (chunk) => {
      stdout += chunk;
      const lines = stdout.split(/\r?\n/);
      stdout = lines.pop() ?? "";
      for (const line of lines) {
        if (!line) continue;
        try {
          const ready = JSON.parse(line);
          if (
            ready.event === "ready" &&
            typeof ready.origin === "string" &&
            /^http:\/\/127\.0\.0\.1:\d+$/.test(ready.origin) &&
            typeof ready.session_token === "string"
          ) {
            finish(null, ready);
          } else {
            finish(new Error("Rust server returned an invalid owned-ready message"));
          }
        } catch {
          finish(new Error("Rust server stdout was not a valid owned-ready message"));
        }
      }
      if (stdout.length > 4096) finish(new Error("Rust server stdout line exceeded the protocol limit"));
    });
    child.once("error", (error) => finish(error));
    child.once("exit", (code, signal) => finish(new Error(`Rust server exited before readiness (code=${code}, signal=${signal})`)));
  });
}

async function stopServer(child) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  child.kill("SIGINT");
  const exited = new Promise((resolve) => child.once("exit", resolve));
  await Promise.race([exited, delay(3_500)]);
  if (child.exitCode === null && child.signalCode === null) {
    child.kill("SIGKILL");
    await Promise.race([exited, delay(1_000)]);
  }
}
