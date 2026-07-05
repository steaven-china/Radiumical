//! MCP (Model Context Protocol) client — async stdio JSON-RPC.
//!
//! Spawns MCP servers via tokio async child process, discovers tools,
//! wraps them as native `Tool` impls with timeout support.
//!
//! Config: `~/.radi/mcp.json`
//! ```json
//! {
//!   "mcpServers": {
//!     "fs": {
//!       "command": "npx",
//!       "args": ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
//!     }
//!   }
//! }
//! ```

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;

// ── Config ──

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpConfig {
    #[serde(default, alias = "mcpServers")]
    pub servers: HashMap<String, McpServerConfig>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpServerConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
}

// ── JSON-RPC ──

#[derive(Serialize)]
struct Request<'a> {
    jsonrpc: &'static str,
    id: i64,
    method: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    params: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct Response {
    #[allow(dead_code)]
    id: Option<i64>,
    #[serde(default)]
    result: Option<serde_json::Value>,
    #[serde(default)]
    error: Option<RpcError>,
}

#[derive(Deserialize, Debug)]
struct RpcError {
    code: i64,
    #[serde(alias = "msg")]
    message: String,
}

// ── Tool info ──

#[derive(Debug, Clone)]
pub struct McpToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
    pub server_name: String,
}

// ── Client ──

type ChildStdin = tokio::process::ChildStdin;
type ChildStdout = BufReader<tokio::process::ChildStdout>;

pub struct McpClient {
    name: String,
    child: Child,
    stdin: Mutex<ChildStdin>,
    stdout: Mutex<ChildStdout>,
    next_id: AtomicI64,
    timeout: Duration,
}

impl McpClient {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Spawn an MCP server process and perform the handshake.
    pub async fn spawn(name: &str, config: &McpServerConfig, timeout: Duration) -> Result<Self> {
        let mut cmd = Command::new(&config.command);
        cmd.args(&config.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null());
        for (k, v) in &config.env {
            cmd.env(k, v);
        }
        let mut child = cmd.spawn().with_context(|| {
            format!(
                "spawn MCP server '{}' ({} {})",
                name,
                config.command,
                config.args.join(" ")
            )
        })?;
        let stdin = child.stdin.take().context("MCP stdin pipe")?;
        let stdout = child.stdout.take().context("MCP stdout pipe")?;

        let mut client = Self {
            name: name.to_string(),
            child,
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(BufReader::new(stdout)),
            next_id: AtomicI64::new(1),
            timeout,
        };
        client.handshake().await?;
        Ok(client)
    }

    fn next_id(&self) -> i64 {
        self.next_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Send a JSON-RPC request and read the response with timeout.
    async fn call(
        &self,
        method: &str,
        params: Option<serde_json::Value>,
    ) -> Result<serde_json::Value> {
        let id = self.next_id();
        let req = Request {
            jsonrpc: "2.0",
            id,
            method,
            params,
        };
        let json = serde_json::to_string(&req)?;

        // Write request
        {
            let mut stdin = self.stdin.lock().await;
            stdin.write_all(json.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await?;
        }

        // Read response with timeout (skip non-JSON lines)
        let mut line = String::new();
        loop {
            line.clear();
            let n = {
                let mut stdout = self.stdout.lock().await;
                match tokio::time::timeout(self.timeout, stdout.read_line(&mut line)).await {
                    Ok(result) => result?,
                    Err(_) => anyhow::bail!(
                        "MCP server '{}' timed out after {:.1}s",
                        self.name,
                        self.timeout.as_secs_f64()
                    ),
                }
            };
            if n == 0 {
                anyhow::bail!("MCP server '{}' exited unexpectedly", self.name);
            }

            let trimmed = line.trim();
            if trimmed.is_empty() || !trimmed.starts_with('{') {
                continue;
            }
            let resp: Response = serde_json::from_str(trimmed)
                .with_context(|| format!("parse MCP response: {trimmed}"))?;
            if let Some(err) = resp.error {
                anyhow::bail!("MCP error {}: {}", err.code, err.message);
            }
            return Ok(resp.result.unwrap_or(serde_json::Value::Null));
        }
    }

    async fn handshake(&mut self) -> Result<()> {
        self.call(
            "initialize",
            Some(serde_json::json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "radiumical", "version": "0.1.0" }
            })),
        )
        .await?;
        // Send initialized notification (no response expected)
        let note = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized"
        });
        let json = serde_json::to_string(&note)?;
        let mut stdin = self.stdin.lock().await;
        stdin.write_all(json.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        Ok(())
    }

    pub async fn list_tools(&self) -> Result<Vec<McpToolInfo>> {
        let result = self.call("tools/list", None).await?;
        let tools_raw = result.get("tools").and_then(|v| v.as_array());
        let Some(arr) = tools_raw else {
            return Ok(Vec::new());
        };
        let mut tools = Vec::new();
        for v in arr {
            let name = v
                .get("name")
                .and_then(|n| n.as_str())
                .unwrap_or("")
                .to_string();
            let description = v
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();
            let input_schema = v
                .get("inputSchema")
                .cloned()
                .unwrap_or(serde_json::json!({}));
            tools.push(McpToolInfo {
                name,
                description,
                input_schema,
                server_name: self.name.clone(),
            });
        }
        Ok(tools)
    }

    pub async fn call_tool(&self, name: &str, arguments: serde_json::Value) -> Result<String> {
        let result = self
            .call(
                "tools/call",
                Some(serde_json::json!({
                    "name": name,
                    "arguments": arguments
                })),
            )
            .await?;
        // MCP tool result: { content: [{ type: "text", text: "..." }] }
        if let Some(content) = result.get("content").and_then(|c| c.as_array()) {
            let texts: Vec<&str> = content
                .iter()
                .filter_map(|c| c.get("text").and_then(|t| t.as_str()))
                .collect();
            Ok(texts.join("\n"))
        } else {
            Ok(serde_json::to_string_pretty(&result)?)
        }
    }

    pub async fn is_alive(&mut self) -> bool {
        self.child.try_wait().ok().flatten().is_none()
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        if let Err(e) = self.child.start_kill() {
            tracing::debug!(error = %e, "failed to kill MCP child process");
        }
    }
}

// ── Config helpers ──

pub fn config_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".radi")
        .join("mcp.json")
}

pub fn load_config() -> McpConfig {
    let path = config_path();
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_config() {
        let json = r#"{
            "mcpServers": {
                "fs": { "command": "npx", "args": ["-y", "@mcp/fs"] }
            }
        }"#;
        let cfg: McpConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.servers.len(), 1);
        assert_eq!(cfg.servers["fs"].command, "npx");
    }

    #[test]
    fn test_parse_config_alias() {
        let json = r#"{ "servers": { "x": { "command": "echo" } } }"#;
        let cfg: McpConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.servers.len(), 1);
    }

    #[test]
    fn test_parse_empty() {
        let cfg: McpConfig = serde_json::from_str(r#"{}"#).unwrap();
        assert!(cfg.servers.is_empty());
    }

    #[test]
    fn test_parse_response() {
        let json = r#"{"id":1,"result":{"tools":[{"name":"read","description":"read file","inputSchema":{}}]}}"#;
        let resp: Response = serde_json::from_str(json).unwrap();
        assert!(resp.error.is_none());
        let tools = resp.result.unwrap();
        assert_eq!(tools["tools"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_parse_config_with_env() {
        let json = r#"{
            "mcpServers": {
                "db": {
                    "command": "node",
                    "args": ["server.js"],
                    "env": { "DATABASE_URL": "postgres://localhost/test", "API_KEY": "secret123" }
                }
            }
        }"#;
        let cfg: McpConfig = serde_json::from_str(json).unwrap();
        let server = &cfg.servers["db"];
        assert_eq!(server.command, "node");
        assert_eq!(server.env.len(), 2);
        assert_eq!(server.env["DATABASE_URL"], "postgres://localhost/test");
        assert_eq!(server.env["API_KEY"], "secret123");
    }

    #[test]
    fn test_mcp_server_config_serde_roundtrip() {
        let mut env = HashMap::new();
        env.insert("FOO".to_string(), "bar".to_string());
        let cfg = McpServerConfig {
            command: "npx".to_string(),
            args: vec!["-y".to_string(), "some-tool".to_string()],
            env,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let deserialized: McpServerConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(cfg, deserialized);
    }

    #[test]
    fn test_rpc_error_deserialize() {
        let json = r#"{"jsonrpc":"2.0","id":1,"error":{"code":-1,"msg":"bad"}}"#;
        let resp: Response = serde_json::from_str(json).unwrap();
        assert!(resp.error.is_some());
        let err = resp.error.unwrap();
        assert_eq!(err.code, -1);
        assert_eq!(err.message, "bad");
    }
}
