mod error;
mod models;
mod operations;
mod service;
mod storage;
#[cfg(test)]
mod test_helpers;

use mcp_core::ServerConfig;

#[tokio::main]
async fn main() -> mcp_core::Result<()> {
    let config = ServerConfig::new("timeclock-mcp", env!("CARGO_PKG_VERSION"));
    mcp_core::run_simple(config, || async { Ok(service::TimeclockService) }).await
}
