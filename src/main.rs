#![deny(warnings)]

mod error;
mod models;
mod operations;
mod server;
mod storage;
#[cfg(test)]
mod test_helpers;
mod tools;
mod transport;

use axum::{
    extract::{ws::WebSocketUpgrade, State},
    response::Response,
    routing::get,
    Router,
};
use clap::{Parser, ValueEnum};
use error::Result;
use server::McpServer;
use serde_json::Value;
use std::fmt;
use std::sync::Arc;
use tokio::net::TcpListener;
use transport::StdioTransportHandler;

// ---------------------------------------------------------------------------
// CLI
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, ValueEnum)]
enum TransportMode {
    /// STDIN/STDOUT (recommended for local / VS Code usage)
    Stdio,
    /// WebSocket
    Websocket,
}

impl fmt::Display for TransportMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransportMode::Stdio => write!(f, "stdio"),
            TransportMode::Websocket => write!(f, "websocket"),
        }
    }
}

#[derive(Parser)]
#[command(name = "timeclock-mcp")]
#[command(about = "Local-first time-tracking MCP server")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(clap::Subcommand)]
enum Commands {
    /// Run the MCP server
    Serve {
        #[arg(short, long, default_value_t = TransportMode::Stdio)]
        mode: TransportMode,
        #[arg(short, long, default_value_t = 8080)]
        port: u16,
        #[arg(long, default_value = "0.0.0.0")]
        host: String,
    },
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Serve { mode, port, host } => {
            let server = McpServer::new();
            match mode {
                TransportMode::Stdio => run_stdio_server(server).await?,
                TransportMode::Websocket => run_websocket_server(server, &host, port).await?,
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Stdio server
// ---------------------------------------------------------------------------

async fn run_stdio_server(server: McpServer) -> Result<()> {
    let server = Arc::new(server);
    let mut transport = StdioTransportHandler::new();

    loop {
        let message_str = match transport.read_message().await {
            Ok(msg) => msg,
            Err(e) => {
                eprintln!("Error reading message: {e}");
                break;
            }
        };
        if message_str.is_empty() {
            continue;
        }
        let message: Value = match serde_json::from_str(&message_str) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("Error parsing JSON-RPC: {e}");
                let err = jsonrpc_error(None, -32700, "Parse error", None);
                if let Ok(s) = serde_json::to_string(&err) {
                    let _ = transport.write_message(&s).await;
                }
                continue;
            }
        };
        let response = handle_message(Arc::clone(&server), message).await;
        if let Some(resp) = response {
            match serde_json::to_string(&resp) {
                Ok(s) => {
                    if let Err(e) = transport.write_message(&s).await {
                        eprintln!("Error writing response: {e}");
                        break;
                    }
                }
                Err(e) => eprintln!("Error serializing response: {e}"),
            }
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// WebSocket server
// ---------------------------------------------------------------------------

async fn run_websocket_server(server: McpServer, host: &str, port: u16) -> Result<()> {
    let server = Arc::new(server);
    let app = Router::new()
        .route("/ws", get(ws_handler))
        .with_state(server);
    let addr = format!("{host}:{port}");
    let listener = TcpListener::bind(&addr).await?;
    eprintln!("WebSocket server listening on {addr}");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(server): State<Arc<McpServer>>,
) -> Response {
    ws.on_upgrade(move |socket| handle_ws_connection(socket, server))
}

async fn handle_ws_connection(socket: axum::extract::ws::WebSocket, server: Arc<McpServer>) {
    use axum::extract::ws::Message;
    use futures_util::{SinkExt, StreamExt};

    let (mut sender, mut receiver) = socket.split();
    while let Some(msg) = receiver.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                let message: Value = match serde_json::from_str(&text) {
                    Ok(m) => m,
                    Err(e) => {
                        eprintln!("Error parsing JSON-RPC: {e}");
                        let err = jsonrpc_error(None, -32700, "Parse error", None);
                        if let Ok(s) = serde_json::to_string(&err) {
                            let _ = sender.send(Message::Text(s.into())).await;
                        }
                        continue;
                    }
                };
                let response = handle_message(Arc::clone(&server), message).await;
                if let Some(resp) = response
                    && let Ok(s) = serde_json::to_string(&resp)
                    && let Err(e) = sender.send(Message::Text(s.into())).await
                {
                    eprintln!("Error sending WS response: {e}");
                    break;
                }
            }
            Ok(Message::Close(_)) => break,
            Err(e) => {
                eprintln!("WebSocket error: {e}");
                break;
            }
            _ => {}
        }
    }
}

// ---------------------------------------------------------------------------
// JSON-RPC dispatch
// ---------------------------------------------------------------------------

async fn handle_message(server: Arc<McpServer>, message: Value) -> Option<Value> {
    if let Some(v) = message.get("jsonrpc").and_then(|v| v.as_str())
        && v != "2.0"
    {
        let id = message.get("id").cloned();
        return Some(jsonrpc_error(
            id,
            -32600,
            &format!("Invalid JSON-RPC version: {v}"),
            None,
        ));
    }

    let id = message.get("id").cloned();
    let method = message.get("method").and_then(|m| m.as_str());
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let is_notification = id.is_none();

    let result: std::result::Result<Value, String> = match method {
        Some("initialize") => {
            let protocol_version = params
                .get("protocolVersion")
                .and_then(|v| v.as_str())
                .unwrap_or("2024-11-05");
            let caps = params.get("capabilities").unwrap_or(&Value::Null);
            server
                .handle_initialize(protocol_version, caps)
                .await
                .map_err(|e| e.to_string())
        }
        Some("initialized") | Some("notifications/initialized") => {
            if is_notification {
                let _ = server.handle_initialized().await;
                return None;
            }
            server
                .handle_initialized()
                .await
                .map(|_| Value::Null)
                .map_err(|e| e.to_string())
        }
        Some("tools/list") => {
            if !server.is_initialized().await {
                return Some(jsonrpc_error(
                    id,
                    -32000,
                    "Server not initialized. Call 'initialize' first.",
                    None,
                ));
            }
            Ok(serde_json::json!({ "tools": server.list_tools() }))
        }
        Some("tools/call") => {
            if !server.is_initialized().await {
                return Some(jsonrpc_error(
                    id,
                    -32000,
                    "Server not initialized. Call 'initialize' first.",
                    None,
                ));
            }
            let tool_name = params.get("name").and_then(|n| n.as_str());
            let arguments = params.get("arguments").unwrap_or(&Value::Null);
            match tool_name {
                Some(name) => server
                    .handle_tool_call(name, arguments)
                    .await
                    .map(|result| {
                        serde_json::json!({
                            "content": [{ "type": "text", "text": result.to_string() }]
                        })
                    })
                    .map_err(|e| e.to_string()),
                None => Err("tools/call requires 'name' in params".to_string()),
            }
        }
        Some("shutdown") => {
            server.handle_shutdown().await.map(|_| Value::Null).map_err(|e| e.to_string())
        }
        Some("ping") => Ok(serde_json::json!({})),
        None if is_notification => return None,
        _ => {
            if is_notification {
                return None;
            }
            Err(format!("Method not found: {}", method.unwrap_or("<none>")))
        }
    };

    if is_notification {
        return None;
    }

    Some(match result {
        Ok(value) => jsonrpc_success(id, value),
        Err(msg) => jsonrpc_error(id, -32000, &msg, None),
    })
}

// ---------------------------------------------------------------------------
// JSON-RPC helpers
// ---------------------------------------------------------------------------

fn jsonrpc_success(id: Option<Value>, result: Value) -> Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

fn jsonrpc_error(
    id: Option<Value>,
    code: i64,
    message: &str,
    data: Option<Value>,
) -> Value {
    let mut error = serde_json::json!({
        "code": code,
        "message": message,
    });
    if let Some(d) = data {
        error["data"] = d;
    }
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": error,
    })
}
