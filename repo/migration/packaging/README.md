# Rust packaging matrix

This is a candidate workflow and packaging harness for the migration. The
workflow file lives here because task 62 owns `repo/migration/packaging/`; the
integrator must copy it into `.github/workflows/` to activate it. No root Cargo
manifest, lockfile, CI workflow, or release workflow is changed by this task.

## Current matrix state

`targets.toml` is the source of truth. macOS arm64 and Linux x86-64 can attempt
the existing experimental egui executable. The WASM Slint frontend and
standalone NAS server are marked pending because their crates do not exist on
the base commit. The NAS target assumes x86-64 Linux; the deployed NAS model and
CPU architecture still need confirmation. A skipped target is not a successful
build.

The manifest pins Rust 1.97.1 and target triples. CI runner images are pinned
to named GitHub runner generations, but GitHub hosted runner images and system
packages can change. For byte-for-byte rebuilds, replace hosted runners with
digest-pinned containers/VM images and lock the system package repository.
`Cargo.lock` is required with `--locked`. The Python script records the source
commit, compiler, runner OS, UTC build time, and artifact SHA-256. It labels
smoke tests `not-run`; startup checks still belong to integration after the
relevant server/frontend is implemented.

## Commands

```sh
python3 repo/migration/packaging/package.py validate
python3 repo/migration/packaging/package.py build linux-x86_64
python3 repo/migration/packaging/package.py build wasm32-web --allow-pending
```

The Linux and macOS build commands require their native system libraries and
matching host. Cross-host builds are not implied. `--allow-pending` is only for
matrix reporting; it exits successfully while clearly reporting a missing
crate and writes no artifact. Omit it for a release gate so a missing target
fails.

Each completed build is staged under `artifacts/<target>/` with a JSON
provenance manifest and `SHA256SUMS`. These outputs are ignored from source
control and should be uploaded as CI artifacts. No build or runtime startup is
claimed by this commit.
