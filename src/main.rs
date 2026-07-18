mod error;
mod models;
mod operations;
mod service;
mod storage;
#[cfg(test)]
mod test_helpers;

use mcp_core::ServerConfig;

/// Server-level, model-facing description emitted in the MCP `initialize`
/// response. The daemon indexes it so tool-search can route time-tracking
/// requests to this server; keep it honest about what the tools actually do.
const SERVER_INSTRUCTIONS: &str = "Local time-tracking (timeclock) for work sessions grouped by project, kept in on-disk files with no external account or auth. Reach for it whenever the user wants to start or stop the clock, check whether they are currently clocked in, or produce a timesheet or hours report - e.g. \"clock me in to Acme\", \"how many hours did I spend on the redesign last week\", or \"am I still tracking time?\". Start and stop with timeclock_clock_in / timeclock_clock_out (one active session per project), see what is running with timeclock_session_get_active, and pull reports over a date range with timeclock_session_query (per project, JSON or CSV). Projects are managed with timeclock_project_upsert / timeclock_project_list and records fixed or annotated with timeclock_session_correct / timeclock_session_add_note; clocking in needs a project_id, so create the project first if it does not exist.";

/// Build the [`ServerConfig`] for this server.
///
/// Why a helper: keeps the server-level `instructions` blurb (emitted in the
/// MCP `initialize` response and indexed by the daemon as this server's
/// model-facing description) in one testable place.
fn server_config() -> ServerConfig {
    ServerConfig::new("timeclock-mcp", env!("CARGO_PKG_VERSION")).instructions(SERVER_INSTRUCTIONS)
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
