// This file only loads Rust/WASM and reports failures before Slint owns the
// canvas. Application API calls (including health) live in Rust.
const diagnostic = document.querySelector("#diagnostic");
const canvas = document.querySelector("#canvas");

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

async function start() {
  if (!canvas) {
    showDiagnostic("Photo Culler cannot start", "The Slint canvas is missing. Reload the page or open Photo Culler Desktop.");
    return;
  }
  if (!webGlAvailable()) {
    showDiagnostic(
      "Photo Culler cannot start in this browser",
      "WebGL is unavailable or disabled. Enable hardware acceleration, reload, or open Photo Culler Desktop.",
    );
    return;
  }

  try {
    const app = await import("./photo_culler_slint.js");
    await app.default();
  } catch (error) {
    console.error("Photo Culler WebAssembly initialization failed:", error);
    showDiagnostic(
      "Photo Culler could not load its web application",
      "The WebAssembly interface could not initialize. Reload this page or open Photo Culler Desktop.",
    );
  }
}

start();
