import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { access } from "node:fs/promises";
import { setTimeout as delay } from "node:timers/promises";
import { chromium, firefox } from "playwright";

const MIME_BY_PATH = new Map([
  ["/api/v1/health", "application/json"],
  ["/api/v1/galleries/demo/photos", "application/json"],
  ["/api/v1/photos/demo-001/thumbnail/800", "image/jpeg"],
  ["/photo_culler_slint_bg.wasm", "application/wasm"],
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
    ["--port", "0", "--web-root", webRoot],
    { stdio: ["ignore", "pipe", "pipe"] },
  );
  let serverStderr = "";
  processState.stderr.setEncoding("utf8");
  processState.stderr.on("data", (chunk) => { serverStderr += chunk; });

  let origin;
  let browser;
  try {
    origin = await waitForOrigin(processState);
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
      if (MIME_BY_PATH.has(url.pathname)) {
        responses.push({
          origin: url.origin,
          path: url.pathname,
          status: response.status(),
          mime: (response.headers()["content-type"] ?? "").split(";")[0].trim().toLowerCase(),
        });
      }
    });
    page.on("pageerror", (error) => pageErrors.push(error.message));
    page.on("console", (message) => {
      if (message.type() === "error") consoleErrors.push(message.text());
    });

    await page.goto(origin, { waitUntil: "domcontentloaded" });
    try {
      await page.waitForFunction(
        () => document.documentElement.dataset.photoCullerState === "ready",
        undefined,
        { timeout: 45_000 },
      );
    } catch (error) {
      const pageState = await page.evaluate(() => ({
        state: document.documentElement.dataset.photoCullerState ?? null,
        status: document.querySelector("#application-status")?.textContent ?? null,
        diagnosticVisible: Boolean(document.querySelector("#diagnostic")?.getClientRects().length),
        diagnosticTitle: document.querySelector("#diagnostic-title")?.textContent ?? null,
        diagnosticMessage: document.querySelector("#diagnostic-message")?.textContent ?? null,
      }));
      if (process.env.PHOTO_CULLER_SCREENSHOT) {
        await page.screenshot({ path: process.env.PHOTO_CULLER_SCREENSHOT, fullPage: true }).catch(() => {});
      }
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
      const response = responses.find((entry) => entry.path === path);
      assert.ok(response, `the actual application must request ${path}`);
      assert.equal(response.status, 200, `${path} must return HTTP 200`);
      assert.equal(response.mime, expectedMime, `${path} must have MIME ${expectedMime}`);
      assert.equal(response.origin, origin, `${path} must use the Rust server's same origin`);
    }

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
      result: "real Rust server and Slint WASM displayed the JPEG; bootstrap diagnostic is accessible",
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

function waitForOrigin(child) {
  return new Promise((resolve, reject) => {
    let stdout = "";
    const timeout = setTimeout(() => reject(new Error("Rust server did not announce its origin")), 10_000);
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
        if (/^http:\/\/127\.0\.0\.1:\d+$/.test(line)) finish(null, line);
        else finish(new Error(`unexpected Rust server stdout: ${line}`));
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
