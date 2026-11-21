//! Operational CLI command contract and REST adapter boundary.
//!
//! The command map is the subset currently backed by the Rust server API.

use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub command: String,
    pub args: Vec<String>,
    pub catalog: String,
    pub server: String,
    pub credential_file: Option<PathBuf>,
    pub dry_run: bool,
}

impl Invocation {
    pub fn parse(argv: &[String]) -> Result<Self, CliError> {
        let mut command = None;
        let mut args = Vec::new();
        let mut catalog = "catalog.db".to_owned();
        let mut catalog_provided = false;
        let mut server = "http://127.0.0.1:8765".to_owned();
        let mut credential_file = None;
        let mut dry_run = false;
        let mut i = 0;
        while i < argv.len() {
            if command.is_some() {
                args.push(argv[i].clone());
                i += 1;
                continue;
            }
            match argv[i].as_str() {
                "--catalog" | "-c" => {
                    i += 1;
                    catalog_provided = true;
                    catalog = argv
                        .get(i)
                        .ok_or(CliError::Usage("--catalog needs a path"))?
                        .clone();
                }
                "--server" => {
                    i += 1;
                    server = argv
                        .get(i)
                        .ok_or(CliError::Usage("--server needs a URL"))?
                        .clone();
                }
                "--credential-file" => {
                    i += 1;
                    credential_file = Some(PathBuf::from(
                        argv.get(i)
                            .ok_or(CliError::Usage("--credential-file needs a path"))?
                            .clone(),
                    ));
                }
                "--dry-run" => dry_run = true,
                flag if flag.starts_with('-') => {
                    return Err(CliError::Usage(
                        "unknown option; API options must precede a server command",
                    ));
                }
                value => command = Some(value.to_owned()),
            }
            i += 1;
        }
        let command = command.ok_or(CliError::Usage("missing command"))?;
        if !is_supported(&command) {
            return Err(CliError::Usage("unknown command"));
        }
        let invocation = Self {
            command,
            args,
            catalog,
            server,
            credential_file,
            dry_run,
        };
        if invocation.command == "auth" {
            if catalog_provided {
                return Err(CliError::Usage("--catalog is not used by auth commands"));
            }
            if !matches!(invocation.args.as_slice(), [action] if matches!(action.as_str(), "pair" | "pairing-code" | "logout"))
            {
                return Err(CliError::Usage(
                    "auth expects one of: pair, pairing-code, logout",
                ));
            }
            if invocation.dry_run {
                return Err(CliError::Usage("auth commands do not support --dry-run"));
            }
            return Ok(invocation);
        }
        if matches!(invocation.command.as_str(), "doctor" | "diagnostics")
            && !invocation.args.is_empty()
        {
            return Err(CliError::Usage("doctor/diagnostics takes no arguments"));
        }
        if catalog_provided && !matches!(invocation.command.as_str(), "doctor" | "diagnostics") {
            return Err(CliError::Usage(
                "--catalog is only used by doctor/diagnostics",
            ));
        }
        // Catch missing positional/options before any network call so the
        // process can return the documented usage status immediately.
        if !matches!(invocation.command.as_str(), "doctor" | "diagnostics") {
            request_for(&invocation)?;
        }
        Ok(invocation)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    Usage(&'static str),
    ServerUnavailable(String),
    CredentialStore(String),
    ServerRejected { status: u16, message: String },
}

impl CliError {
    /// Stable process codes: syntax=2, unavailable server=3, remote failure=4.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            Self::ServerUnavailable(_) | Self::CredentialStore(_) => 3,
            Self::ServerRejected { .. } => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: Method,
    pub path: String,
    pub body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub body: String,
}

pub trait RestClient {
    fn request(&mut self, base_url: &str, request: Request) -> Result<Response, String>;
}

/// Synchronous HTTP adapter used by the command-line binary.
pub struct UreqRestClient {
    pub agent: ureq::Agent,
    bearer: Option<String>,
}

impl UreqRestClient {
    pub fn new() -> Self {
        Self {
            agent: ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(2)))
                .timeout_recv_response(Some(Duration::from_secs(5)))
                .timeout_recv_body(Some(Duration::from_secs(10)))
                .http_status_as_error(false)
                .build()
                .new_agent(),
            bearer: None,
        }
    }

    pub fn with_bearer(bearer: Option<String>) -> Self {
        let mut client = Self::new();
        client.bearer = bearer;
        client
    }
}

impl RestClient for UreqRestClient {
    fn request(&mut self, base_url: &str, request: Request) -> Result<Response, String> {
        let (origin, secure, loopback) = parse_server_origin(base_url)?;
        let contains_pairing_secret = request.path == "/api/v1/session/pair";
        if (self.bearer.is_some() || contains_pairing_secret) && !secure && !loopback {
            return Err(
                "refusing to send session credentials over non-loopback HTTP; use HTTPS".into(),
            );
        }
        let url = format!("{origin}{}", request.path);
        let response = match (request.method, request.body) {
            (Method::Get, None) => {
                let builder = self.agent.get(&url).header("Origin", origin);
                match self.bearer.as_deref() {
                    Some(token) => builder
                        .header("Authorization", &format!("Bearer {token}"))
                        .call(),
                    None => builder.call(),
                }
            }
            (Method::Get, Some(_)) => return Err("GET requests cannot include a body".into()),
            (Method::Post, Some(body)) => {
                let builder = self
                    .agent
                    .post(&url)
                    .header("Origin", origin)
                    .content_type("application/json");
                match self.bearer.as_deref() {
                    Some(token) => builder
                        .header("Authorization", &format!("Bearer {token}"))
                        .send(body),
                    None => builder.send(body),
                }
            }
            (Method::Post, None) => {
                let builder = self.agent.post(&url).header("Origin", origin);
                match self.bearer.as_deref() {
                    Some(token) => builder
                        .header("Authorization", &format!("Bearer {token}"))
                        .send(""),
                    None => builder.send(""),
                }
            }
            (Method::Put, Some(body)) => {
                let builder = self
                    .agent
                    .put(&url)
                    .header("Origin", origin)
                    .content_type("application/json");
                match self.bearer.as_deref() {
                    Some(token) => builder
                        .header("Authorization", &format!("Bearer {token}"))
                        .send(body),
                    None => builder.send(body),
                }
            }
            (Method::Put, None) => return Err("PUT requests require a body".into()),
            (Method::Delete, None) => {
                let builder = self.agent.delete(&url).header("Origin", origin);
                match self.bearer.as_deref() {
                    Some(token) => builder
                        .header("Authorization", &format!("Bearer {token}"))
                        .call(),
                    None => builder.call(),
                }
            }
            (Method::Delete, Some(_)) => {
                return Err("DELETE requests with a body are unsupported".into());
            }
        }
        .map_err(|error| error.to_string())?;
        let mut response = response;
        let status = response.status();
        let body = response
            .body_mut()
            .read_to_string()
            .map_err(|error| error.to_string())?;
        Ok(Response {
            status: status.as_u16(),
            body,
        })
    }
}

fn parse_server_origin(value: &str) -> Result<(&str, bool, bool), String> {
    let origin = value.trim_end_matches('/');
    let (secure, authority) = if let Some(authority) = origin.strip_prefix("https://") {
        (true, authority)
    } else if let Some(authority) = origin.strip_prefix("http://") {
        (false, authority)
    } else {
        return Err("server must be an http or https origin".into());
    };
    if authority.is_empty()
        || authority.contains(['/', '?', '#', '@', '\\'])
        || authority.bytes().any(|byte| byte.is_ascii_whitespace())
    {
        return Err(
            "server must be an origin without credentials, path, query, or fragment".into(),
        );
    }
    let host = if authority.starts_with('[') {
        let close = authority
            .find(']')
            .ok_or_else(|| "server IPv6 origin is invalid".to_owned())?;
        let host = &authority[1..close];
        let suffix = &authority[close + 1..];
        if !suffix.is_empty()
            && (!suffix.starts_with(':')
                || suffix[1..]
                    .parse::<u16>()
                    .ok()
                    .filter(|port| *port > 0)
                    .is_none())
        {
            return Err("server port is invalid".into());
        }
        host
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        if host.contains(':') || port.parse::<u16>().ok().filter(|port| *port > 0).is_none() {
            return Err("server host or port is invalid".into());
        }
        host
    } else {
        authority
    };
    if host.is_empty() {
        return Err("server host is empty".into());
    }
    let loopback = host
        .parse::<std::net::IpAddr>()
        .is_ok_and(|address| address.is_loopback());
    Ok((origin, secure, loopback))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: u8,
}

/// Execute a parsed command. `doctor`/`diagnostics` remain useful offline and
/// turn transport failure into a successful diagnostic report.
pub fn execute(client: &mut impl RestClient, invocation: &Invocation) -> CommandOutput {
    if invocation.command == "auth" {
        return execute_auth(invocation);
    }
    if matches!(invocation.command.as_str(), "doctor" | "diagnostics") {
        return diagnostic(client, invocation);
    }
    match request_for(invocation) {
        Err(error) => failure(error),
        Ok(request) if invocation.dry_run && request.method != Method::Get => CommandOutput {
            stdout: format!(
                "dry run: {} {}{}",
                method_name(request.method),
                request.path,
                request
                    .body
                    .map(|body| format!(" {body}"))
                    .unwrap_or_default()
            ),
            stderr: String::new(),
            exit_code: 0,
        },
        Ok(request) => match client.request(&invocation.server, request) {
            Err(message) => failure(CliError::ServerUnavailable(message)),
            Ok(response) if (200..300).contains(&response.status) => CommandOutput {
                stdout: response.body,
                stderr: String::new(),
                exit_code: 0,
            },
            Ok(response) => failure(CliError::ServerRejected {
                status: response.status,
                message: response.body,
            }),
        },
    }
}

/// Load the caller's bearer without placing it in a command argument or URL.
/// Token files are restricted to the current user on Unix; an environment
/// variable is accepted for short-lived, externally managed invocations.
pub fn load_bearer(invocation: &Invocation) -> Result<Option<String>, CliError> {
    if let Some(path) = invocation.credential_file.as_deref() {
        return read_credential_file(path)
            .map(Some)
            .map_err(CliError::CredentialStore);
    }
    match std::env::var("PHOTO_CULLER_BEARER") {
        Ok(token) => validate_token(&token)
            .map(|()| Some(token))
            .map_err(CliError::CredentialStore),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(std::env::VarError::NotUnicode(_)) => Err(CliError::CredentialStore(
            "PHOTO_CULLER_BEARER is not valid UTF-8".into(),
        )),
    }
}

fn execute_auth(invocation: &Invocation) -> CommandOutput {
    let action = invocation
        .args
        .first()
        .map(String::as_str)
        .unwrap_or_default();
    let path = match invocation.credential_file.as_deref() {
        Some(path) => path,
        None => {
            return failure(CliError::Usage(
                "auth requires --credential-file PATH; the token is never accepted as an argument",
            ));
        }
    };
    match action {
        "pair" => pair_cli(invocation, path),
        "pairing-code" => issue_pairing_code(invocation, path),
        "logout" => logout_cli(invocation, path),
        _ => failure(CliError::Usage("unsupported auth command")),
    }
}

fn pair_cli(invocation: &Invocation, path: &Path) -> CommandOutput {
    let mut code = String::new();
    if let Err(error) = std::io::stdin().read_line(&mut code) {
        return failure(CliError::CredentialStore(format!(
            "could not read one-time pairing code from stdin: {error}"
        )));
    }
    let code = code.trim();
    if code.len() != 64 || !code.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return failure(CliError::Usage(
            "pairing code must be 64 hexadecimal characters read from stdin",
        ));
    }
    let body = serde_json::json!({ "code": code }).to_string();
    let mut client = UreqRestClient::new();
    let response = match client.request(
        &invocation.server,
        Request {
            method: Method::Post,
            path: "/api/v1/session/pair".into(),
            body: Some(body),
        },
    ) {
        Ok(response) => response,
        Err(error) => return failure(CliError::ServerUnavailable(error)),
    };
    if !(200..300).contains(&response.status) {
        return failure(CliError::ServerRejected {
            status: response.status,
            message: "pairing request rejected".into(),
        });
    }
    let paired: serde_json::Value = match serde_json::from_str(&response.body) {
        Ok(paired) => paired,
        Err(_) => {
            return failure(CliError::ServerRejected {
                status: response.status,
                message: "server returned an invalid pairing response".into(),
            });
        }
    };
    let Some(token) = paired.get("token").and_then(serde_json::Value::as_str) else {
        return failure(CliError::ServerRejected {
            status: response.status,
            message: "server returned an invalid pairing response".into(),
        });
    };
    let Some(expires_at) = paired.get("expires_at").and_then(serde_json::Value::as_u64) else {
        return failure(CliError::ServerRejected {
            status: response.status,
            message: "server returned an invalid pairing response".into(),
        });
    };
    if let Err(error) = save_credential_file(path, token) {
        return failure(CliError::CredentialStore(error));
    }
    CommandOutput {
        stdout: format!(
            "paired; credential saved to {}; expires at {}",
            path.display(),
            expires_at
        ),
        stderr: String::new(),
        exit_code: 0,
    }
}

fn issue_pairing_code(invocation: &Invocation, path: &Path) -> CommandOutput {
    let token = match read_credential_file(path) {
        Ok(token) => token,
        Err(error) => return failure(CliError::CredentialStore(error)),
    };
    let mut client = UreqRestClient::with_bearer(Some(token));
    let response = match client.request(
        &invocation.server,
        Request {
            method: Method::Post,
            path: "/api/v1/session/pairing-codes".into(),
            body: None,
        },
    ) {
        Ok(response) => response,
        Err(error) => return failure(CliError::ServerUnavailable(error)),
    };
    if !(200..300).contains(&response.status) {
        return failure(CliError::ServerRejected {
            status: response.status,
            message: "pairing-code request rejected".into(),
        });
    }
    let pairing: serde_json::Value = match serde_json::from_str(&response.body) {
        Ok(pairing) => pairing,
        Err(_) => {
            return failure(CliError::ServerRejected {
                status: response.status,
                message: "server returned an invalid pairing-code response".into(),
            });
        }
    };
    let Some(code) = pairing.get("code").and_then(serde_json::Value::as_str) else {
        return failure(CliError::ServerRejected {
            status: response.status,
            message: "server returned an invalid pairing-code response".into(),
        });
    };
    if code.len() != 64 || !code.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return failure(CliError::ServerRejected {
            status: response.status,
            message: "server returned an invalid pairing code".into(),
        });
    }
    let Some(expires_at) = pairing
        .get("expires_at")
        .and_then(serde_json::Value::as_u64)
    else {
        return failure(CliError::ServerRejected {
            status: response.status,
            message: "server returned an invalid pairing-code response".into(),
        });
    };
    CommandOutput {
        stdout: format!("{code}\nexpires at {expires_at}"),
        stderr: "one-time pairing code; share it only with the intended client".into(),
        exit_code: 0,
    }
}

fn logout_cli(invocation: &Invocation, path: &Path) -> CommandOutput {
    let token = match read_credential_file(path) {
        Ok(token) => token,
        Err(error) => return failure(CliError::CredentialStore(error)),
    };
    let mut client = UreqRestClient::with_bearer(Some(token));
    let response = match client.request(
        &invocation.server,
        Request {
            method: Method::Delete,
            path: "/api/v1/session".into(),
            body: None,
        },
    ) {
        Ok(response) => Some(response),
        Err(_) => None,
    };
    let revocation_error = response.as_ref().and_then(|response| {
        (!(200..300).contains(&response.status)).then(|| CliError::ServerRejected {
            status: response.status,
            message: "session revocation rejected".into(),
        })
    });
    if let Err(error) = fs::remove_file(path) {
        return failure(CliError::CredentialStore(format!(
            "could not remove the local credential file: {error}"
        )));
    }
    if let Some(error) = revocation_error {
        return failure(error);
    }
    if response.is_none() {
        return failure(CliError::ServerUnavailable(
            "server could not be reached; local credential removed, remote session may remain valid until expiry".into(),
        ));
    }
    CommandOutput {
        stdout: "session revoked and local credential removed".into(),
        stderr: String::new(),
        exit_code: 0,
    }
}

fn read_credential_file(path: &Path) -> Result<String, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("could not inspect credential file: {error}"))?;
    if !metadata.is_file() {
        return Err("credential path must be a regular non-symlink file".into());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("credential file must not be accessible by group or others".into());
        }
    }
    let mut token = String::new();
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW);
    }
    options
        .open(path)
        .and_then(|file| file.take(256).read_to_string(&mut token))
        .map_err(|error| format!("could not read credential file: {error}"))?;
    if token.len() > 128 {
        return Err("credential file exceeds the allowed size".into());
    }
    let token = token.trim();
    validate_token(token)?;
    Ok(token.to_owned())
}

fn save_credential_file(path: &Path, token: &str) -> Result<(), String> {
    validate_token(token)?;
    if let Ok(metadata) = fs::symlink_metadata(path)
        && !metadata.is_file()
    {
        return Err("credential path must not be a symlink or special file".into());
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("could not create credential directory: {error}"))?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
    }
    let mut file = options
        .open(path)
        .map_err(|error| format!("could not open credential file: {error}"))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("could not restrict credential-file permissions: {error}"))?;
    }
    file.write_all(token.as_bytes())
        .and_then(|()| file.write_all(b"\n"))
        .and_then(|()| file.sync_all())
        .map_err(|error| format!("could not write credential file: {error}"))
}

fn validate_token(token: &str) -> Result<(), String> {
    if token.len() == 64 && token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("credential must be a 64-character hexadecimal bearer token".into())
    }
}

fn method_name(method: Method) -> &'static str {
    match method {
        Method::Get => "GET",
        Method::Post => "POST",
        Method::Put => "PUT",
        Method::Delete => "DELETE",
    }
}

fn diagnostic(client: &mut impl RestClient, invocation: &Invocation) -> CommandOutput {
    let health = client.request(
        &invocation.server,
        Request {
            method: Method::Get,
            path: "/api/v1/health".into(),
            body: None,
        },
    );
    let (server, exit_code) = match health {
        Ok(response) if (200..300).contains(&response.status) => {
            (format!("online: {}", response.body), 0)
        }
        Ok(response) => (
            format!("unavailable: HTTP {} {}", response.status, response.body),
            0,
        ),
        Err(error) => (format!("offline: {error}"), 0),
    };
    CommandOutput {
        stdout: format!(
            "photo-culler diagnostics\nserver: {server}\ncatalog: {}",
            invocation.catalog
        ),
        stderr: String::new(),
        exit_code,
    }
}

fn request_for(invocation: &Invocation) -> Result<Request, CliError> {
    let arg = |index: usize, usage| {
        invocation
            .args
            .get(index)
            .map(String::as_str)
            .ok_or(CliError::Usage(usage))
    };
    let (method, path, body) = match invocation.command.as_str() {
        "status" => {
            ensure_arg_count(&invocation.args, 0, "status takes no arguments")?;
            (Method::Get, "/api/v1/health".into(), None)
        }
        "active-gallery" => {
            ensure_arg_count(&invocation.args, 0, "active-gallery takes no arguments")?;
            (Method::Get, "/api/v1/galleries/active".into(), None)
        }
        "galleries" if invocation.args.is_empty() => {
            (Method::Get, "/api/v1/galleries".into(), None)
        }
        "profiles" => {
            ensure_arg_count(&invocation.args, 0, "profiles takes no arguments")?;
            (Method::Get, "/api/v1/profiles".into(), None)
        }
        "photo" => {
            ensure_arg_count(&invocation.args, 1, "photo expects PHOTO_ID")?;
            let photo = path_id(arg(0, "photo PHOTO_ID needs a photo ID")?)?;
            (Method::Get, format!("/api/v1/photos/{photo}"), None)
        }
        "summary" | "sessions" | "groups" => {
            ensure_arg_count(&invocation.args, 1, "command expects GALLERY_ID")?;
            let gallery = path_id(arg(0, "command needs a gallery ID")?)?;
            let resource = match invocation.command.as_str() {
                "summary" => "summary",
                "sessions" => "sessions",
                "groups" => "groups",
                _ => unreachable!(),
            };
            (
                Method::Get,
                format!("/api/v1/galleries/{gallery}/{resource}"),
                None,
            )
        }
        "photos" => {
            photos_request(&invocation.args)?
        }
        "sources" => {
            ensure_arg_count(&invocation.args, 1, "sources expects GALLERY_ID")?;
            let gallery = path_id(arg(0, "sources GALLERY_ID needs a gallery ID")?)?;
            (
                Method::Get,
                format!("/api/v2/galleries/{gallery}/sources"),
                None,
            )
        }
        "estimate" => {
            ensure_arg_count(&invocation.args, 1, "estimate expects SOURCE_ID")?;
            let source = arg(0, "estimate SOURCE_ID needs a source ID")?;
            path_id(source)?;
            (
                Method::Post,
                "/api/v2/import-estimates".into(),
                Some(format!(
                    "{{\"source_id\":\"{}\",\"recursive\":true}}",
                    json_escape(source)
                )),
            )
        }
        "import" => {
            ensure_arg_count(&invocation.args, 2, "import expects GALLERY_ID SOURCE_ID")?;
            let gallery = path_id(arg(0, "import GALLERY_ID SOURCE_ID needs a gallery ID")?)?;
            let source = arg(1, "import GALLERY_ID SOURCE_ID needs a source ID")?;
            path_id(source)?;
            (
                Method::Post,
                format!("/api/v2/galleries/{gallery}/imports"),
                Some(format!(
                    "{{\"source_id\":\"{}\",\"recursive\":true}}",
                    json_escape(source)
                )),
            )
        }
        "scan" => {
            ensure_arg_count(&invocation.args, 1, "scan expects GALLERY_ID")?;
            let gallery = path_id(arg(0, "scan GALLERY_ID needs a gallery ID")?)?;
            (
                Method::Post,
                format!("/api/v2/galleries/{gallery}/rescan"),
                None,
            )
        }
        "jobs" if invocation.args.is_empty() => (Method::Get, "/api/v2/import-jobs".into(), None),
        "job" => {
            let id = path_id(arg(0, "job JOB_ID [pause|resume|cancel] needs a job ID")?)?;
            match invocation.args.get(1).map(String::as_str) {
                None => (Method::Get, format!("/api/v2/import-jobs/{id}"), None),
                Some(action @ ("pause" | "resume" | "cancel")) if invocation.args.len() == 2 => (
                    Method::Post,
                    format!("/api/v2/import-jobs/{id}/{action}"),
                    None,
                ),
                _ => {
                    return Err(CliError::Usage(
                        "job action must be pause, resume, or cancel",
                    ));
                }
            }
        }
        "revisions" => {
            ensure_arg_count(&invocation.args, 1, "revisions expects GALLERY_ID")?;
            let gallery = path_id(arg(0, "revisions GALLERY_ID needs a gallery ID")?)?;
            (
                Method::Get,
                format!("/api/v2/scan-revisions?gallery_id={gallery}"),
                None,
            )
        }
        "decision" => {
            let photo = path_id(arg(0, "decision PHOTO_ID STATE REVISION needs a photo ID")?)?;
            let decision = arg(1, "decision PHOTO_ID STATE REVISION needs a state")?;
            let decision = match decision.to_ascii_lowercase().as_str() {
                "keep" => "keep",
                "alt" | "alternate" => "alt",
                "reject" => "reject",
                "undecided" => "undecided",
                _ => {
                    return Err(CliError::Usage(
                        "decision state must be keep, alt, reject, or undecided",
                    ));
                }
            };
            let revision = arg(
                2,
                "decision PHOTO_ID STATE REVISION needs an expected revision",
            )?
            .parse::<u64>()
            .map_err(|_| CliError::Usage("decision revision must be an unsigned integer"))?;
            if invocation.args.len() != 3 {
                return Err(CliError::Usage("decision expects PHOTO_ID STATE REVISION"));
            }
            let request_id = next_request_id();
            (
                Method::Put,
                format!("/api/v1/photos/{photo}/decision"),
                Some(format!(
                    "{{\"decision\":\"{decision}\",\"expected_revision\":{revision},\"request_id\":\"{request_id}\"}}"
                )),
            )
        }
        "doctor" | "diagnostics" => {
            return Err(CliError::Usage("diagnostic command is handled offline"));
        }
        "backends" | "frontends" | "config" | "init" | "volumes" | "analyze" | "group"
        | "bursts" | "decisions" | "preselect" | "verify" | "evaluate" | "report" | "nas" => {
            return Err(CliError::Usage(
                "this command is not implemented by the integrated Rust server yet",
            ));
        }
        _ => return Err(CliError::Usage("unsupported command arguments")),
    };
    Ok(Request { method, path, body })
}

fn photos_request(args: &[String]) -> Result<(Method, String, Option<String>), CliError> {
    let gallery = path_id(
        args.first()
            .map(String::as_str)
            .ok_or(CliError::Usage("photos expects GALLERY_ID [options]"))?,
    )?;
    let mut limit = None;
    let mut decision = None;
    let mut cursor = None;
    let mut index = 1;
    while index < args.len() {
        let option = args[index].as_str();
        let value = args
            .get(index + 1)
            .ok_or(CliError::Usage("photos option needs a value"))?;
        match option {
            "--limit" if limit.is_none() => {
                let parsed = value
                    .parse::<usize>()
                    .map_err(|_| CliError::Usage("photos --limit must be an integer from 1 to 200"))?;
                if !(1..=200).contains(&parsed) {
                    return Err(CliError::Usage(
                        "photos --limit must be an integer from 1 to 200",
                    ));
                }
                limit = Some(parsed);
            }
            "--decision" if decision.is_none() => {
                decision = Some(normalize_photo_decision(value)?);
            }
            "--cursor" if cursor.is_none() => {
                if value.len() > 4096 {
                    return Err(CliError::Usage("photos --cursor exceeds 4096 bytes"));
                }
                cursor = Some(value.clone());
            }
            "--limit" | "--decision" | "--cursor" => {
                return Err(CliError::Usage("photos options may only be specified once"));
            }
            _ => {
                return Err(CliError::Usage(
                    "photos options are --limit N, --decision STATE, and --cursor JSON",
                ));
            }
        }
        index += 2;
    }

    let cursor_decision = cursor
        .as_deref()
        .map(|value| validate_photo_cursor(value, gallery))
        .transpose()?
        .flatten();
    if let (Some(requested), Some(from_cursor)) = (decision, cursor_decision) {
        if requested != from_cursor {
            return Err(CliError::Usage(
                "photos cursor filter does not match --decision",
            ));
        }
    }
    let decision = decision.or(cursor_decision);
    let mut query = Vec::new();
    if let Some(limit) = limit {
        query.push(format!("limit={limit}"));
    }
    if let Some(decision) = decision {
        query.push(format!("decision={decision}"));
    }
    if let Some(cursor) = cursor {
        query.push(format!("cursor={}", percent_encode_query_value(&cursor)));
    }
    let mut path = format!("/api/v1/galleries/{gallery}/photos");
    if !query.is_empty() {
        path.push('?');
        path.push_str(&query.join("&"));
    }
    Ok((Method::Get, path, None))
}

fn normalize_photo_decision(value: &str) -> Result<&'static str, CliError> {
    match value.to_ascii_lowercase().as_str() {
        "keep" | "best" | "protected_by_coverage" => Ok("keep"),
        "alt" | "alternate" => Ok("alt"),
        "reject" | "reject_technical" | "reject_redundant" => Ok("reject"),
        "undecided" | "review" | "unprocessed" => Ok("undecided"),
        _ => Err(CliError::Usage(
            "photos decision must be keep, alt, reject, or undecided",
        )),
    }
}

fn validate_photo_cursor(value: &str, gallery_id: &str) -> Result<Option<&'static str>, CliError> {
    let cursor: serde_json::Value = serde_json::from_str(value)
        .map_err(|_| CliError::Usage("photos --cursor must be a valid page cursor JSON object"))?;
    let filter = cursor
        .get("filter")
        .and_then(serde_json::Value::as_object)
        .ok_or(CliError::Usage("photos cursor is missing its filter object"))?;
    let cursor_gallery = filter
        .get("gallery_id")
        .and_then(serde_json::Value::as_str)
        .ok_or(CliError::Usage("photos cursor is missing filter.gallery_id"))?;
    if cursor_gallery != gallery_id {
        return Err(CliError::Usage(
            "photos cursor belongs to a different gallery",
        ));
    }
    let after_id = cursor
        .get("after_id")
        .and_then(serde_json::Value::as_str)
        .ok_or(CliError::Usage("photos cursor is missing after_id"))?;
    path_id(after_id)?;
    match filter.get("decision") {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::String(value)) => Ok(Some(normalize_photo_decision(value)?)),
        _ => Err(CliError::Usage("photos cursor has an invalid decision filter")),
    }
}

fn percent_encode_query_value(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            encoded.push(char::from(byte));
        } else {
            use std::fmt::Write;
            write!(encoded, "%{byte:02X}").expect("writing to String cannot fail");
        }
    }
    encoded
}

fn is_supported(command: &str) -> bool {
    matches!(
        command,
        "status"
            | "active-gallery"
            | "doctor"
            | "diagnostics"
            | "galleries"
            | "profiles"
            | "photo"
            | "summary"
            | "photos"
            | "sources"
            | "estimate"
            | "import"
            | "scan"
            | "jobs"
            | "job"
            | "revisions"
            | "decision"
            | "auth"
            | "backends"
            | "frontends"
            | "config"
            | "init"
            | "volumes"
            | "analyze"
            | "group"
            | "bursts"
            | "sessions"
            | "groups"
            | "decisions"
            | "preselect"
            | "verify"
            | "evaluate"
            | "report"
            | "nas"
    )
}

fn path_id(value: &str) -> Result<&str, CliError> {
    let valid = !value.is_empty()
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    valid.then_some(value).ok_or(CliError::Usage(
        "IDs may contain only letters, numbers, dot, dash, or underscore",
    ))
}

fn ensure_arg_count(
    args: &[String],
    expected: usize,
    message: &'static str,
) -> Result<(), CliError> {
    if args.len() == expected {
        Ok(())
    } else {
        Err(CliError::Usage(message))
    }
}

fn next_request_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(1);
    format!(
        "cli-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

fn failure(error: CliError) -> CommandOutput {
    let message = format_error(&error);
    CommandOutput {
        stdout: String::new(),
        stderr: message,
        exit_code: error.exit_code(),
    }
}

pub fn format_error(error: &CliError) -> String {
    match error {
        CliError::Usage(text) => (*text).to_owned(),
        CliError::ServerUnavailable(text) => format!("server unavailable: {text}"),
        CliError::CredentialStore(text) => format!("credential error: {text}"),
        CliError::ServerRejected { status, message } => {
            format!("server returned HTTP {status}: {message}")
        }
    }
}

fn json_escape(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| match ch {
            '"' => "\\\"".chars().collect::<Vec<_>>(),
            '\\' => "\\\\".chars().collect(),
            '\n' => "\\n".chars().collect(),
            '\r' => "\\r".chars().collect(),
            '\t' => "\\t".chars().collect(),
            other => vec![other],
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreselectionRow {
    pub jpeg_path: String,
    pub raw_path: String,
    pub group: String,
    pub selected: bool,
    pub representative_jpeg: String,
    pub quality: String,
    pub similarity: String,
}

/// Legacy-compatible task 36 manifest column order and yes/no values.
pub fn preselection_csv(rows: &[PreselectionRow]) -> String {
    let mut csv =
        String::from("jpeg_path,raw_path,group,selected,representative_jpeg,quality,similarity\n");
    for row in rows {
        let values = [
            row.jpeg_path.as_str(),
            row.raw_path.as_str(),
            row.group.as_str(),
            if row.selected { "yes" } else { "no" },
            row.representative_jpeg.as_str(),
            row.quality.as_str(),
            row.similarity.as_str(),
        ];
        csv.push_str(
            &values
                .iter()
                .map(|value| csv_field(value))
                .collect::<Vec<_>>()
                .join(","),
        );
        csv.push('\n');
    }
    csv
}

fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Default)]
    struct FixtureServer {
        seen: Vec<Request>,
        replies: VecDeque<Result<Response, String>>,
    }
    impl RestClient for FixtureServer {
        fn request(&mut self, _: &str, request: Request) -> Result<Response, String> {
            self.seen.push(request);
            self.replies.pop_front().unwrap_or_else(|| {
                Ok(Response {
                    status: 200,
                    body: "{}".into(),
                })
            })
        }
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).into()).collect()
    }

    #[test]
    fn parses_global_catalog_and_server_options() {
        let parsed = Invocation::parse(&args(&[
            "--catalog",
            "/data/library.sqlite",
            "--server",
            "http://nas:8080",
            "doctor",
        ]))
        .unwrap();
        assert_eq!(parsed.command, "doctor");
        assert_eq!(parsed.catalog, "/data/library.sqlite");
        assert_eq!(parsed.server, "http://nas:8080");
    }

    #[test]
    fn scan_uses_the_real_gallery_rescan_route() {
        let inv = Invocation::parse(&args(&["scan", "gallery-1"])).unwrap();
        let mut server = FixtureServer::default();
        assert_eq!(execute(&mut server, &inv).exit_code, 0);
        assert_eq!(server.seen[0].method, Method::Post);
        assert_eq!(server.seen[0].path, "/api/v2/galleries/gallery-1/rescan");
        assert_eq!(server.seen[0].body, None);
    }

    #[test]
    fn photos_cursor_without_decision_filter_builds_a_valid_request() {
        let cursor = r#"{"after_id":"photo-1","filter":{"gallery_id":"gallery-1","decision":null}}"#;
        let (_, path, _) = photos_request(&args(&[
            "gallery-1",
            "--limit",
            "25",
            "--cursor",
            cursor,
        ]))
        .unwrap();
        assert!(path.starts_with("/api/v1/galleries/gallery-1/photos?limit=25&cursor="));
        assert!(!path.contains("decision="));
    }

    #[test]
    fn photos_cursor_decision_must_match_explicit_filter() {
        let cursor = r#"{"after_id":"photo-1","filter":{"gallery_id":"gallery-1","decision":"keep"}}"#;
        assert!(photos_request(&args(&[
            "gallery-1",
            "--decision",
            "reject",
            "--cursor",
            cursor,
        ]))
        .is_err());
    }

    #[test]
    fn single_decision_maps_to_revision_checked_route() {
        let inv = Invocation::parse(&args(&["decision", "photo-1", "keep", "7"])).unwrap();
        let mut server = FixtureServer::default();
        assert_eq!(execute(&mut server, &inv).exit_code, 0);
        assert_eq!(server.seen[0].method, Method::Put);
        assert_eq!(server.seen[0].path, "/api/v1/photos/photo-1/decision");
        assert!(
            server.seen[0]
                .body
                .as_deref()
                .unwrap()
                .contains("\"expected_revision\":7")
        );
    }

    #[test]
    fn dry_run_reports_mutation_without_contacting_server() {
        let inv = Invocation::parse(&args(&["--dry-run", "scan", "gallery-1"])).unwrap();
        assert!(inv.dry_run);
        let mut server = FixtureServer::default();
        let out = execute(&mut server, &inv);
        assert_eq!(out.exit_code, 0);
        assert!(
            out.stdout
                .contains("dry run: POST /api/v2/galleries/gallery-1/rescan")
        );
        assert!(server.seen.is_empty());
    }

    #[test]
    fn remote_errors_have_stable_process_codes() {
        let inv = Invocation::parse(&args(&["status"])).unwrap();
        let mut server = FixtureServer {
            replies: VecDeque::from([Ok(Response {
                status: 409,
                body: "revision conflict".into(),
            })]),
            ..Default::default()
        };
        let out = execute(&mut server, &inv);
        assert_eq!(out.exit_code, 4);
        assert!(out.stderr.contains("HTTP 409"));
        let mut disconnected = FixtureServer {
            replies: VecDeque::from([Err("connection refused".into())]),
            ..Default::default()
        };
        assert_eq!(execute(&mut disconnected, &inv).exit_code, 3);
    }

    #[test]
    fn diagnostics_succeeds_and_reports_offline_server() {
        let inv = Invocation::parse(&args(&["doctor"])).unwrap();
        let mut server = FixtureServer {
            replies: VecDeque::from([Err("connection refused".into())]),
            ..Default::default()
        };
        let out = execute(&mut server, &inv);
        assert_eq!(out.exit_code, 0);
        assert!(out.stdout.contains("server: offline"));
        assert_eq!(server.seen[0].path, "/api/v1/health");
    }

    #[test]
    fn preselection_csv_matches_legacy_header_and_escapes_fields() {
        let csv = preselection_csv(&[PreselectionRow {
            jpeg_path: "/photos/a,1.jpg".into(),
            raw_path: "/raw/a.nef".into(),
            group: "burst-1".into(),
            selected: true,
            representative_jpeg: "a,1.jpg".into(),
            quality: "0.9123".into(),
            similarity: "1.0000".into(),
        }]);
        assert_eq!(
            csv,
            "jpeg_path,raw_path,group,selected,representative_jpeg,quality,similarity\n\"/photos/a,1.jpg\",/raw/a.nef,burst-1,yes,\"a,1.jpg\",0.9123,1.0000\n"
        );
    }

    #[test]
    fn invalid_invocations_return_usage_code_two() {
        assert_eq!(
            Invocation::parse(&args(&["scan"])).unwrap_err().exit_code(),
            2
        );
        assert_eq!(
            Invocation::parse(&args(&["wat"])).unwrap_err().exit_code(),
            2
        );
        assert_eq!(
            Invocation::parse(&args(&["volumes"]))
                .unwrap_err()
                .exit_code(),
            2
        );
    }
}
