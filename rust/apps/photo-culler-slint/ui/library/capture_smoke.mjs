/**
 * Capture the deterministic ten-card Pro Library fixture at the two reference
 * viewports. The browser-only fixture selector is `?photo-culler-smoke=1`;
 * unlike the normal live view, it repeats the REST-served bundled JPEG while
 * assigning explicitly synthetic photo metadata.
 *
 * From the repository root, after building the server and staging the WASM:
 *
 *   npm install --no-save --package-lock=false playwright@1.51.1
 *   PHOTO_CULLER_SERVER=rust/target/release/photo-culler-server \
 *   PHOTO_CULLER_WEB_ROOT=artifacts/wasm32/bundle \
 *   PHOTO_CULLER_GALLERY_CAPTURE_DIR=artifacts/s05-gallery \
 *   node rust/apps/photo-culler-slint/ui/library/capture_smoke.mjs
 *
 * Outputs `pro-library-1440x900.png`, `pro-library-1100x700.png`, and a JSON
 * manifest containing the browser version, viewport, selected fixture and
 * SHA-256 of the local REST JPEG. Requires a built Rust server, staged Slint
 * WASM bundle, Node.js and Playwright 1.51.1.
 */

import assert from "node:assert/strict";
import { createHash } from "node:crypto";
import { mkdir, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import { setTimeout as delay } from "node:timers/promises";
import { chromium } from "playwright";

const EXPECTED_JPEG_SHA256 = "cc20c24844d2fc6fca729fcb66cfe661960f7b56be41a3dd462a0f1b22cfb4f2";
const THUMBNAIL_PATH = "/api/v1/photos/demo-001/thumbnail/800";
const serverExecutable = process.env.PHOTO_CULLER_SERVER;
const webRoot = process.env.PHOTO_CULLER_WEB_ROOT;
const outputDir = process.env.PHOTO_CULLER_GALLERY_CAPTURE_DIR ?? "artifacts/s05-gallery";
assert.ok(serverExecutable, "PHOTO_CULLER_SERVER must point to photo-culler-server");
assert.ok(webRoot, "PHOTO_CULLER_WEB_ROOT must point to the staged Slint WASM bundle");
await mkdir(outputDir, { recursive: true });

const server = spawn(serverExecutable, ["--port", "0", "--web-root", webRoot], {
  stdio: ["ignore", "pipe", "pipe"],
});
let stderr = "";
server.stderr.setEncoding("utf8");
server.stderr.on("data", (chunk) => { stderr += chunk; });
let browser;
try {
  const origin = await waitForOrigin(server);
  browser = await chromium.launch({
    headless: true,
    args: ["--enable-webgl", "--use-gl=angle", "--use-angle=swiftshader"],
  });
  const captures = [];
  for (const viewport of [
    { width: 1440, height: 900, name: "pro-library-1440x900.png" },
    { width: 1100, height: 700, name: "pro-library-1100x700.png" },
  ]) {
    const page = await browser.newPage({
      viewport: { width: viewport.width, height: viewport.height },
      deviceScaleFactor: 1,
    });
    const thumbnailResponsePromise = page.waitForResponse(
      (response) => new URL(response.url()).pathname === THUMBNAIL_PATH,
      { timeout: 45_000 },
    );
    await page.goto(`${origin}/?photo-culler-smoke=1`, { waitUntil: "domcontentloaded" });
    const applicationThumbnailResponse = await thumbnailResponsePromise;
    assert.equal(applicationThumbnailResponse.status(), 200, "the application thumbnail route must return HTTP 200");
    const fixtureResponse = await page.request.get(new URL(THUMBNAIL_PATH, origin).toString());
    assert.equal(fixtureResponse.status(), 200, "the same-origin fixture route must return HTTP 200");
    const thumbnailHash = createHash("sha256").update(await fixtureResponse.body()).digest("hex");
    await page.waitForFunction(
      () => document.documentElement.dataset.photoCullerState === "ready",
      undefined,
      { timeout: 45_000 },
    );
    const browserViewport = await page.evaluate(() => ({
      inner_width: window.innerWidth,
      inner_height: window.innerHeight,
      canvas_width: Math.round(document.querySelector("#canvas").getBoundingClientRect().width),
      canvas_height: Math.round(document.querySelector("#canvas").getBoundingClientRect().height),
      canvas_backing_width: document.querySelector("#canvas").width,
      canvas_backing_height: document.querySelector("#canvas").height,
    }));
    assert.deepEqual(
      browserViewport,
      {
        inner_width: viewport.width,
        inner_height: viewport.height,
        canvas_width: viewport.width,
        canvas_height: viewport.height,
        canvas_backing_width: viewport.width,
        canvas_backing_height: viewport.height,
      },
      "the Slint canvas must follow the requested browser viewport",
    );
    assert.match(
      await page.locator("#application-status").textContent(),
      /demo-001\.jpg · 10-card smoke fixture/,
      "the URL selector must activate only the explicit ten-card fixture",
    );
    assert.equal(
      thumbnailHash,
      EXPECTED_JPEG_SHA256,
      `REST JPEG SHA-256 must match the paired React fixture ${EXPECTED_JPEG_SHA256}`,
    );
    const screenshotPath = `${outputDir}/${viewport.name}`;
    const screenshot = await page.screenshot({ path: screenshotPath, fullPage: false });
    assert.ok(screenshot.length > 1024, `${viewport.name} must be a non-empty Slint capture`);
    const screenshotWidth = screenshot.readUInt32BE(16);
    const screenshotHeight = screenshot.readUInt32BE(20);
    assert.deepEqual(
      { width: screenshotWidth, height: screenshotHeight },
      { width: viewport.width, height: viewport.height },
      `${viewport.name} must preserve the requested capture dimensions`,
    );
    captures.push({
      path: screenshotPath,
      width: viewport.width,
      height: viewport.height,
      browser_viewport: browserViewport,
      device_scale_factor: 1,
      selected_fixture: "photo-01",
      photo_count: 10,
      image_sha256: thumbnailHash,
      screenshot_sha256: createHash("sha256").update(screenshot).digest("hex"),
      screenshot_bytes: screenshot.length,
    });
    await page.close();
  }
  const manifest = {
    mode: "explicit browser smoke fixture (?photo-culler-smoke=1)",
    browser: `Chromium ${browser.version()}`,
    server: "local Rust Axum; same-origin WASM/API/JPEG",
    synthetic_metadata: true,
    captures,
  };
  await writeFile(`${outputDir}/manifest.json`, `${JSON.stringify(manifest, null, 2)}\n`);
  console.log(JSON.stringify(manifest, null, 2));
} catch (error) {
  if (stderr.trim()) error.message += `\nRust server stderr:\n${stderr.trim()}`;
  throw error;
} finally {
  await browser?.close();
  await stopServer(server);
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
