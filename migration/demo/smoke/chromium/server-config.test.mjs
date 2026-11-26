import assert from "node:assert/strict";
import test from "node:test";
import { buildServerArguments } from "../smoke-config.mjs";

test("live browser smoke starts an authorized persistent catalog for global summaries", () => {
  assert.deepEqual(
    buildServerArguments("/bundle", "/catalog-data", "/fixture-source"),
    [
      "--owned", "--port", "0", "--web-root", "/bundle",
      "--data-dir", "/catalog-data", "--authorized-root", "/fixture-source",
    ],
  );
});
