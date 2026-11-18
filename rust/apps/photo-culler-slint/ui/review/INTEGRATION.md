# Group review UI integration

At base `dfb7434`, the repository has shared Slint `ActionButton`/`DesignTokens`
components and a `photo-culler-client` with gallery-page GET support. The REST
client has no review-group reads, decision writes, or DTO adapter yet, and the
Slint application root and manifest are still pending. This folder contains a
Slint panel and Rust view model with an injected `ReviewApi`; no shared root
module or manifest was changed.

## View and state contract

- `ReviewPanel` displays each member's score, score reasons, current proposal,
  representative and recommended status. It uses the shared keyboard-focusable
  `ActionButton` for decision filters, representative selection, decision
  changes, save and reload; Tab reaches the controls and Enter/Space activates
  them. It binds these controls to the existing callbacks.
- `ReviewController` keeps the last confirmed `ReviewSnapshot` separately from
  the local `ReviewDraft`. Changing a representative or decision changes only
  the draft until the server confirms a write.
- A write carries `group_id` and `expected_revision`. The REST adapter should
  send that revision as the optimistic concurrency precondition (such as
  `If-Match`) and map the server's stale-revision response to `Conflict`.
- `begin_write` returns one request and marks it pending. A second click is
  rejected until `finish_write` handles the response, so the UI dispatches no
  duplicate write. While pending, edit callbacks are disabled.
- On conflict, the draft is retained. `reload` replaces confirmed state from the
  server and retains the draft for explicit comparison/reconciliation; a later
  save uses the refreshed confirmed revision.

## Threading

All REST loads and writes must run off the Slint event thread. In particular,
`load` and `reload` call the injected `ReviewApi` synchronously, so invoke them
from a worker/executor; `begin_write` creates the single pending request for
off-thread dispatch. Marshal each completed result back to the Slint event
thread before calling `finish_write`, updating controller state, or refreshing
component properties. This keeps network latency from blocking input and
painting.

The current `catalog_reads::Group` DTO only contains a recommended photo ID and
count; the review session/decision endpoint contract is not present yet. The
integrator must align `ReviewSnapshot` with the frozen gallery/group DTO, add
review read/write methods and optimistic-conflict mapping to
`photo-culler-client`, add the corresponding server routes, register this
controller and bind `ReviewCard` rows in the Slint application. The Slint app
root and manifest are not yet in the workspace, so visual compilation and
native/WASM interaction remain pending.

## Standalone test

```sh
rustc --edition=2024 --test rust/apps/photo-culler-slint/ui/review/mod.rs \
  -o /private/tmp/photo-culler-task50-review-tests
/private/tmp/photo-culler-task50-review-tests
```

Tests use an in-memory REST double and cover tied scores/reasons, filtering,
representative changes, correcting a rejected decision, stale-version
conflicts, reload with a retained local proposal, and duplicate pending writes.
