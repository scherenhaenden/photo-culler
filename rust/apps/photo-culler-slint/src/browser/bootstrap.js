const diagnostic = document.querySelector("#diagnostic");
const canvas = document.querySelector("#slint-canvas");

function showDiagnostic(title, message) {
  document.querySelector("#diagnostic-title").textContent = title;
  document.querySelector("#diagnostic-message").textContent = message;
  diagnostic.hidden = false;
  canvas.hidden = true;
}

function webGlAvailable() {
  try {
    const probe = document.createElement("canvas");
    return Boolean(probe.getContext("webgl2") || probe.getContext("webgl"));
  } catch (_) {
    return false;
  }
}

function apiUrl(path) {
  if (typeof path !== "string" || path.startsWith("//") || /^[a-z][a-z0-9+.-]*:/i.test(path)) {
    throw new TypeError("API paths must be relative to this origin");
  }
  const suffix = path.replace(/^\/+/, "");
  if (!suffix || suffix.split("/").includes("..") || suffix.includes("?") || suffix.includes("#")) {
    throw new TypeError("Invalid API path");
  }
  const url = new URL(`/api/v1/${suffix}`, window.location.origin);
  if (url.origin !== window.location.origin) throw new TypeError("Cross-origin API URL rejected");
  return url;
}

/** Same-origin Fetch transport for the versioned API; credentials stay in headers. */
export async function fetchApi(path, init = {}) {
  const url = apiUrl(path);
  const response = await fetch(url, { ...init, credentials: "same-origin" });
  if (!response.ok) throw new Error(`Photo Culler API returned HTTP ${response.status}`);
  return response;
}

async function start() {
  if (!webGlAvailable()) {
    showDiagnostic(
      "Photo Culler cannot start in this browser",
      "WebGL is unavailable or disabled. Enable hardware acceleration, reload, or open Photo Culler Desktop.",
    );
    return;
  }

  try {
    await fetchApi("health");
  } catch (_) {
    showDiagnostic(
      "Photo Culler cannot reach its local service",
      "The local Photo Culler service did not respond. Restart the app or open Photo Culler Desktop.",
    );
    return;
  }

  try {
    // The Slint crate build supplies this generated wasm-bindgen adapter.
    const app = await import("./photo_culler_slint.js");
    await app.default();
    if (typeof app.start !== "function") throw new Error("Missing Slint browser start adapter");
    await app.start(canvas, { fetchApi });
  } catch (_) {
    showDiagnostic(
      "Photo Culler could not load its web application",
      "The WebAssembly interface could not initialize. Reload this page or open Photo Culler Desktop.",
    );
  }
}

start();
