//! HTTP server package scaffold. D34 will connect the application router.

pub mod demo_args;

/// Explicit result for an invocation that is valid but cannot be served yet.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RouterNotIntegrated;

impl std::fmt::Display for RouterNotIntegrated {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("server router is not integrated; service was not started")
    }
}

impl std::error::Error for RouterNotIntegrated {}
