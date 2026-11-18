import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import http from "node:http";
import { firefox } from "playwright";

const jpeg = readFileSync(new URL("./tiny.jpg", import.meta.url));
const wasm = Buffer.from([0, 97, 115, 109, 1, 0, 0, 0]);
const requests = [];
const pageHtml = `<!doctype html><html><body><main id="diagnostic" role="alert" aria-live="assertive" hidden><h1 id="diagnostic-title"></h1><p id="diagnostic-message"></p></main><canvas id="canvas" aria-label="Photo Culler"></canvas><p id="result" role="status">loading</p><img id="thumbnail" alt="demo thumbnail"><script type="module" src="/bootstrap.js"></script></body></html>`;
const bootstrap = `const alertBox=document.querySelector('#diagnostic'); const canvas=document.querySelector('#canvas'); function fail(title,message){document.querySelector('#diagnostic-title').textContent=title;document.querySelector('#diagnostic-message').textContent=message;alertBox.hidden=false;canvas.hidden=true;} async function start(){try{if(location.search.includes('fail=1'))throw Error('simulated WASM init failure');const app=await import('/photo_culler_slint.js');await app.default();}catch(_){fail('Photo Culler could not load its web application','The WebAssembly interface could not initialize. Reload this page or open Photo Culler Desktop.');}} start();`;
// This generated-module fixture emulates the expected WASM application's
// read sequence. It is deliberately not product JavaScript or Rust/WASM proof.
const generatedStub = `export default async function init(){ const wasmResponse=await fetch('/photo_culler_slint_bg.wasm'); if(wasmResponse.headers.get('content-type')!=='application/wasm')throw Error('bad wasm MIME'); await WebAssembly.instantiateStreaming(wasmResponse); const h=await fetch('/api/v1/health'); if(!h.ok)throw Error('health failed'); const g=await fetch('/api/v1/galleries/demo/photos'); if(!g.ok)throw Error('gallery failed'); const page=await g.json(); const image=await fetch(page.items[0].thumbnail_url); if(!image.ok||image.headers.get('content-type')!=='image/jpeg')throw Error('thumbnail failed'); document.querySelector('#thumbnail').src=URL.createObjectURL(await image.blob()); document.querySelector('#result').textContent='ready '+page.items[0].photo_id; document.querySelector('#canvas').dataset.ready='true'; }`;

const server = http.createServer((req, res) => {
  const url = new URL(req.url, "http://127.0.0.1");
  requests.push({ method: req.method, path: url.pathname, origin: req.headers.origin ?? null });
  const send = (status, type, body) => { res.writeHead(status, { "content-type": type, "cache-control": "no-store" }); res.end(body); };
  if (url.pathname === "/") return send(200, "text/html; charset=utf-8", pageHtml);
  if (url.pathname === "/bootstrap.js") return send(200, "text/javascript; charset=utf-8", bootstrap);
  if (url.pathname === "/photo_culler_slint.js") return send(200, "text/javascript; charset=utf-8", generatedStub);
  if (url.pathname === "/photo_culler_slint_bg.wasm") return send(200, "application/wasm", wasm);
  if (url.pathname === "/api/v1/health") return send(200, "application/json", JSON.stringify({ status: "ready", resources: null, request_id: "req-smoke" }));
  if (url.pathname === "/api/v1/galleries/demo/photos") return send(200, "application/json", JSON.stringify({ gallery_id: "demo", items: [{ photo_id: "demo-001", filename: "demo-001.jpg", decision: "undecided", thumbnail_url: "/api/v1/photos/demo-001/thumbnail/800" }], next_cursor: null, total: 1 }));
  if (url.pathname === "/api/v1/photos/demo-001/thumbnail/800") return send(200, "image/jpeg", jpeg);
  return send(404, "application/json", JSON.stringify({ code: "not_found", message: "Not found", request_id: "req-smoke" }));
});

await new Promise(resolve => server.listen(0, "127.0.0.1", resolve));
const origin = `http://127.0.0.1:${server.address().port}`;
let browser;
try {
  browser = await firefox.launch({ headless: true, ...(process.env.FIREFOX_EXECUTABLE ? { executablePath: process.env.FIREFOX_EXECUTABLE } : {}) });
  const page = await browser.newPage();
  page.on("request", request => requests.push({ method: request.method(), path: new URL(request.url()).pathname, origin: new URL(request.url()).origin }));
  await page.goto(origin, { waitUntil: "domcontentloaded" });
  await page.locator("#result").waitFor({ state: "visible" });
  await page.waitForFunction(() => document.querySelector("#result").textContent.startsWith("ready"));
  assert.equal(await page.locator("#thumbnail").evaluate(el => el.naturalWidth), 1);
  assert.equal(await page.locator("#canvas").getAttribute("data-ready"), "true");
  await page.goto(`${origin}/?fail=1`, { waitUntil: "domcontentloaded" });
  await page.getByRole("alert").waitFor({ state: "visible" });
  assert.match(await page.locator("#diagnostic-title").textContent(), /could not load/i);
  const relevant = requests.filter(entry => entry.path?.startsWith("/api/v1/") || entry.path?.endsWith(".wasm"));
  assert.ok(relevant.some(entry => entry.path === "/api/v1/health"));
  assert.ok(relevant.some(entry => entry.path === "/api/v1/galleries/demo/photos"));
  assert.ok(relevant.some(entry => entry.path.endsWith(".wasm")));
  console.log(JSON.stringify({ browser: "Firefox", version: browser.version(), origin, requests: relevant, result: "fixture page ready; thumbnail visible; WASM failure diagnostic visible", application_wasm: false }));
} finally {
  await browser?.close();
  await new Promise(resolve => server.close(resolve));
}
