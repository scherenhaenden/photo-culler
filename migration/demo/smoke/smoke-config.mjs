import assert from "node:assert/strict";

export function buildServerArguments(webRoot, dataRoot, sourceRoot) {
  return [
    "--owned", "--port", "0", "--web-root", webRoot,
    "--data-dir", dataRoot, "--authorized-root", sourceRoot,
  ];
}

export function assertFixtureGalleryPage(page) {
  assert.equal(page.items.length, 1, "the persistent browser fixture must contain one catalog photo");
  assert.equal(page.items[0].name, "demo-001", "the server page contract exposes the imported filename stem in `name`");
}
