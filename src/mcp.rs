use crate::event::{append_event, attach_git_context, ContextEvent};
use crate::gate::{brief, precheck};
use crate::index::search;
use crate::object::{new_object, slugify, KnowledgeObject, ObjectType};
use crate::paths::ContextPaths;
use crate::status::status;
use anyhow::Result;
use serde_json::{json, Value};
use std::io::{BufRead, Write};

pub fn serve_stdio(paths: &ContextPaths) -> Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(request) => request,
            Err(err) => {
                let response = error(None, -32700, &format!("parse error: {err}"));
                writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
                stdout.flush()?;
                continue;
            }
        };
        let id = request.get("id").cloned();
        let method = request.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let params = request.get("params").cloned().unwrap_or(json!({}));

        let response = match method {
            "initialize" => ok(id, initialize_result()),
            "tools/list" => ok(id, tools_list()),
            "tools/call" => match handle_tool_call(paths, &params) {
                Ok(result) => ok(id, result),
                Err(e) => error(id, -32000, &e.to_string()),
            },
            "notifications/initialized" => continue,
            _ => error(id, -32601, &format!("method not found: {method}")),
        };
        writeln!(stdout, "{}", serde_json::to_string(&response)?)?;
        stdout.flush()?;
    }
    Ok(())
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": "2024-11-05",
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "contextvc", "version": env!("CARGO_PKG_VERSION") }
    })
}

fn tools_list() -> Value {
    json!({
        "tools": [
            tool("context_brief", "Session/task brief pack within token budget", json!({"task": {"type": "string"}})),
            tool("context_search", "JIT search over knowledge objects", json!({"query": {"type": "string"}, "scope": {"type": "string"}})),
            tool("context_precheck", "Deterministic gate query for path or command", json!({"path": {"type": "string"}, "command": {"type": "string"}})),
            tool("context_log", "Append an event to the ledger", json!({"event": {"type": "object"}})),
            tool("context_propose", "Submit a knowledge proposal (never directly active)", json!({"object": {"type": "object"}})),
            tool("context_status", "Pending review / stale / conflict counts", json!({})),
        ]
    })
}

fn tool(name: &str, description: &str, schema: Value) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": {
            "type": "object",
            "properties": schema,
        }
    })
}

fn handle_tool_call(paths: &ContextPaths, params: &Value) -> Result<Value> {
    let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(json!({}));
    let content = match name {
        "context_brief" => {
            let task = args.get("task").and_then(|t| t.as_str());
            json!({ "text": brief(paths, task)? })
        }
        "context_search" => {
            let query = args.get("query").and_then(|q| q.as_str()).unwrap_or("");
            let scope = args.get("scope").and_then(|s| s.as_str());
            let hits = search(paths, query, scope, 10)?;
            json!({ "hits": hits })
        }
        "context_precheck" => {
            let path = args.get("path").and_then(|p| p.as_str());
            let command = args.get("command").and_then(|c| c.as_str());
            json!(precheck(paths, path, command)?)
        }
        "context_log" => {
            let event = context_log_event_from_args(&args);
            let event = attach_git_context(event, &paths.root);
            append_event(paths, &event)?;
            json!({ "ok": true, "id": event.id })
        }
        "context_propose" => {
            let proposal = propose_from_value(paths, &args)?;
            json!({ "ok": true, "id": proposal.frontmatter.id, "path": proposal.path })
        }
        "context_status" => {
            let report = status(paths)?;
            json!(report)
        }
        other => anyhow::bail!("unknown tool: {other}"),
    };
    Ok(json!({
        "content": [{ "type": "text", "text": serde_json::to_string_pretty(&content)? }],
        "isError": false
    }))
}

fn context_log_event_from_args(args: &Value) -> ContextEvent {
    let event_val = args.get("event").unwrap_or(args);
    let event_type = event_val
        .get("type")
        .or_else(|| event_val.get("event_type"))
        .or_else(|| args.get("type"))
        .or_else(|| args.get("event_type"))
        .and_then(|t| t.as_str())
        .filter(|t| !t.trim().is_empty())
        .unwrap_or("mcp_log");
    let payload = event_val
        .get("payload")
        .cloned()
        .unwrap_or_else(|| payload_without_reserved_event_fields(event_val));
    ContextEvent::new(event_type, "mcp-context-log", payload)
}

fn payload_without_reserved_event_fields(value: &Value) -> Value {
    let Value::Object(map) = value else {
        return value.clone();
    };
    let mut payload = map.clone();
    for key in ["id", "ts", "actor", "git", "redacted", "type", "event_type"] {
        payload.remove(key);
    }
    Value::Object(payload)
}

fn propose_from_value(paths: &ContextPaths, args: &Value) -> Result<KnowledgeObject> {
    let title = args
        .get("title")
        .or_else(|| args.get("object").and_then(|o| o.get("title")))
        .and_then(|t| t.as_str())
        .unwrap_or("Untitled proposal");
    let body = args
        .get("body")
        .or_else(|| args.get("object").and_then(|o| o.get("body")))
        .and_then(|b| b.as_str())
        .unwrap_or("");
    let object_type = args
        .get("type")
        .or_else(|| args.get("object").and_then(|o| o.get("type")))
        .and_then(|t| t.as_str())
        .and_then(ObjectType::from_str)
        .unwrap_or(ObjectType::Decision);
    let scope = scope_from_value(args).unwrap_or_else(|| vec!["**".into()]);

    let mut obj = new_object(object_type, title, body, scope, "proposed");
    obj.frontmatter.trust = "agent_auto".into();
    obj.path = paths.proposals.join(object_type.as_str()).join(format!(
        "{}-{}.md",
        slugify(title),
        &obj.frontmatter.id[2..]
    ));
    std::fs::create_dir_all(obj.path.parent().unwrap())?;
    obj.save()?;
    Ok(obj)
}

fn scope_from_value(args: &Value) -> Option<Vec<String>> {
    args.get("scope")
        .or_else(|| args.get("object").and_then(|o| o.get("scope")))
        .and_then(|s| s.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .filter(|scope| !scope.is_empty())
}

fn ok(id: Option<Value>, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn error(id: Option<Value>, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::read_events;
    use crate::init::{init_repo, InitOptions};
    use tempfile::TempDir;

    #[test]
    fn context_log_sanitizes_client_supplied_actor() {
        let tmp = TempDir::new().unwrap();
        let paths = init_repo(
            tmp.path(),
            InitOptions {
                skip_adopt: true,
                skip_backfill: true,
                skip_render: true,
            },
        )
        .unwrap();

        handle_tool_call(
            &paths,
            &json!({
                "name": "context_log",
                "arguments": {
                    "event": {
                        "id": "ev_fake",
                        "ts": "2026-01-01T00:00:00Z",
                        "actor": { "kind": "human", "name": "mallory" },
                        "type": "attempt",
                        "payload": {
                            "outcome": "failed",
                            "evidence": format!(
                                "token={}{}",
                                "ghp_",
                                "abcdefghijklmnopqrstuvwxyz1234567890"
                            )
                        }
                    }
                }
            }),
        )
        .unwrap();

        let events = read_events(&paths.events).unwrap();
        assert_eq!(events.len(), 1);
        assert_ne!(events[0].id, "ev_fake");
        assert_eq!(events[0].event_type, "attempt");
        assert_eq!(events[0].actor.kind, "agent");
        assert_eq!(events[0].actor.name, "mcp-context-log");
        assert!(events[0].redacted);
        assert!(events[0]
            .payload
            .get("evidence")
            .and_then(|v| v.as_str())
            .unwrap()
            .contains("[REDACTED]"));
    }
}
