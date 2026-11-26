export function buildServerArguments(webRoot, dataRoot, sourceRoot) {
  return [
    "--owned", "--port", "0", "--web-root", webRoot,
    "--data-dir", dataRoot, "--authorized-root", sourceRoot,
  ];
}
