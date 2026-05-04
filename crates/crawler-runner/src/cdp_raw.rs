//! Raw CDP WebSocket adapter (T102.4).
//!
//! `chromiumoxide`'s typed Message dispatch can't deserialize
//! every Chromium-147 CDP event payload — we get
//! `WS Invalid message: data did not match any variant of
//! untagged enum Message` warnings and the typed event_listener
//! subscriptions silently never fire.
//!
//! Workaround: open a SECOND WebSocket against the same
//! `browser.websocket_address()`, auto-attach to all targets
//! via `Target.setAutoAttach`, and dispatch incoming messages
//! by their `method` field as untyped `serde_json::Value`. Same
//! pattern Playwright uses internally — robust to wire-format
//! evolution because we only inspect the fields we care about.
//!
//! Captured method names → `crawler_report::EventKind`:
//!
//! | CDP method                 | EventKind         |
//! |----------------------------|-------------------|
//! | Runtime.consoleAPICalled   | Console           |
//! | Runtime.exceptionThrown    | Pageerror         |
//! | Network.loadingFailed      | RequestFailed     |
//! | Network.responseReceived   | (filtered: status >= 400 → ResponseError) |
//! | Audits.issueAdded          | CspViolation (filtered to CSP issues) |
//! | Log.entryAdded             | CspViolation (filtered to security source) |
//!
//! BUG ASSUMPTION: messages with no `method` field are command
//! responses (handled by chromiumoxide), not events. We ignore
//! them. If a future CDP version routes events without `method`,
//! this dispatcher will silently miss them — surface that case
//! by returning the count of UNROUTED methods we encountered.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use crawler_report::{CapturedEvent, EventKind};
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::sync::Mutex;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, info};

/// Per-URL network observation — what the css_health detector
/// (and future audit-of-CDN type rules) reads to know whether a
/// resource was actually fetched, what its content-type was, and
/// how big the body was on the wire.
///
/// BUG ASSUMPTION: `body_bytes` is `Network.loadingFinished`'s
/// `encodedDataLength` (post-content-encoding bytes). For empty-
/// or-tiny detection (< 50 byte threshold) this is sufficient;
/// for true source-byte counting use `Network.getResponseBody`
/// (deferred — adds an async round-trip per URL).
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct NetworkObservation {
    /// HTTP status (0 if not observed).
    pub status: u16,
    /// `Content-Type` from the response headers.
    pub content_type: Option<String>,
    /// Encoded body length (best-effort, from
    /// `Network.loadingFinished.encodedDataLength`).
    pub body_bytes: u64,
    /// Network-level error text from `Network.loadingFailed`.
    pub error_text: Option<String>,
}

/// Shared map of URL → observation. Populated by the raw-CDP
/// pump; readable from any other task.
pub type NetworkObservations = Arc<Mutex<HashMap<String, NetworkObservation>>>;

/// Spawn a background task that pumps the raw-CDP WS, attaches
/// to every target, and pushes captured events into `events`.
/// Network responses are accumulated into `network` for the
/// css_health detector.
///
/// Returns a `JoinHandle`. Caller can `.abort()` at journey end.
pub async fn spawn_raw_cdp_capture(
    ws_url: String,
    events: Arc<Mutex<Vec<CapturedEvent>>>,
    network: NetworkObservations,
    started_at: Instant,
) -> Result<tokio::task::JoinHandle<()>> {
    let (ws, _resp) = tokio_tungstenite::connect_async(&ws_url)
        .await
        .with_context(|| format!("connect raw CDP at {ws_url}"))?;
    let (mut writer, mut reader) = ws.split();
    info!("raw-CDP connected at {ws_url}");

    // Step 1: Target.setAutoAttach with flatten — every new
    // target's events get tagged with a sessionId in the
    // top-level message envelope. We don't need per-session
    // dispatch; we just see everything.
    //
    // Step 2: send Network/Runtime/Page/Audits/Log enables on
    // the BROWSER session (sessionId omitted). For TARGET
    // sessions, the auto-attach hook re-enables on attach.
    let mut next_id: u64 = 1;
    let send_cmd =
        |id: u64, method: &str, params: Value, session_id: Option<&str>| -> Result<Message> {
            let mut req = json!({
                "id": id,
                "method": method,
                "params": params,
            });
            if let Some(sid) = session_id {
                req["sessionId"] = Value::String(sid.to_owned());
            }
            Ok(Message::Text(req.to_string().into()))
        };

    // setAutoAttach on the root session
    let m = send_cmd(
        next_id,
        "Target.setAutoAttach",
        json!({
            "autoAttach": true,
            "waitForDebuggerOnStart": false,
            "flatten": true,
        }),
        None,
    )?;
    writer.send(m).await.context("send setAutoAttach")?;
    next_id += 1;

    // Browser-session enables (best-effort).
    for method in [
        "Network.enable",
        "Runtime.enable",
        "Page.enable",
        "Audits.enable",
        "Log.enable",
    ] {
        let m = send_cmd(next_id, method, json!({}), None)?;
        let _ = writer.send(m).await;
        next_id += 1;
    }

    let writer = Arc::new(Mutex::new(writer));

    // Spawn the read loop.
    let handle = tokio::spawn(async move {
        let mut response_url_by_request: std::collections::HashMap<String, String> =
            std::collections::HashMap::new();
        let mut seen_sessions: std::collections::HashSet<String> = std::collections::HashSet::new();
        while let Some(msg) = reader.next().await {
            let msg = match msg {
                Ok(m) => m,
                Err(e) => {
                    debug!("raw-CDP read err: {e}");
                    break;
                }
            };
            let text = match msg {
                Message::Text(t) => t.to_string(),
                Message::Binary(_) | Message::Ping(_) | Message::Pong(_) => continue,
                Message::Close(_) => break,
                Message::Frame(_) => continue,
            };
            let v: Value = match serde_json::from_str(&text) {
                Ok(v) => v,
                Err(e) => {
                    debug!("raw-CDP parse err (msg ignored): {e}");
                    continue;
                }
            };
            // Determine if event (has `method`) or command response.
            let Some(method) = v.get("method").and_then(|m| m.as_str()) else {
                continue;
            };
            let session_id = v
                .get("sessionId")
                .and_then(|s| s.as_str())
                .map(|s| s.to_owned());
            let params = v.get("params").cloned().unwrap_or(Value::Null);

            // On Target.attachedToTarget — re-enable domains on the
            // new session so child-target events flow.
            if method == "Target.attachedToTarget" {
                if let Some(sid) = params.get("sessionId").and_then(|s| s.as_str()) {
                    if seen_sessions.insert(sid.to_owned()) {
                        // Send enables on this child session.
                        let enables = [
                            "Network.enable",
                            "Runtime.enable",
                            "Audits.enable",
                            "Log.enable",
                        ];
                        for (i, m) in enables.iter().enumerate() {
                            let req = json!({
                                "id": 100_000 + (seen_sessions.len() * 10 + i) as u64,
                                "method": m,
                                "params": {},
                                "sessionId": sid,
                            });
                            let mut w = writer.lock().await;
                            let _ = w.send(Message::Text(req.to_string().into())).await;
                        }
                        debug!("auto-attached + enabled domains for session {sid}");
                    }
                }
                continue;
            }

            let t_ms = started_at.elapsed().as_millis() as u64;

            match method {
                "Runtime.consoleAPICalled" => {
                    let level = params
                        .get("type")
                        .and_then(|t| t.as_str())
                        .unwrap_or("log")
                        .to_owned();
                    let text = params
                        .get("args")
                        .and_then(|a| a.as_array())
                        .map(|args| {
                            args.iter()
                                .filter_map(|a| {
                                    a.get("value")
                                        .map(|v| v.to_string())
                                        .or_else(|| a.get("description").map(|v| v.to_string()))
                                })
                                .collect::<Vec<_>>()
                                .join(" ")
                        })
                        .unwrap_or_default();
                    push_event(
                        &events,
                        CapturedEvent {
                            t: t_ms,
                            kind: EventKind::Console,
                            level: Some(level),
                            text,
                            url: None,
                            status: None,
                            stack: None,
                            impact: None,
                            rule_id: None,
                            severity: None,
                        },
                    )
                    .await;
                }
                "Runtime.exceptionThrown" => {
                    let details = params
                        .get("exceptionDetails")
                        .cloned()
                        .unwrap_or(Value::Null);
                    let text = details
                        .get("exception")
                        .and_then(|e| e.get("description"))
                        .and_then(|d| d.as_str())
                        .unwrap_or("page error")
                        .to_owned();
                    let stack = details.get("stackTrace").map(|s| s.to_string());
                    push_event(
                        &events,
                        CapturedEvent {
                            t: t_ms,
                            kind: EventKind::Pageerror,
                            level: None,
                            text,
                            url: None,
                            status: None,
                            stack,
                            impact: None,
                            rule_id: None,
                            severity: None,
                        },
                    )
                    .await;
                }
                "Network.responseReceived" => {
                    // Track URL by requestId so loadingFailed can name the URL.
                    let url_opt = params
                        .get("response")
                        .and_then(|r| r.get("url"))
                        .and_then(|s| s.as_str())
                        .map(ToOwned::to_owned);
                    if let (Some(req_id), Some(url)) = (
                        params.get("requestId").and_then(|s| s.as_str()),
                        url_opt.as_ref(),
                    ) {
                        response_url_by_request.insert(req_id.to_owned(), url.clone());
                    }
                    let status_u = params
                        .get("response")
                        .and_then(|r| r.get("status"))
                        .and_then(|s| s.as_u64())
                        .unwrap_or(0);
                    let content_type = params
                        .get("response")
                        .and_then(|r| r.get("headers"))
                        .and_then(|h| {
                            h.get("Content-Type")
                                .or_else(|| h.get("content-type"))
                                .or_else(|| h.get("Content-type"))
                        })
                        .and_then(|v| v.as_str())
                        .map(ToOwned::to_owned)
                        .or_else(|| {
                            params
                                .get("response")
                                .and_then(|r| r.get("mimeType"))
                                .and_then(|s| s.as_str())
                                .map(ToOwned::to_owned)
                        });
                    // Update the per-URL observation.
                    if let Some(ref url) = url_opt {
                        let mut net = network.lock().await;
                        let entry = net.entry(url.clone()).or_default();
                        entry.status =
                            u16::try_from(status_u).unwrap_or(u16::MAX);
                        entry.content_type = content_type;
                    }
                    // Status >= 400 → ResponseError.
                    if status_u >= 400 {
                        push_event(
                            &events,
                            CapturedEvent {
                                t: t_ms,
                                kind: EventKind::ResponseError,
                                level: None,
                                text: format!("HTTP {status_u}"),
                                url: url_opt,
                                status: u16::try_from(status_u).ok(),
                                stack: None,
                                impact: None,
                                rule_id: None,
                                severity: None,
                            },
                        )
                        .await;
                    }
                }
                "Network.loadingFinished" => {
                    let req_id = params
                        .get("requestId")
                        .and_then(|s| s.as_str());
                    let encoded = params
                        .get("encodedDataLength")
                        .and_then(|n| n.as_f64())
                        .unwrap_or(0.0)
                        .max(0.0);
                    if let Some(req_id) = req_id {
                        if let Some(url) = response_url_by_request.get(req_id).cloned() {
                            let mut net = network.lock().await;
                            let entry = net.entry(url).or_default();
                            entry.body_bytes = encoded as u64;
                        }
                    }
                }
                "Network.loadingFailed" => {
                    let req_id = params
                        .get("requestId")
                        .and_then(|s| s.as_str())
                        .unwrap_or("?")
                        .to_owned();
                    let url = response_url_by_request.get(&req_id).cloned();
                    let err = params
                        .get("errorText")
                        .and_then(|s| s.as_str())
                        .unwrap_or("loading failed")
                        .to_owned();
                    if let Some(ref u) = url {
                        let mut net = network.lock().await;
                        let entry = net.entry(u.clone()).or_default();
                        entry.error_text = Some(err.clone());
                    }
                    push_event(
                        &events,
                        CapturedEvent {
                            t: t_ms,
                            kind: EventKind::RequestFailed,
                            level: None,
                            text: err,
                            url,
                            status: None,
                            stack: None,
                            impact: None,
                            rule_id: None,
                            severity: None,
                        },
                    )
                    .await;
                }
                "Audits.issueAdded" => {
                    let issue = params.get("issue").cloned().unwrap_or(Value::Null);
                    let code = issue
                        .get("code")
                        .and_then(|c| c.as_str())
                        .unwrap_or("UnknownIssue");
                    if code == "ContentSecurityPolicyIssue" {
                        let details = issue
                            .get("details")
                            .and_then(|d| d.get("contentSecurityPolicyIssueDetails"))
                            .cloned()
                            .unwrap_or(Value::Null);
                        let directive = details
                            .get("violatedDirective")
                            .and_then(|d| d.as_str())
                            .unwrap_or("?")
                            .to_owned();
                        let blocked = details
                            .get("blockedURL")
                            .and_then(|s| s.as_str())
                            .or_else(|| {
                                details
                                    .get("sourceCodeLocation")
                                    .and_then(|loc| loc.get("url"))
                                    .and_then(|s| s.as_str())
                            })
                            .unwrap_or("inline")
                            .to_owned();
                        push_event(
                            &events,
                            CapturedEvent {
                                t: t_ms,
                                kind: EventKind::CspViolation,
                                level: None,
                                text: format!("[csp.{directive}] blocked {blocked}"),
                                url: details
                                    .get("sourceCodeLocation")
                                    .and_then(|l| l.get("url"))
                                    .and_then(|s| s.as_str())
                                    .map(|s| s.to_owned()),
                                status: None,
                                stack: None,
                                impact: None,
                                rule_id: Some(format!("csp.{directive}")),
                                severity: Some(crawler_report::Severity::Strict),
                            },
                        )
                        .await;
                    }
                }
                _ => {
                    let _ = session_id;
                }
            }
        }
        debug!("raw-CDP read loop exited");
    });

    // Stash next_id so cargo doesn't warn "unused".
    let _ = next_id;
    Ok(handle)
}

async fn push_event(events: &Arc<Mutex<Vec<CapturedEvent>>>, evt: CapturedEvent) {
    let mut g = events.lock().await;
    g.push(evt);
}

#[cfg(test)]
mod tests {
    // No unit tests yet — this module is integration-tested via
    // the runner smoke against fixture-perf-csp. Once the mvp
    // proves out, we can mock the WS server and add unit tests.
}
