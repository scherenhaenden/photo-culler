//! Operational CLI command contract and REST adapter boundary.
//!
//! This module is deliberately dependency-free. The current binary entrypoint
//! does not yet register a commands module and the Rust server package is not
//! present, so tests use a fake REST client while exercising the command map.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub command: String,
    pub args: Vec<String>,
    pub catalog: String,
    pub server: String,
    pub json_output: bool,
    pub csv_output: bool,
    pub quiet: bool,
    pub verbose: bool,
    pub dry_run: bool,
    pub no_color: bool,
}

impl Invocation {
    pub fn parse(argv: &[String]) -> Result<Self, CliError> {
        let mut command = None;
        let mut args = Vec::new();
        let mut catalog = "catalog.db".to_owned();
        let mut server = "http://127.0.0.1:8080".to_owned();
        let (mut json_output, mut csv_output, mut quiet, mut verbose, mut dry_run, mut no_color) =
            (false, false, false, false, false, false);
        let mut i = 0;
        while i < argv.len() {
            match argv[i].as_str() {
                "--catalog" | "-c" => {
                    i += 1;
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
                "--json" => json_output = true,
                "--csv" => csv_output = true,
                "--quiet" | "-q" => quiet = true,
                "--verbose" | "-v" => verbose = true,
                "--dry-run" => dry_run = true,
                "--no-color" => no_color = true,
                flag if flag.starts_with('-') && command.is_some() => {
                    args.push(flag.to_owned());
                }
                flag if flag.starts_with('-') => {
                    return Err(CliError::Usage("unknown global option"));
                }
                value if command.is_none() => command = Some(value.to_owned()),
                value => args.push(value.to_owned()),
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
            json_output,
            csv_output,
            quiet,
            verbose,
            dry_run,
            no_color,
        };
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
    ServerRejected { status: u16, message: String },
}

impl CliError {
    /// Stable process codes: syntax=2, unavailable server=3, remote failure=4.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            Self::ServerUnavailable(_) => 3,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: u8,
}

/// Execute a parsed command. `doctor`/`diagnostics` remain useful offline and
/// turn transport failure into a successful diagnostic report.
pub fn execute(client: &mut impl RestClient, invocation: &Invocation) -> CommandOutput {
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
        "status" => (Method::Get, "/api/v1/health".into(), None),
        "backends" | "frontends" | "config"
            if invocation.args.is_empty()
                || invocation.args.first().is_some_and(|a| a == "show") =>
        {
            (Method::Get, "/api/v1/config".into(), None)
        }
        "config" if invocation.args.first().is_some_and(|a| a == "get") => (
            Method::Get,
            format!("/api/v1/config/{}", arg(1, "config get KEY needs a key")?),
            None,
        ),
        "config" if invocation.args.first().is_some_and(|a| a == "set") => {
            let key = arg(1, "config set KEY VALUE needs a key")?;
            let value = arg(2, "config set KEY VALUE needs a value")?;
            (
                Method::Put,
                format!("/api/v1/config/{key}"),
                Some(format!("{{\"value\":\"{}\"}}", json_escape(value))),
            )
        }
        "volumes" => (Method::Get, "/api/v1/volumes".into(), None),
        "scan" => (
            Method::Post,
            "/api/v1/imports".into(),
            Some(json_arg("root", arg(0, "scan PATH needs a path")?)),
        ),
        "analyze" => (
            Method::Post,
            "/api/v1/analysis-jobs".into(),
            Some(json_arg("target", arg(0, "analyze TARGET needs a target")?)),
        ),
        "group" | "bursts" => (
            Method::Post,
            "/api/v1/groups/rebuild".into(),
            Some("{}".into()),
        ),
        "sessions" => (Method::Get, "/api/v1/sessions".into(), None),
        "photos" => (Method::Get, "/api/v1/photos".into(), None),
        "decisions" => {
            if arg(0, "decisions set --decision STATE [PHOTO_ID]")? != "set" {
                return Err(CliError::Usage("decisions only supports set"));
            }
            let decision_at = invocation
                .args
                .iter()
                .position(|item| item == "--decision")
                .ok_or(CliError::Usage("decisions set requires --decision STATE"))?;
            let decision = invocation
                .args
                .get(decision_at + 1)
                .ok_or(CliError::Usage("--decision needs a state"))?;
            let confirmed = invocation.args.iter().any(|item| item == "--confirm");
            (
                Method::Put,
                "/api/v1/decisions".into(),
                Some(format!(
                    "{{\"decision\":\"{}\",\"confirmed\":{confirmed}}}",
                    json_escape(decision)
                )),
            )
        }
        "preselect" => {
            let root = json_escape(arg(0, "preselect PATH needs a directory")?);
            let output =
                optional_option(&invocation.args, "--output").unwrap_or("preselection.csv");
            let raw_dir = optional_option(&invocation.args, "--raw-dir").unwrap_or("");
            let similarity =
                optional_option(&invocation.args, "--duplicate-similarity").unwrap_or("0.88");
            (
                Method::Post,
                "/api/v1/preselection".into(),
                Some(format!(
                    "{{\"root\":\"{root}\",\"output\":\"{}\",\"raw_dir\":\"{}\",\"duplicate_similarity\":\"{}\"}}",
                    json_escape(output),
                    json_escape(raw_dir),
                    json_escape(similarity)
                )),
            )
        }
        "verify" => {
            let path = json_escape(arg(0, "verify PATH needs a path")?);
            let against = optional_option(&invocation.args, "--against").unwrap_or("");
            (
                Method::Post,
                "/api/v1/verification".into(),
                Some(format!(
                    "{{\"path\":\"{path}\",\"against\":\"{}\"}}",
                    json_escape(against)
                )),
            )
        }
        "evaluate" => (
            Method::Post,
            "/api/v1/evaluations".into(),
            Some(json_arg(
                "target",
                arg(0, "evaluate TARGET needs a target")?,
            )),
        ),
        "report" => (Method::Get, "/api/v1/reports".into(), None),
        "init" => (
            Method::Post,
            "/api/v1/catalog/initialize".into(),
            Some(format!(
                "{{\"path\":\"{}\"}}",
                json_escape(&invocation.catalog)
            )),
        ),
        "nas" => (Method::Get, "/api/v1/nas/status".into(), None),
        _ => return Err(CliError::Usage("unsupported command arguments")),
    };
    Ok(Request { method, path, body })
}

fn is_supported(command: &str) -> bool {
    matches!(
        command,
        "status"
            | "doctor"
            | "diagnostics"
            | "backends"
            | "frontends"
            | "config"
            | "init"
            | "volumes"
            | "scan"
            | "analyze"
            | "group"
            | "bursts"
            | "sessions"
            | "photos"
            | "decisions"
            | "preselect"
            | "verify"
            | "evaluate"
            | "report"
            | "nas"
    )
}

fn failure(error: CliError) -> CommandOutput {
    let message = match &error {
        CliError::Usage(text) => (*text).to_owned(),
        CliError::ServerUnavailable(text) => format!("server unavailable: {text}"),
        CliError::ServerRejected { status, message } => {
            format!("server returned HTTP {status}: {message}")
        }
    };
    CommandOutput {
        stdout: String::new(),
        stderr: message,
        exit_code: error.exit_code(),
    }
}

fn json_arg(name: &str, value: &str) -> String {
    format!("{{\"{name}\":\"{}\"}}", json_escape(value))
}

fn optional_option<'a>(args: &'a [String], option: &str) -> Option<&'a str> {
    args.iter()
        .position(|argument| argument == option)
        .and_then(|index| args.get(index + 1))
        .map(String::as_str)
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
    fn parses_legacy_global_catalog_and_server_options() {
        let parsed = Invocation::parse(&args(&[
            "--catalog",
            "/data/library.sqlite",
            "--server",
            "http://nas:8080",
            "volumes",
        ]))
        .unwrap();
        assert_eq!(parsed.command, "volumes");
        assert_eq!(parsed.catalog, "/data/library.sqlite");
        assert_eq!(parsed.server, "http://nas:8080");
    }

    #[test]
    fn operational_commands_use_the_rest_contract() {
        let inv = Invocation::parse(&args(&["scan", "/photos"])).unwrap();
        let mut server = FixtureServer::default();
        assert_eq!(execute(&mut server, &inv).exit_code, 0);
        assert_eq!(server.seen[0].method, Method::Post);
        assert_eq!(server.seen[0].path, "/api/v1/imports");
        assert_eq!(
            server.seen[0].body.as_deref(),
            Some("{\"root\":\"/photos\"}")
        );
    }

    #[test]
    fn decision_options_survive_parsing_and_map_to_bulk_decision_contract() {
        let inv = Invocation::parse(&args(&[
            "decisions",
            "set",
            "--decision",
            "keep",
            "--confirm",
        ]))
        .unwrap();
        let mut server = FixtureServer::default();
        assert_eq!(execute(&mut server, &inv).exit_code, 0);
        assert_eq!(server.seen[0].method, Method::Put);
        assert_eq!(server.seen[0].path, "/api/v1/decisions");
        assert_eq!(
            server.seen[0].body.as_deref(),
            Some("{\"decision\":\"keep\",\"confirmed\":true}")
        );
    }

    #[test]
    fn preselection_arguments_are_forwarded_to_the_rest_contract() {
        let inv = Invocation::parse(&args(&[
            "preselect",
            "/photos",
            "--output",
            "/reports/picks.csv",
            "--raw-dir",
            "/raw",
            "--duplicate-similarity",
            "0.91",
        ]))
        .unwrap();
        let mut server = FixtureServer::default();
        assert_eq!(execute(&mut server, &inv).exit_code, 0);
        assert_eq!(
            server.seen[0].body.as_deref(),
            Some(
                "{\"root\":\"/photos\",\"output\":\"/reports/picks.csv\",\"raw_dir\":\"/raw\",\"duplicate_similarity\":\"0.91\"}"
            )
        );
    }

    #[test]
    fn dry_run_reports_mutation_without_contacting_server() {
        let inv = Invocation::parse(&args(&["--dry-run", "scan", "/photos"])).unwrap();
        assert!(inv.dry_run);
        let mut server = FixtureServer::default();
        let out = execute(&mut server, &inv);
        assert_eq!(out.exit_code, 0);
        assert!(out.stdout.contains("dry run: POST /api/v1/imports"));
        assert!(server.seen.is_empty());
    }

    #[test]
    fn remote_errors_have_stable_process_codes() {
        let inv = Invocation::parse(&args(&["sessions"])).unwrap();
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
    }
}
