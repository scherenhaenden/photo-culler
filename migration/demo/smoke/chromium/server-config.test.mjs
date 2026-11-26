import assert from "node:assert/strict";
import test from "node:test";
import { assertFixtureGalleryPage, buildServerArguments } from "../smoke-config.mjs";

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
