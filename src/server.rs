#![deny(warnings)]

use serde_json::{Value, json};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::{McpError, Result};
use crate::tools::ToolRegistry;

pub struct McpServer {
    tool_registry: Arc<ToolRegistry>,
    initialized: Arc<RwLock<bool>>,
}

impl McpServer {
    pub fn new() -> Self {
        Self {
            tool_registry: Arc::new(ToolRegistry::new()),
            initialized: Arc::new(RwLock::new(false)),
        }
    }

    pub async fn handle_initialize(
        &self,
        protocol_version: &str,
        _client_capabilities: &Value,
    ) -> Result<Value> {
        let supported = ["2024-11-05", "2025-06-18", "2025-11-25"];
        if !supported.contains(&protocol_version) {
            return Err(McpError::InvalidProtocolVersion(protocol_version.to_string()).into());
        }
        let tools = self.tool_registry.list_tools();
        Ok(json!({
            "protocolVersion": protocol_version,
            "serverInfo": {
                "name": "timeclock-mcp",
                "version": env!("CARGO_PKG_VERSION"),
            },
            "capabilities": {
                "tools": { "listChanged": false },
            },
            "tools": tools,
        }))
    }

    pub async fn handle_initialized(&self) -> Result<()> {
        *self.initialized.write().await = true;
        Ok(())
    }

    pub async fn handle_tool_call(&self, tool_name: &str, arguments: &Value) -> Result<Value> {
        self.tool_registry.execute_tool(tool_name, arguments).await
    }

    pub async fn handle_shutdown(&self) -> Result<()> {
        *self.initialized.write().await = false;
        Ok(())
    }

    pub fn list_tools(&self) -> Value {
        self.tool_registry.list_tools()
    }

    pub async fn is_initialized(&self) -> bool {
        *self.initialized.read().await
    }
}

impl Default for McpServer {
    fn default() -> Self {
        Self::new()
    }
}
