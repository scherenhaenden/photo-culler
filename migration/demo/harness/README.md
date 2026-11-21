# Sprint Rust/Slint package integration harness (D33)

Run `./migration/demo/harness/verify-packages.sh` from any working directory
after the API, client, server and Slint package owners have supplied their
manifests. The script is intentionally read-only: it checks all four manifests,
then validates the locked workspace metadata and compiles each registered
package. It does not rewrite `rust/Cargo.toml`, `rust/Cargo.lock`, or a package
owner's manifest.

The sprint base currently lacks `photo-culler-api`, `photo-culler-client`,
`photo-culler-server` and `photo-culler-slint` in the Cargo workspace (the API,
client and Slint manifests are also absent). Therefore D33 registration and the
full acceptance run must happen after those owner branches are integrated. Keep
the sprint version at `2026.11.18+18.43.40` when applying those registrations.
Once the wasm target is installed, the script also checks the API for
`wasm32-unknown-unknown`; otherwise it reports that check as pending.
