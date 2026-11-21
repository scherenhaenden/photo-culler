use std::{net::IpAddr, path::PathBuf, str::FromStr};

pub const DEFAULT_BIND: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 8765;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DemoArgs {
    pub bind: IpAddr,
    pub port: u16,
    pub data_dir: Option<PathBuf>,
    pub web_root: Option<PathBuf>,
    /// Optional opaque catalog gallery ID used as the import destination.
    pub gallery_id: Option<String>,
    /// Server-local roots explicitly authorized for imports; canonicalize in runtime.
    pub authorized_roots: Vec<PathBuf>,
    pub owned: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ParseOutcome {
    Help,
    Run(DemoArgs),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ArgsError {
    UnknownOption(String),
    MissingValue(String),
    InvalidValue { option: String, value: String },
    NonLoopbackBind(IpAddr),
}

impl std::fmt::Display for ArgsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownOption(option) => write!(f, "unknown option: {option}"),
            Self::MissingValue(option) => write!(f, "missing value for {option}"),
            Self::InvalidValue { option, value } => {
                write!(f, "invalid value for {option}: {value}")
            }
            Self::NonLoopbackBind(address) => {
                write!(f, "bind address must be loopback for the demo: {address}")
            }
        }
    }
}

impl std::error::Error for ArgsError {}

pub const HELP: &str = "Photo Culler local catalog server\n\
Usage: photo-culler-server [OPTIONS]\n\
\n\
Options:\n\
  --bind <IP>          Loopback address (default: 127.0.0.1)\n\
  --port <PORT>        TCP port (default: 8765)\n\
  --data-dir <PATH>    Writable catalog and cache directory\n\
  --gallery-id <ID>    Select the catalog gallery and import destination\n\
  --authorized-root <PATH>  Allow imports from this local directory (repeatable)\n\
  --web-root <PATH>    Validated generated Slint web bundle directory\n\
  --owned              Emit one JSON ready line; stdin EOF requests shutdown\n\
  -h, --help           Print this help\n";

pub fn parse<I, S>(arguments: I) -> Result<ParseOutcome, ArgsError>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut args = arguments.into_iter().map(Into::into);
    let mut bind = IpAddr::from_str(DEFAULT_BIND).expect("default IP is valid");
    let mut port = DEFAULT_PORT;
    let mut data_dir = None;
    let mut web_root = None;
    let mut gallery_id = None;
    let mut authorized_roots = Vec::new();
    let mut owned = false;

    while let Some(option) = args.next() {
        if option == "-h" || option == "--help" {
            return Ok(ParseOutcome::Help);
        }
        let value = || ArgsError::MissingValue(option.clone());
        match option.as_str() {
            "--bind" => {
                let text = args.next().ok_or_else(value)?;
                bind = text.parse().map_err(|_| ArgsError::InvalidValue {
                    option: option.clone(),
                    value: text,
                })?;
                if !bind.is_loopback() {
                    return Err(ArgsError::NonLoopbackBind(bind));
                }
            }
            "--port" => {
                let text = args.next().ok_or_else(value)?;
                port = text.parse().map_err(|_| ArgsError::InvalidValue {
                    option: option.clone(),
                    value: text,
                })?;
            }
            "--data-dir" => data_dir = Some(PathBuf::from(args.next().ok_or_else(value)?)),
            "--gallery-id" => gallery_id = Some(args.next().ok_or_else(value)?),
            "--authorized-root" => {
                authorized_roots.push(PathBuf::from(args.next().ok_or_else(value)?))
            }
            "--web-root" | "--static-dir" => {
                web_root = Some(PathBuf::from(args.next().ok_or_else(value)?))
            }
            "--owned" => owned = true,
            _ => return Err(ArgsError::UnknownOption(option)),
        }
    }

    Ok(ParseOutcome::Run(DemoArgs {
        bind,
        port,
        data_dir,
        web_root,
        gallery_id,
        authorized_roots,
        owned,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_lists_supported_flags() {
        assert_eq!(parse(["--help"]).unwrap(), ParseOutcome::Help);
        for flag in ["--bind", "--port", "--data-dir", "--web-root", "--owned"] {
            assert!(HELP.contains(flag));
        }
    }

    #[test]
    fn defaults_bind_to_localhost_and_parse_values() {
        let ParseOutcome::Run(defaults) = parse(Vec::<String>::new()).unwrap() else {
            panic!()
        };
        assert_eq!(defaults.bind, IpAddr::from_str(DEFAULT_BIND).unwrap());
        assert_eq!(defaults.port, DEFAULT_PORT);
        let ParseOutcome::Run(args) = parse([
            "--bind",
            "::1",
            "--port",
            "9000",
            "--data-dir",
            "/tmp/catalog",
            "--static-dir",
            "./web",
        ])
        .unwrap() else {
            panic!()
        };
        assert_eq!(args.bind, IpAddr::from_str("::1").unwrap());
        assert_eq!(args.port, 9000);
        assert!(!args.owned);
        assert_eq!(
            args.data_dir.as_deref(),
            Some(std::path::Path::new("/tmp/catalog"))
        );
        assert_eq!(
            args.web_root.as_deref(),
            Some(std::path::Path::new("./web"))
        );
    }

    #[test]
    fn rejects_invalid_missing_and_nonlocal_arguments() {
        assert!(matches!(
            parse(["--unknown"]),
            Err(ArgsError::UnknownOption(_))
        ));
        assert!(matches!(parse(["--port"]), Err(ArgsError::MissingValue(_))));
        assert!(matches!(
            parse(["--port", "70000"]),
            Err(ArgsError::InvalidValue { .. })
        ));
        assert!(matches!(
            parse(["--bind", "0.0.0.0"]),
            Err(ArgsError::NonLoopbackBind(_))
        ));
    }

    #[test]
    fn owned_mode_and_web_root_alias_are_parsed() {
        let ParseOutcome::Run(args) =
            parse(["--owned", "--port", "0", "--web-root", "./web"]).unwrap()
        else {
            panic!()
        };
        assert!(args.owned);
        assert_eq!(args.port, 0);
        assert_eq!(
            args.web_root.as_deref(),
            Some(std::path::Path::new("./web"))
        );
        let ParseOutcome::Run(alias) = parse(["--static-dir", "./old-name"]).unwrap() else {
            panic!()
        };
        assert_eq!(
            alias.web_root.as_deref(),
            Some(std::path::Path::new("./old-name"))
        );
    }
}
