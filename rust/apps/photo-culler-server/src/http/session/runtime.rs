//! In-memory bearer sessions and fail-closed HTTP authorization middleware.

use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{Method, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, post},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{AuthorizationError, Operation, Role, Session, SessionStore, authorize};

const SESSION_TTL_SECONDS: u64 = 12 * 60 * 60;
const PAIRING_TTL_SECONDS: u64 = 5 * 60;
const MAX_ACTIVE_SESSIONS: usize = 256;
const MAX_ACTIVE_PAIRING_CODES: usize = 32;

#[derive(Clone)]
pub struct SessionAuth(Arc<Mutex<SessionState>>);

#[derive(Default)]
struct SessionState {
    sessions: HashMap<String, Session>,
    pairing_codes: HashMap<String, u64>,
}

#[derive(Clone, Copy, Debug)]
pub struct StoreUnavailable;

impl SessionStore for SessionAuth {
    type Error = StoreUnavailable;

    fn lookup(&self, credential: &str) -> Result<Option<Session>, Self::Error> {
        let verifier = digest(credential.as_bytes());
        self.0
            .lock()
            .map(|state| state.sessions.get(&verifier).cloned())
            .map_err(|_| StoreUnavailable)
    }
}

pub struct Bootstrap {
    pub auth: SessionAuth,
    /// Secret shown once by the manually started server. It is absent for an
    /// owned child, whose editor token is sent over the private ready pipe.
    pub pairing_code: Option<String>,
    pub session_token: Option<String>,
}

impl Bootstrap {
    pub fn new(owned: bool) -> Result<Self, String> {
        let auth = SessionAuth(Arc::new(Mutex::new(SessionState::default())));
        if owned {
            let (session_token, _) = auth.issue_session(Role::Editor)?;
            Ok(Self {
                auth,
                pairing_code: None,
                session_token: Some(session_token),
            })
        } else {
            let (pairing_code, _) = auth.issue_pairing_code()?;
            Ok(Self {
                auth,
                pairing_code: Some(pairing_code),
                session_token: None,
            })
        }
    }
}

impl SessionAuth {
    fn issue_session(&self, role: Role) -> Result<(String, u64), String> {
        let token = random_secret()?;
        let now = now_seconds();
        let expires_at = now.saturating_add(SESSION_TTL_SECONDS);
        let mut state = self.0.lock().map_err(|_| "session store unavailable")?;
        state.sessions.retain(|_, session| session.expires_at > now);
        if state.sessions.len() >= MAX_ACTIVE_SESSIONS {
            return Err("active session limit reached".to_owned());
        }
        state
            .sessions
            .insert(digest(token.as_bytes()), Session { role, expires_at });
        Ok((token, expires_at))
    }

    fn issue_pairing_code(&self) -> Result<(String, u64), String> {
        let code = random_secret()?;
        let expires_at = now_seconds().saturating_add(PAIRING_TTL_SECONDS);
        let mut state = self.0.lock().map_err(|_| "session store unavailable")?;
        state
            .pairing_codes
            .retain(|_, expiry| *expiry > now_seconds());
        if state.pairing_codes.len() >= MAX_ACTIVE_PAIRING_CODES {
            return Err("active pairing code limit reached".to_owned());
        }
        state
            .pairing_codes
            .insert(digest(code.as_bytes()), expires_at);
        Ok((code, expires_at))
    }

    fn redeem_pairing_code(&self, code: &str) -> Result<Option<(String, u64)>, StoreUnavailable> {
        let verifier = digest(code.as_bytes());
        let now = now_seconds();
        let mut state = self.0.lock().map_err(|_| StoreUnavailable)?;
        let Some(expires_at) = state.pairing_codes.remove(&verifier) else {
            return Ok(None);
        };
        if now >= expires_at {
            return Ok(None);
        }
        drop(state);
        self.issue_session(Role::Editor)
            .map(Some)
            .map_err(|_| StoreUnavailable)
    }

    fn revoke(&self, credential: &str) -> Result<(), StoreUnavailable> {
        let verifier = digest(credential.as_bytes());
        self.0
            .lock()
            .map(|mut state| {
                state.sessions.remove(&verifier);
            })
            .map_err(|_| StoreUnavailable)
    }

    fn generate_pairing_code(&self) -> Result<(String, u64), String> {
        self.issue_pairing_code()
    }
}

pub fn router(auth: SessionAuth) -> Router {
    Router::new()
        .route("/api/v1/session/pair", post(pair::<SessionAuth>))
        .route(
            "/api/v1/session/pairing-codes",
            post(new_pairing_code::<SessionAuth>),
        )
        .route("/api/v1/session", delete(revoke_session::<SessionAuth>))
        .with_state(Arc::new(auth))
}

#[derive(Clone)]
struct MiddlewareState {
    auth: Arc<SessionAuth>,
    public_assets: BTreeSet<String>,
}

pub fn protect(router: Router, auth: SessionAuth, public_assets: BTreeSet<String>) -> Router {
    router.layer(middleware::from_fn_with_state(
        MiddlewareState {
            auth: Arc::new(auth),
            public_assets,
        },
        authorize_request,
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PairRequest {
    code: String,
}

#[derive(Serialize)]
struct PairResponse {
    token: String,
    expires_at: u64,
}

async fn pair<S: PairingStore + Send + Sync + 'static>(
    State(auth): State<Arc<S>>,
    Json(request): Json<PairRequest>,
) -> Response {
    if request.code.len() != 64 || !request.code.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return auth_error(StatusCode::UNAUTHORIZED, "invalid_session");
    }
    match auth.redeem_pairing_code(&request.code) {
        Ok(Some((token, expires_at))) => {
            secret_response(StatusCode::OK, PairResponse { token, expires_at })
        }
        Ok(None) => auth_error(StatusCode::UNAUTHORIZED, "invalid_session"),
        Err(_) => auth_error(StatusCode::SERVICE_UNAVAILABLE, "session_unavailable"),
    }
}

async fn new_pairing_code<S: PairingStore + Send + Sync + 'static>(
    State(auth): State<Arc<S>>,
) -> Response {
    match auth.generate_pairing_code() {
        Ok((code, expires_at)) => {
            secret_response(StatusCode::OK, PairingCodeResponse { code, expires_at })
        }
        Err(_) => auth_error(StatusCode::SERVICE_UNAVAILABLE, "session_unavailable"),
    }
}

#[derive(Serialize)]
struct PairingCodeResponse {
    code: String,
    expires_at: u64,
}

async fn revoke_session<S: PairingStore + Send + Sync + 'static>(
    State(auth): State<Arc<S>>,
    request: Request<Body>,
) -> Response {
    let Some(credential) = bearer_credential(request.headers()) else {
        return auth_error(StatusCode::UNAUTHORIZED, "invalid_session");
    };
    match auth.revoke(&credential) {
        Ok(()) => StatusCode::NO_CONTENT.into_response(),
        Err(_) => auth_error(StatusCode::SERVICE_UNAVAILABLE, "session_unavailable"),
    }
}

trait PairingStore: SessionStore<Error = StoreUnavailable> {
    fn redeem_pairing_code(&self, code: &str) -> Result<Option<(String, u64)>, StoreUnavailable>;
    fn generate_pairing_code(&self) -> Result<(String, u64), StoreUnavailable>;
    fn revoke(&self, credential: &str) -> Result<(), StoreUnavailable>;
}

impl PairingStore for SessionAuth {
    fn redeem_pairing_code(&self, code: &str) -> Result<Option<(String, u64)>, StoreUnavailable> {
        SessionAuth::redeem_pairing_code(self, code)
    }

    fn generate_pairing_code(&self) -> Result<(String, u64), StoreUnavailable> {
        SessionAuth::generate_pairing_code(self).map_err(|_| StoreUnavailable)
    }

    fn revoke(&self, credential: &str) -> Result<(), StoreUnavailable> {
        SessionAuth::revoke(self, credential)
    }
}

async fn authorize_request(
    State(state): State<MiddlewareState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let method = request.method();
    let path = request.uri().path();
    match route_operation(method, path, &state.public_assets) {
        RouteOperation::Public => next.run(request).await,
        RouteOperation::Deny => auth_error(StatusCode::NOT_FOUND, "not_found"),
        RouteOperation::Protected(operation) => {
            let credential = bearer_credential(request.headers());
            match authorize(
                &*state.auth,
                credential.as_deref(),
                operation,
                now_seconds(),
            ) {
                Ok(_) => next.run(request).await,
                Err(AuthorizationError::Forbidden) => {
                    auth_error(StatusCode::FORBIDDEN, "forbidden")
                }
                Err(AuthorizationError::Store(_)) => {
                    auth_error(StatusCode::SERVICE_UNAVAILABLE, "session_unavailable")
                }
                Err(
                    AuthorizationError::MissingCredential
                    | AuthorizationError::InvalidCredential
                    | AuthorizationError::ExpiredCredential,
                ) => auth_error(StatusCode::UNAUTHORIZED, "unauthorized"),
            }
        }
    }
}

enum RouteOperation {
    Public,
    Protected(Operation),
    Deny,
}

fn route_operation(
    method: &Method,
    path: &str,
    public_assets: &BTreeSet<String>,
) -> RouteOperation {
    if matches!(*method, Method::GET | Method::HEAD) && path == "/api/v1/health" {
        return RouteOperation::Public;
    }
    if method == Method::POST && path == "/api/v1/session/pair" {
        return RouteOperation::Public;
    }
    if matches!(*method, Method::GET | Method::HEAD) && public_assets.contains(path) {
        return RouteOperation::Public;
    }
    let segments = segments(path);
    if matches!(*method, Method::GET | Method::HEAD) {
        let known = matches!(
            segments.as_slice(),
            ["api", "v1", "galleries"]
                | ["api", "v1", "galleries", "active"]
                | ["api", "v1", "profiles"]
                | ["api", "v1", "galleries", _, "photos"]
                | ["api", "v1", "galleries", _, "sessions"]
                | ["api", "v1", "galleries", _, "groups"]
                | ["api", "v1", "galleries", _, "summary"]
                | ["api", "v1", "photos", _]
                | ["api", "v1", "photos", _, "thumbnail", _]
                | ["api", "v1", "photos", _, "recipe"]
                | ["api", "v1", "photos", _, "preview"]
                | ["api", "v1", "analysis-jobs", _, _]
                | ["api", "v2", "galleries", _, "sources"]
                | ["api", "v2", "import-jobs"]
                | ["api", "v2", "import-jobs", _]
                | ["api", "v2", "scan-revisions"]
        );
        return if known {
            RouteOperation::Protected(Operation::Read)
        } else {
            RouteOperation::Deny
        };
    }
    if matches!(
        (method, segments.as_slice()),
        (m, ["api", "v1", "session", "pairing-codes"]) if m == Method::POST
    ) {
        return RouteOperation::Protected(Operation::Edit);
    }
    if matches!(
        (method, segments.as_slice()),
        (m, ["api", "v1", "session"]) if m == Method::DELETE
    ) {
        return RouteOperation::Protected(Operation::Edit);
    }
    let edit = match (method, segments.as_slice()) {
        (m, ["api", "v1", "photos", _, "decision"]) if *m == Method::PUT => true,
        (m, ["api", "v1", "photos", _, "recipe"]) if *m == Method::PUT => true,
        (m, ["api", "v1", "photos", _, "recipe", action])
            if *m == Method::POST && matches!(*action, "undo" | "redo") =>
        {
            true
        }
        (m, ["api", "v2", "import-estimates"]) if *m == Method::POST => true,
        (m, ["api", "v2", "galleries", _, "imports" | "rescan"]) if *m == Method::POST => true,
        (m, ["api", "v2", "import-jobs", _, action])
            if *m == Method::POST && matches!(*action, "pause" | "resume" | "cancel") =>
        {
            true
        }
        (m, ["api", "v1", "galleries", _, "analysis-jobs"]) if *m == Method::POST => true,
        (m, ["api", "v1", "analysis-jobs", _, _, action])
            if *m == Method::POST && matches!(*action, "start" | "pause" | "resume" | "cancel") =>
        {
            true
        }
        _ => false,
    };
    if edit {
        return RouteOperation::Protected(Operation::Edit);
    }
    RouteOperation::Deny
}

fn segments(path: &str) -> Vec<&str> {
    path.strip_prefix('/')
        .unwrap_or_default()
        .split('/')
        .collect()
}

fn bearer_credential(headers: &axum::http::HeaderMap) -> Option<String> {
    let values = headers.get_all(axum::http::header::AUTHORIZATION);
    let mut values = values.iter();
    let value = values.next()?.to_str().ok()?;
    if values.next().is_some() {
        return None;
    }
    let (scheme, token) = value.split_once(' ')?;
    (scheme.eq_ignore_ascii_case("bearer")
        && token.len() == 64
        && token.bytes().all(|byte| byte.is_ascii_hexdigit()))
    .then(|| token.to_owned())
}

fn auth_error(status: StatusCode, code: &'static str) -> Response {
    (
        status,
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        Json(serde_json::json!({
            "contract_version": 1,
            "code": code,
            "message": "The session request was rejected.",
            "request_id": "req-session"
        })),
    )
        .into_response()
}

fn secret_response<T: Serialize>(status: StatusCode, body: T) -> Response {
    let mut response = (status, Json(body)).into_response();
    response.headers_mut().insert(
        axum::http::header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    response
}

fn random_secret() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| "secure random source unavailable")?;
    let mut output = String::with_capacity(64);
    for byte in bytes {
        use std::fmt::Write;
        write!(output, "{byte:02x}").map_err(|_| "could not encode session secret")?;
    }
    Ok(output)
}

fn digest(secret: &[u8]) -> String {
    let hash = Sha256::digest(secret);
    let mut output = String::with_capacity(hash.len() * 2);
    for byte in hash {
        use std::fmt::Write;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

fn now_seconds() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or_default()
}
