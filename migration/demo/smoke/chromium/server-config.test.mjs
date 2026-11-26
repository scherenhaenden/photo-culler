import assert from "node:assert/strict";
import test from "node:test";
import { assertFixtureGalleryPage, buildServerArguments, installSummaryFixture, summaryForPhotoCount } from "../smoke-config.mjs";

test("live browser smoke starts an authorized persistent catalog for global summaries", () => {
  assert.deepEqual(
    buildServerArguments("/bundle", "/catalog-data", "/fixture-source"),
    [
      "--owned", "--port", "0", "--web-root", "/bundle",
      "--data-dir", "/catalog-data", "--authorized-root", "/fixture-source",
    ],
  );
});

test("persistent gallery fixture validates the catalog DTO filename field", () => {
  assert.doesNotThrow(() => assertFixtureGalleryPage({ items: [{ id: "photo-1", name: "demo-001" }] }));
  assert.throws(
    () => assertFixtureGalleryPage({ items: [{ id: "photo-1", filename: "demo-001" }] }),
    /name/,
  );
});

test("UI state fixture returns a valid global summary for empty and populated galleries", () => {
  assert.deepEqual(summaryForPhotoCount(0), {
    total_photos: 0,
    total_files: 0,
    selected_count: 0,
    pending_count: 0,
    decisions: { undecided: 0 },
    quality_tiers: {},
  });
  assert.deepEqual(summaryForPhotoCount(1), {
    total_photos: 1,
    total_files: 1,
    selected_count: 0,
    pending_count: 1,
    decisions: { undecided: 1 },
    quality_tiers: {},
  });
  assert.throws(() => summaryForPhotoCount(-1), /nonnegative/);
});

test("summary fixture intercepts the endpoint before the UI load and returns JSON", async () => {
  let pattern;
  let response;
  const page = {
    async route(registeredPattern, handler) {
      pattern = registeredPattern;
      await handler({ fulfill: async (value) => { response = value; } });
    },
  };
  const requests = await installSummaryFixture(page, 1);
  assert.equal(pattern, "**/api/v1/galleries/*/summary");
  assert.equal(requests(), 1);
  assert.equal(response.status, 200);
  assert.match(response.contentType, /^application\/json/);
  assert.deepEqual(JSON.parse(response.body), summaryForPhotoCount(1));
});
