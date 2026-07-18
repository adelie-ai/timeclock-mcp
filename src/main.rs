mod error;
mod models;
mod operations;
mod service;
mod storage;
#[cfg(test)]
mod test_helpers;

use mcp_core::ServerConfig;

/// Build the [`ServerConfig`] for this server.
///
/// Why a helper: keeps the server-level `instructions` blurb (emitted in the
/// MCP `initialize` response and indexed by the daemon as this server's
/// model-facing description) in one testable place.
fn server_config() -> ServerConfig {
    ServerConfig::new("timeclock-mcp", env!("CARGO_PKG_VERSION"))
}

#[tokio::main]
async fn main() -> mcp_core::Result<()> {
    mcp_core::run_simple(server_config(), || async { Ok(service::TimeclockService) }).await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The server must advertise a non-empty `instructions` blurb so the daemon
    /// can index it as the server-level, model-facing description.
    #[test]
    fn server_config_exposes_nonempty_instructions() {
        let instructions = server_config()
            .instructions
            .expect("server_config must set instructions");
        assert!(
            !instructions.trim().is_empty(),
            "instructions blurb must be non-empty"
        );
    }
}
