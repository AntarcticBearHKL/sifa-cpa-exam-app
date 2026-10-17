//! `sfzklearning-mcp` — a minimal stdio MCP server (newline-delimited JSON-RPC 2.0)
//! exposing exactly one tool: `get_current_study_context`.
//!
//! Protocol bytes go to **stdout only**; all diagnostics go to **stderr**.

use std::io::{BufRead, BufWriter, Write};
use std::path::PathBuf;

use serde_json::{json, Value};

use sfzklearning::{current_json_path, parse_cli_args, read_current, resolve_state_dir};

const PROTOCOL_VERSION: &str = "2024-11-05";
const SERVER_NAME: &str = "sfzklearning";
const SERVER_VERSION: &str = "0.1.0";
const TOOL_NAME: &str = "get_current_study_context";

fn log(msg: &str) {
    let _ = writeln!(std::io::stderr(), "[sfzklearning-mcp] {msg}");
}

/// The single tool definition.
fn tool_list() -> Value {
    json!({
        "tools": [
            {
                "name": TOOL_NAME,
                "description": "返回用户当前正在做的题目及其关联知识点（含解析与出处），供 Agent 讲解",
                "inputSchema": {
                    "type": "object",
                    "properties": {},
                    "additionalProperties": false
                }
            }
        ]
    })
}

fn missing_state_message(state_dir: &PathBuf) -> String {
    format!(
        "当前没有正在学习的内容。\n\n请先启动 `sfzklearning` 并选择一道题目或一个知识点；\
         GUI 会把当前上下文写入：\n`{}`。\n\n写入后再次调用本工具即可获得题目、选项、正确答案、解析与关联知识点。",
        current_json_path(state_dir).display()
    )
}

/// Execute `tools/call` for the single tool.
fn call_tool(params: &Value, state_dir: &PathBuf) -> Result<Value, (i64, String)> {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    if name != TOOL_NAME {
        return Err((
            -32602,
            format!("未知工具: {name}（本服务仅提供 {TOOL_NAME}）"),
        ));
    }

    match read_current(state_dir) {
        Some(ctx) => {
            let text = ctx.to_markdown();
            let structured = serde_json::to_value(&ctx).unwrap_or(Value::Null);
            Ok(json!({
                "content": [ { "type": "text", "text": text } ],
                "structuredContent": structured,
                "isError": false
            }))
        }
        None => {
            let text = missing_state_message(state_dir);
            Ok(json!({
                "content": [ { "type": "text", "text": text } ],
                "isError": false
            }))
        }
    }
}

/// Handle one parsed JSON-RPC message.
/// Returns `None` for notifications (no reply is allowed).
fn handle(msg: &Value, state_dir: &PathBuf) -> Option<Value> {
    let id_present = msg.get("id").is_some();
    let id = msg.get("id").cloned().unwrap_or(Value::Null);
    let method = msg
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or_default()
        .to_string();

    // Notifications never get a reply.
    if method == "notifications/initialized" || method.starts_with("notifications/") {
        return None;
    }
    if !id_present {
        // A request without an id is a notification.
        return None;
    }

    let ok = |result: Value| Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }));
    let err = |code: i64, message: String| {
        Some(json!({
            "jsonrpc": "2.0",
            "id": id,
            "error": { "code": code, "message": message }
        }))
    };

    match method.as_str() {
        "initialize" => ok(json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION }
        })),
        "ping" => ok(json!({})),
        "tools/list" => ok(tool_list()),
        "tools/call" => {
            let params = msg.get("params").cloned().unwrap_or(Value::Null);
            match call_tool(&params, state_dir) {
                Ok(v) => ok(v),
                Err((code, message)) => err(code, message),
            }
        }
        other => err(-32601, format!("Method not found: {other}")),
    }
}

fn main() {
    let args = parse_cli_args();
    let state_dir = resolve_state_dir(args.state_dir.as_deref());
    log(&format!(
        "ready; state dir = {} (protocol {PROTOCOL_VERSION})",
        state_dir.display()
    ));

    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut out = BufWriter::new(stdout.lock());

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                log(&format!("stdin read error: {e}"));
                break;
            }
        };
        let trimmed = line.trim_start_matches('\u{FEFF}').trim();
        if trimmed.is_empty() {
            continue;
        }

        let msg: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                // Parse error: id is unknown, use null per JSON-RPC.
                let resp = json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": { "code": -32700, "message": format!("Parse error: {e}") }
                });
                let _ = writeln!(out, "{}", resp);
                let _ = out.flush();
                continue;
            }
        };

        // A batch (array) is not required by this server; answer each element.
        let messages: Vec<Value> = if msg.is_array() {
            msg.as_array().cloned().unwrap_or_default()
        } else {
            vec![msg]
        };

        for m in messages {
            if let Some(resp) = handle(&m, &state_dir) {
                match serde_json::to_string(&resp) {
                    Ok(s) => {
                        let _ = writeln!(out, "{s}");
                    }
                    Err(e) => log(&format!("serialize response failed: {e}")),
                }
            }
        }
        let _ = out.flush();
    }

    let _ = out.flush();
    log("stdin closed; exiting");
}
