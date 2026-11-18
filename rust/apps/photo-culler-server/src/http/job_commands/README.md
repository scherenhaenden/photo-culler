# Task 14 integration handoff

This base has no server crate, shared DTOs, router, executor or frozen HTTP contract. `mod.rs` is a standalone provisional adapter, not registered HTTP routes. Integrator must reconcile types, status codes, action idempotency, key scope and endpoint paths with the frozen API, then wire Axum handlers, request IDs, authentication, authorization and the persistent executor. Do not use the illustrative response shape as a finalized public contract.

The executor must atomically persist idempotency keys, request payloads and job IDs across restarts and enforce job transitions. Control-action replay semantics (including manual versus thermal pause) must be agreed with the common job state machine; the fake's behavior is illustrative. Export accepts an approved plan ID, not an arbitrary destination. Domain validation remains the executor's responsibility.

Run standalone acceptance: `rustc --edition=2021 --test rust/apps/photo-culler-server/src/http/job_commands/mod.rs -o /tmp/photo-culler-task14-tests && /tmp/photo-culler-task14-tests`.
