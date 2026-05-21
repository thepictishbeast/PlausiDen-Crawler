//! `crawler-mcp` — Model Context Protocol server for
//! PlausiDen-Crawler.
//!
//! Exposes the `crawler` subcommand surface as JSON-RPC tools so
//! MCP-aware clients (Claude Code, Codex, Cursor, …) can run
//! reference-captures, journeys, and detector scans without
//! re-parsing CLI text on every invocation.
//!
//! Per paul 2026-05-21: "i was not using the crawler, would it be
//! easier if there was an MCP for crawler?" — yes. Survives
//! cargo clean (the MCP binary stays installed independent of the
//! workspace target/ dir), standardises the screenshot + detect +
//! diff loop, exposes typed detector findings as structured JSON
//! instead of inferring from raw pixels.
//!
//! ## Tool surface (v0.1.0)
//!
//! - `crawler.capture_reference { url, site_slug?, out_dir? }` —
//!   shells out to `crawler --capture-reference <url>`. Returns
//!   the per-viewport capture manifest as JSON.
//!
//! ## Planned
//!
//! - `crawler.journey { journey_path, out_dir? }` — run a typed
//!   journey + return the findings JSON.
//! - `crawler.detect { url, detectors[] }` — invoke a subset of
//!   detector axes against a URL.
//! - `crawler.diff { reference_dir, current_dir }` — compare two
//!   reference captures + surface pixel deltas.
//!
//! Same stdio JSON-RPC 2.0 shape as `forge-mcp`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::Write;
use tokio::io::{AsyncBufReadExt, BufReader};

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: String,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

const SERVER_INFO: &str = r#"{
    "name": "crawler-mcp",
    "version": "0.1.0",
    "description": "PlausiDen-Crawler operations as MCP tools."
}"#;

fn tool_list() -> Value {
    json!({
        "tools": [
            {
                "name": "crawler.capture_reference",
                "description": "Reference-capture mode: screenshot the given URL at the 390 / 768 / 1280 px viewports, save HTML + screenshot + styles.json per viewport, emit a CaptureManifest JSON. Backed by `crawler --capture-reference <url>`. Use this when comparing a tenant's Forge output against a live reference site.",
                "inputSchema": {
                    "type": "object",
                    "required": ["url"],
                    "properties": {
                        "url": {
                            "type": "string",
                            "description": "URL to capture."
                        },
                        "site_slug": {
                            "type": "string",
                            "description": "Site slug for the capture output dir. Default: derived from the URL host in kebab-case."
                        },
                        "out_dir": {
                            "type": "string",
                            "description": "Output directory. Default: `runs/` relative to the working directory."
                        }
                    }
                }
            }
        ]
    })
}

async fn handle_request(req: JsonRpcRequest) -> JsonRpcResponse {
    let result = match req.method.as_str() {
        "initialize" => Some(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": serde_json::from_str::<Value>(SERVER_INFO).unwrap_or(json!({}))
        })),
        "tools/list" => Some(tool_list()),
        "tools/call" => {
            let name = req
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let args = req.params.get("arguments").cloned().unwrap_or(json!({}));
            match name {
                "crawler.capture_reference" => Some(tool_capture_reference(args).await),
                other => {
                    return JsonRpcResponse {
                        jsonrpc: "2.0",
                        id: req.id,
                        result: None,
                        error: Some(JsonRpcError {
                            code: -32601,
                            message: format!("unknown tool: {other}"),
                        }),
                    };
                }
            }
        }
        _ => None,
    };
    JsonRpcResponse {
        jsonrpc: "2.0",
        id: req.id,
        result,
        error: None,
    }
}

/// Spawn `crawler` with the supplied argv and wrap stdout/stderr
/// in an MCP `content`-shaped response. Centralises the spawn +
/// error path so each `tool_*` body stays short.
async fn run_crawler(label: &str, args: &[&str]) -> Value {
    let output = tokio::process::Command::new("crawler")
        .args(args)
        .output()
        .await;
    match output {
        Ok(out) if out.status.success() => json!({
            "content": [{
                "type": "text",
                "text": format!(
                    "stdout:\n{}\n\nstderr:\n{}",
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&out.stderr)
                )
            }]
        }),
        Ok(out) => json!({
            "isError": true,
            "content": [{
                "type": "text",
                "text": format!(
                    "crawler {label} exited {status}: {err}",
                    label = label,
                    status = out.status,
                    err = String::from_utf8_lossy(&out.stderr)
                )
            }]
        }),
        Err(e) => json!({
            "isError": true,
            "content": [{
                "type": "text",
                "text": format!("could not spawn crawler {label}: {e}")
            }]
        }),
    }
}

async fn tool_capture_reference(args: Value) -> Value {
    let Some(url) = args.get("url").and_then(|v| v.as_str()) else {
        return json!({
            "isError": true,
            "content": [{
                "type": "text",
                "text": "missing required argument: url"
            }]
        });
    };
    let mut crawler_args: Vec<&str> = vec!["--capture-reference", url];
    if let Some(slug) = args.get("site_slug").and_then(|v| v.as_str()) {
        crawler_args.push("--site-slug");
        crawler_args.push(slug);
    }
    if let Some(out) = args.get("out_dir").and_then(|v| v.as_str()) {
        crawler_args.push("--out-dir");
        crawler_args.push(out);
    }
    run_crawler("capture-reference", &crawler_args).await
}

#[tokio::main]
async fn main() -> Result<()> {
    let stdin = tokio::io::stdin();
    let mut reader = BufReader::new(stdin);
    let stdout = std::io::stdout();
    let mut buf = String::new();
    loop {
        buf.clear();
        let n = reader
            .read_line(&mut buf)
            .await
            .context("read stdin")?;
        if n == 0 {
            break;
        }
        let line = buf.trim();
        if line.is_empty() {
            continue;
        }
        let req: JsonRpcRequest = match serde_json::from_str(line) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("crawler-mcp: malformed json-rpc request: {e}");
                continue;
            }
        };
        let is_notification = req.id.is_none() && req.method.starts_with("notifications/");
        let resp = handle_request(req).await;
        if !is_notification {
            let mut out = stdout.lock();
            let line = serde_json::to_string(&resp).unwrap_or_else(|e| {
                format!(r#"{{"jsonrpc":"2.0","error":{{"code":-32603,"message":"{e}"}}}}"#)
            });
            writeln!(out, "{line}").ok();
            out.flush().ok();
        }
    }
    Ok(())
}
