# Shared Slint components — task 41

`components.slint` defines the shared theme and spacing/type tokens, focus policy, action and sidebar controls, and explicit loading, error, and empty states. `ComponentCatalog` is a compact smoke screen that exercises navigation callbacks, the retry action, and switching between light and dark themes. The controls use a focus outline in both themes; tab reaches each action and Enter/Space activates it.

Run the source contract tests with:

```sh
python3 rust/apps/photo-culler-slint/ui/components/test_components.py
```

The current sprint base has no `photo-culler-slint` app, Slint dependency, or component aggregator. To preserve the assigned ownership boundary, this package does not add manifests or root registrations. The source checks cover the exported catalog and its state/focus contract; native and `wasm32` compilation and an interactive keyboard/focus smoke test remain integration checks once the Slint crate and pinned compiler are registered.

Integration: add the `components.slint` import to the shared Slint app, pin the agreed Slint toolchain in its own manifest, expose `ComponentCatalog` to native and browser smoke tests, and verify focus traversal with the real renderer in light and dark modes.
