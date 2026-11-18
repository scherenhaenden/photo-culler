# Task 10: local session and operation authorization

This standalone module supplies the authorization decision and a `SessionStore`
port; the Rust server, launcher, frozen HTTP contracts and shared test harness do
not yet exist at base `a68f360`. It is therefore not registered in a router or
connected to a real credential store.

Integration requirements for the owner of the shared server:

1. Implement `SessionStore` using a secure store shared with the launcher. The
   launcher provisions an unpredictable, short-lived credential and its role and
   expiry through a protected local channel. Authenticate the complete secret,
   never expose or log it, and support revocation. Secure generation, transport,
   persistence and session issuance are integration responsibilities, not
   implemented by this module. A real store must have bounded expiry and reject
   revoked credentials.
2. Authenticate all protected requests with `authorize` using a trusted server
   clock in Unix seconds. Call it before invoking domain operations. Classify
   each route explicitly as `Read`, `Edit` or `AdministerNas`; all NAS parameter
   mutations must use the last category. Do not default an unknown route to read.
   Apply origin/host and browser request protections independently (task 11).
3. Map missing, invalid and expired credentials to the common authentication
   error; map insufficient role to the common forbidden error. Map store failures
   to a generic service error without exposing secrets or internal details.
   HTTP status, error DTO and credential transport await the shared contract.
4. Register the module and add adapter/integration tests against the actual
   launcher, router and store. Verify expiration, revocation, all write routes,
   NAS administration, and that failed authorization cannot invoke handlers.

Self-contained acceptance tests (no Cargo dependencies):

```sh
rustc --edition=2021 --test rust/apps/photo-culler-server/src/http/session/mod.rs -o /tmp/photo-culler-session-tests
/tmp/photo-culler-session-tests
```
