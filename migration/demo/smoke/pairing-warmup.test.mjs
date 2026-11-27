import assert from "node:assert/strict";
import test from "node:test";
import { pairingWarmupDelayMs } from "./pairing-warmup.mjs";

test("Firefox gets a two-second pairing overlay warmup", () => {
  assert.equal(pairingWarmupDelayMs("firefox"), 2000);
});

test("Chromium keeps its short pairing warmup", () => {
  assert.equal(pairingWarmupDelayMs("chromium"), 250);
});

test("unknown browsers use the short pairing warmup", () => {
  assert.equal(pairingWarmupDelayMs("webkit"), 250);
});

test("empty browser name uses the short pairing warmup", () => {
  assert.equal(pairingWarmupDelayMs(""), 250);
});

test("missing browser name uses the short pairing warmup", () => {
  assert.equal(pairingWarmupDelayMs(undefined), 250);
});
