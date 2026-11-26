export function pairingWarmupDelayMs(browserName) {
  return browserName === "firefox" ? 2000 : 250;
}
