use std::{net::IpAddr, path::PathBuf, str::FromStr};

pub const DEFAULT_BIND: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 8765;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DemoArgs {
    pub bind: IpAddr,
    pub port: u16,
    pub data_dir: Option<PathBuf>,
    pub static_dir: Option<PathBuf>,
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

pub const HELP: &str = "Photo Culler read-only fixture server\n\
Usage: photo-culler-server [OPTIONS]\n\
\n\
Options:\n\
  --bind <IP>          Loopback address (default: 127.0.0.1)\n\
  --port <PORT>        TCP port (default: 8765)\n\
  --data-dir <PATH>    Reserved for persistent catalogs; rejected by this fixture server\n\
  --static-dir <PATH>  Generated Slint web assets directory\n\
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
    let mut static_dir = None;

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
            "--static-dir" => static_dir = Some(PathBuf::from(args.next().ok_or_else(value)?)),
            _ => return Err(ArgsError::UnknownOption(option)),
        }
    }

    Ok(ParseOutcome::Run(DemoArgs {
        bind,
        port,
        data_dir,
        static_dir,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn help_lists_supported_flags() {
        assert_eq!(parse(["--help"]).unwrap(), ParseOutcome::Help);
        for flag in ["--bind", "--port", "--data-dir", "--static-dir"] {
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
        assert_eq!(
            args.data_dir.as_deref(),
            Some(std::path::Path::new("/tmp/catalog"))
        );
        assert_eq!(
            args.static_dir.as_deref(),
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
}
