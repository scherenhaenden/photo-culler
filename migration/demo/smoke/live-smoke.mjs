import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { access } from "node:fs/promises";
import { setTimeout as delay } from "node:timers/promises";
import { chromium, firefox } from "playwright";

const MIME_BY_PATH = new Map([
  ["/api/v1/health", "application/json"],
  ["/api/v1/galleries/demo/photos", "application/json"],
  ["/api/v1/photos/demo-001/preview", "image/jpeg"],
  ["/api/v1/photos/demo-001/thumbnail/800", "image/jpeg"],
  ["/photo_culler_slint_bg.wasm", "application/wasm"],
]);
const IMAGE_ROUTES = new Set([
  "/api/v1/photos/demo-001/preview",
  "/api/v1/photos/demo-001/thumbnail/800",
]);

export async function runLiveSmoke(browserName) {
  const serverExecutable = process.env.PHOTO_CULLER_SERVER;
  const webRoot = process.env.PHOTO_CULLER_WEB_ROOT;
  assert.ok(serverExecutable, "PHOTO_CULLER_SERVER must point to the built Rust server");
  assert.ok(webRoot, "PHOTO_CULLER_WEB_ROOT must point to the generated Slint web bundle");
  await access(serverExecutable);
  await access(`${webRoot}/bundle.json`);

  const processState = spawn(
    serverExecutable,
    ["--owned", "--port", "0", "--web-root", webRoot],
    { stdio: ["pipe", "pipe", "pipe"] },
  );
  let serverStderr = "";
  processState.stderr.setEncoding("utf8");
  processState.stderr.on("data", (chunk) => { serverStderr += chunk; });

  let origin;
  let browser;
  try {
    const ready = await waitForReady(processState);
    origin = ready.origin;
    const pairingCode = await issuePairingCode(origin, ready.session_token);
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
    const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, deviceScaleFactor: 1 });
    const responses = [];
    const pageErrors = [];
    const consoleErrors = [];
    page.on("response", (response) => {
      const url = new URL(response.url());
      if (MIME_BY_PATH.has(url.pathname) || url.pathname.startsWith("/api/v1/")) {
        responses.push({
          origin: url.origin,
          path: url.pathname,
          query: url.search,
          status: response.status(),
          mime: (response.headers()["content-type"] ?? "").split(";")[0].trim().toLowerCase(),
        });
      }
    });
    page.on("pageerror", (error) => pageErrors.push(error.message));
    page.on("console", (message) => {
      if (message.type() === "error") consoleErrors.push(message.text());
    });

    const wasmLoaded = page.waitForResponse((response) => {
      const url = new URL(response.url());
      return url.pathname === "/photo_culler_slint_bg.wasm" && response.status() === 200;
    });
    await page.goto(origin, { waitUntil: "domcontentloaded" });
    await wasmLoaded;
    // Firefox's headed WebGL initialization can finish fetching WASM before
    // the Slint pairing overlay is ready to receive focus.
    await page.waitForTimeout(browserName === "firefox" ? 2000 : 250);
    await page.bringToFront();
    await page.mouse.click(720, 450);
    await page.keyboard.type(pairingCode);
    const paired = page.waitForResponse((response) => {
      const url = new URL(response.url());
      return url.pathname === "/api/v1/session/pair" && response.request().method() === "POST";
    });
    await page.keyboard.press("Enter");
    const pairResponse = await paired;
    if (pairResponse.status() !== 200) {
      const requestHeaders = await pairResponse.request().allHeaders();
      const error = await pairResponse.json().catch(() => ({}));
      throw new Error(
        `the browser must pair through the real session route: ` +
          `status=${pairResponse.status()}, ` +
          `error=${JSON.stringify({ code: error.code, message: error.message })}, ` +
          `request-headers=${JSON.stringify({
            host: requestHeaders.host,
            origin: requestHeaders.origin,
            sec_fetch_site: requestHeaders["sec-fetch-site"],
          })}`,
      );
    }
    try {
      await waitForHtmlState(page, "ready", 45_000);
    } catch (error) {
      const pageState = {
        state: await page.locator("html").getAttribute("data-photo-culler-state"),
        status: await page.locator("#application-status").textContent().catch(() => null),
        diagnosticVisible: await page.locator("#diagnostic").isVisible().catch(() => false),
        diagnosticTitle: await page.locator("#diagnostic-title").textContent().catch(() => null),
        diagnosticMessage: await page.locator("#diagnostic-message").textContent().catch(() => null),
      };
      throw new Error(
        `${error.message}; page-state=${JSON.stringify(pageState)}; ` +
          `console=${JSON.stringify(consoleErrors)}; page-errors=${JSON.stringify(pageErrors)}; ` +
          `responses=${JSON.stringify(responses)}`,
      );
    }
    const status = page.locator("#application-status");
    assert.equal(await status.getAttribute("data-state"), "ready");
    assert.match(await status.textContent(), /demo-001\.jpg/);
    const diagnosticVisible = await page.locator("#diagnostic").isVisible();
    const diagnosticTitle = await page.locator("#diagnostic-title").textContent();
    const diagnosticMessage = await page.locator("#diagnostic-message").textContent();
    assert.equal(
      diagnosticVisible,
      false,
      `bootstrap diagnostic appeared during successful app load: ${diagnosticTitle}; ${diagnosticMessage}; console: ${consoleErrors.join(" | ")}`,
    );
    assert.deepEqual(pageErrors, [], "the actual Slint WASM application must not raise page errors");

    for (const [path, expectedMime] of MIME_BY_PATH) {
      if (IMAGE_ROUTES.has(path)) continue;
      const response = responses.find((entry) => entry.path === path && entry.status === 200);
      assert.ok(
        response,
        `${path} must return HTTP 200; observed ${JSON.stringify(responses)}`,
      );
      assert.equal(response.mime, expectedMime, `${path} must have MIME ${expectedMime}`);
      assert.equal(response.origin, origin, `${path} must use the Rust server's same origin`);
    }
    const displayedImage = responses.find(
      (entry) => IMAGE_ROUTES.has(entry.path) && entry.status === 200 && entry.mime === "image/jpeg",
    );
    assert.ok(
      displayedImage,
      `the gallery must load its bounded preview or fixture thumbnail: ${JSON.stringify(responses)}`,
    );
    assert.equal(displayedImage.origin, origin, "the JPEG must come from the Rust server origin");

    const screenshot = await page.screenshot({ fullPage: true });
    assert.ok(screenshot.length > 1024, "the rendered Slint screen must produce a non-empty capture");
    if (process.env.PHOTO_CULLER_SCREENSHOT) {
      await page.screenshot({ path: process.env.PHOTO_CULLER_SCREENSHOT, fullPage: true });
    }

    // Exercise the user-facing recovery path against the real bootstrap. The
    // successful app load above remains unmodified; only the second navigation
    // aborts its WASM request.
    await page.route("**/photo_culler_slint_bg.wasm", (route) => route.abort());
    await page.goto(origin, { waitUntil: "domcontentloaded" });
    const diagnostic = page.locator("#diagnostic");
    await diagnostic.waitFor({ state: "visible", timeout: 15_000 });
    assert.match(await page.locator("#diagnostic-title").textContent(), /could not load/i);
    assert.match(await page.locator("#diagnostic-message").textContent(), /Photo Culler Desktop/);
    assert.equal(await page.locator("#canvas").isVisible(), false);
    assert.deepEqual(pageErrors, [], "WASM load failure must be shown as an accessible diagnostic");
    console.log(JSON.stringify({
      browser: browserName,
      version: browser.version(),
      origin,
      application_wasm: true,
      state: await status.getAttribute("data-state"),
      gallery: await status.textContent(),
      responses,
      screenshot_bytes: screenshot.length,
      authenticated: true,
      result: "paired browser, Rust server, and Slint WASM displayed the JPEG; bootstrap diagnostic is accessible",
    }));
  } catch (error) {
    const detail = serverStderr.trim();
    if (detail) error.message += `\nRust server stderr:\n${detail}`;
    throw error;
  } finally {
    await browser?.close();
    await stopServer(processState);
  }
}

export async function issuePairingCode(origin, sessionToken) {
  assert.match(sessionToken, /^[0-9a-f]{64}$/i, "owned server must return its private session token");
  const response = await fetch(`${origin}/api/v1/session/pairing-codes`, {
    method: "POST",
    headers: {
      authorization: `Bearer ${sessionToken}`,
      origin,
    },
  });
  assert.equal(response.status, 200, "the owned editor session must issue a pairing code");
  const payload = await response.json();
  assert.match(payload.code, /^[0-9a-f]{64}$/i, "server must issue a one-time pairing code");
  return payload.code;
}

async function waitForHtmlState(page, expectedState, timeoutMs) {
  const deadline = Date.now() + timeoutMs;
  const html = page.locator("html");
  while (Date.now() < deadline) {
    if (await html.getAttribute("data-photo-culler-state") === expectedState) return;
    await delay(100);
  }
  throw new Error(`document did not reach ${expectedState} state within ${timeoutMs} ms`);
}

export function waitForReady(child) {
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
    child.once("exit", (code, signal) => {
      finish(new Error(`Rust server exited before readiness (code=${code}, signal=${signal})`));
    });
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
