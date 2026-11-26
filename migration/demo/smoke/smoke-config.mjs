import assert from "node:assert/strict";

export function buildServerArguments(webRoot, dataRoot, sourceRoot) {
  return [
    "--owned", "--port", "0", "--web-root", webRoot,
    "--data-dir", dataRoot, "--authorized-root", sourceRoot,
  ];
}

export function pairingWarmupDelayMs(browserName) {
  return browserName === "firefox" ? 2000 : 250;
}

export function assertFixtureGalleryPage(page) {
  assert.equal(page.items.length, 1, "the persistent browser fixture must contain one catalog photo");
  assert.equal(page.items[0].name, "demo-001", "the server page contract exposes the imported filename stem in `name`");
}

export function summaryForPhotoCount(totalPhotos) {
  assert.ok(Number.isSafeInteger(totalPhotos) && totalPhotos >= 0, "summary fixture count must be a nonnegative integer");
  return {
    total_photos: totalPhotos,
    total_files: totalPhotos,
    selected_count: 0,
    pending_count: totalPhotos,
    decisions: { undecided: totalPhotos },
    quality_tiers: {},
  };
}

export async function installSummaryFixture(page, totalPhotos) {
  let requests = 0;
  const summary = summaryForPhotoCount(totalPhotos);
  await page.route("**/api/v1/galleries/*/summary", async (route) => {
    requests += 1;
    await route.fulfill({
      status: 200,
      contentType: "application/json; charset=utf-8",
      body: JSON.stringify(summary),
    });
  });
  return () => requests;
}
