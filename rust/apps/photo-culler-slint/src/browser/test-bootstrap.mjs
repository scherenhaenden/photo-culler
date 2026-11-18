import test from "node:test";
import assert from "node:assert/strict";
import { readFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const dir = dirname(fileURLToPath(import.meta.url));
const bootstrap = await readFile(resolve(dir, "bootstrap.js"), "utf8");
const html = await readFile(resolve(dir, "index.html"), "utf8");

test("bootstrap only initializes generated WASM and contains no application API transport", () => {
  assert.match(bootstrap, /import\("\.\/photo_culler_slint\.js"\)/);
  assert.match(bootstrap, /app\.default\(\)/);
  assert.doesNotMatch(bootstrap, /fetch\s*\(/);
  assert.doesNotMatch(bootstrap, /api\/v1/);
});

test("Slint receives its required canvas and diagnostics remain outside it", () => {
  assert.match(html, /<canvas id="canvas"/);
  assert.match(html, /id="diagnostic" role="alert"/);
  assert.doesNotMatch(html, /id="slint-canvas"/);
});
