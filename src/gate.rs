use crate::event::{append_event, ContextEvent};
use crate::index::search;
use crate::matchers::{command_pattern_matches, scope_matches};
use crate::object::{load_all_objects, KnowledgeObject, ObjectType};
use crate::paths::ContextPaths;
use anyhow::Result;
use serde::Serialize;
use serde_json::json;
use std::collections::HashSet;

#[derive(Debug, Clone, Serialize)]
pub struct PrecheckResult {
    pub severity: String,
    pub hits: Vec<GateHit>,
    pub suggested_action: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GateHit {
    pub id: String,
    pub object_type: String,
    pub title: String,
    pub enforcement: String,
    pub reason: String,
    pub stale_hint: Option<String>,
}

pub fn precheck(
    paths: &ContextPaths,
    path: Option<&str>,
    command: Option<&str>,
) -> Result<PrecheckResult> {
    let objects = load_all_objects(&paths.objects)?;
    let snoozed = active_snoozes(paths)?;
    let mut hits = Vec::new();

    if let Some(path) = path {
        hits.extend(check_path(paths, &objects, &snoozed, path)?);
    }
    if let Some(command) = command {
        hits.extend(check_command(&objects, &snoozed, command)?);
    }
    record_gate_hit(paths, path, command, &hits)?;

    let severity = hits
        .iter()
        .map(|h| h.enforcement.as_str())
        .max_by_key(|e| match *e {
            "block" => 3,
            "ask" => 2,
            "warn" => 1,
            _ => 0,
        })
        .unwrap_or("none")
        .to_string();

    let suggested_action = hits
        .first()
        .map(|h| format!("Review {} before proceeding: {}", h.id, h.reason));

    Ok(PrecheckResult {
        severity,
        hits,
        suggested_action,
    })
}

fn check_path(
    paths: &ContextPaths,
    objects: &[KnowledgeObject],
    snoozed: &HashSet<String>,
    target: &str,
) -> Result<Vec<GateHit>> {
    let mut hits = Vec::new();
    for obj in objects {
        if obj.frontmatter.status != "active" {
            continue;
        }
        if snoozed.contains(&obj.frontmatter.id) {
            continue;
        }
        if !scope_matches(&obj.frontmatter.scope, target) {
            continue;
        }
        if !has_path_gate_binding(obj) {
            continue;
        }
        let enforcement = obj
            .frontmatter
            .bindings
            .iter()
            .filter(|binding| matches!(binding.kind.as_str(), "file" | "source" | "symbol"))
            .find_map(|b| b.enforcement.clone())
            .unwrap_or_else(|| default_enforcement(obj));

        match obj.type_enum() {
            Some(ObjectType::Constraint) => {
                hits.push(GateHit {
                    id: obj.frontmatter.id.clone(),
                    object_type: "constraint".into(),
                    title: obj.frontmatter.title.clone(),
                    enforcement: enforcement.clone(),
                    reason: format!("Constraint applies to `{target}`: {}", summarize(&obj.body)),
                    stale_hint: binding_stale_hint(paths, obj, target),
                });
            }
            Some(ObjectType::Failure) => {
                hits.push(GateHit {
                    id: obj.frontmatter.id.clone(),
                    object_type: "failure".into(),
                    title: obj.frontmatter.title.clone(),
                    enforcement: "warn".into(),
                    reason: format!("Prior failure on `{target}`: {}", summarize(&obj.body)),
                    stale_hint: binding_stale_hint(paths, obj, target),
                });
            }
            _ => {}
        }
    }
    Ok(hits)
}

fn check_command(
    objects: &[KnowledgeObject],
    snoozed: &HashSet<String>,
    command: &str,
) -> Result<Vec<GateHit>> {
    let mut hits = Vec::new();
    for obj in objects {
        if obj.frontmatter.status != "active" {
            continue;
        }
        if snoozed.contains(&obj.frontmatter.id) {
            continue;
        }
        if obj.type_enum() != Some(ObjectType::Constraint) {
            continue;
        }
        for binding in &obj.frontmatter.bindings {
            if binding.kind == "command" {
                if let Some(pattern) = &binding.pattern {
                    if command_pattern_matches(command, pattern) {
                        hits.push(GateHit {
                            id: obj.frontmatter.id.clone(),
                            object_type: "constraint".into(),
                            title: obj.frontmatter.title.clone(),
                            enforcement: binding
                                .enforcement
                                .clone()
                                .unwrap_or_else(|| "warn".into()),
                            reason: format!("Command constraint: {}", obj.frontmatter.title),
                            stale_hint: None,
                        });
                    }
                }
            }
        }
    }
    Ok(hits)
}

fn default_enforcement(obj: &KnowledgeObject) -> String {
    match obj.type_enum() {
        Some(ObjectType::Constraint) => "warn".into(),
        _ => "warn".into(),
    }
}

fn has_path_gate_binding(obj: &KnowledgeObject) -> bool {
    obj.frontmatter.bindings.is_empty()
        || obj
            .frontmatter
            .bindings
            .iter()
            .any(|binding| matches!(binding.kind.as_str(), "file" | "source" | "symbol"))
}

fn binding_stale_hint(paths: &ContextPaths, obj: &KnowledgeObject, target: &str) -> Option<String> {
    for binding in &obj.frontmatter.bindings {
        if binding.kind == "file" {
            if let Some(path) = &binding.path {
                if path == target {
                    if let Some(expected) = &binding.sha {
                        let actual = crate::verify::safe_file_sha(paths, target).ok()?;
                        if &actual != expected {
                            return Some(format!(
                                "binding stale: file changed (was {expected}, now {actual})"
                            ));
                        }
                    }
                }
            }
        }
    }
    None
}

fn summarize(body: &str) -> String {
    body.lines()
        .find(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .unwrap_or(body)
        .chars()
        .take(160)
        .collect()
}

pub fn brief(paths: &ContextPaths, task: Option<&str>) -> Result<String> {
    let objects = load_all_objects(&paths.objects)?;
    let mut lines = vec!["# ContextVC Brief".to_string()];
    if let Some(task) = task {
        lines.push(format!("Task: {task}"));
    }
    for obj in objects
        .iter()
        .filter(|o| o.frontmatter.status == "active")
        .take(8)
    {
        if matches!(
            obj.type_enum(),
            Some(ObjectType::Constraint | ObjectType::Decision)
        ) {
            lines.push(format!(
                "- [{}] {}: {}",
                obj.frontmatter.object_type,
                obj.frontmatter.title,
                summarize(&obj.body)
            ));
        }
    }
    if let Some(task) = task {
        for hit in search(paths, task, None, 3)? {
            lines.push(format!(
                "- Related: {} — {}",
                hit.title,
                summarize(&hit.body)
            ));
        }
    }
    Ok(lines.join("\n"))
}

pub fn gate_for_hook(paths: &ContextPaths, tool: &str, input: &str) -> Result<PrecheckResult> {
    match tool {
        "Edit" | "Write" | "apply_patch" => precheck(paths, Some(input), None),
        "Bash" => precheck(paths, None, Some(input)),
        _ => precheck(paths, Some(input), None),
    }
}

pub fn snooze(paths: &ContextPaths, obj_id: &str, days: u32) -> Result<()> {
    let until = chrono::Utc::now() + chrono::Duration::days(days as i64);
    let mut event = ContextEvent::new(
        "gate_snooze",
        "ctx-gate",
        json!({
            "object_id": obj_id,
            "days": days,
            "until": until.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        }),
    );
    event = crate::event::attach_git_context(event, &paths.root);
    append_event(paths, &event)?;
    Ok(())
}

fn active_snoozes(paths: &ContextPaths) -> Result<HashSet<String>> {
    let now = chrono::Utc::now();
    let mut out = HashSet::new();
    for event in crate::event::read_events(&paths.events)? {
        if event.event_type != "gate_snooze" {
            continue;
        }
        let Some(obj_id) = event.payload.get("object_id").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(until) = event.payload.get("until").and_then(|v| v.as_str()) else {
            continue;
        };
        if chrono::DateTime::parse_from_rfc3339(until)
            .map(|ts| ts.with_timezone(&chrono::Utc) > now)
            .unwrap_or(false)
        {
            out.insert(obj_id.to_string());
        }
    }
    Ok(out)
}

fn record_gate_hit(
    paths: &ContextPaths,
    path: Option<&str>,
    command: Option<&str>,
    hits: &[GateHit],
) -> Result<()> {
    if hits.is_empty() {
        return Ok(());
    }
    let mut event = ContextEvent::new(
        "gate_hit",
        "ctx-gate",
        json!({
            "path": path,
            "command": command,
            "severity": hits
                .iter()
                .map(|h| h.enforcement.as_str())
                .max_by_key(|e| match *e {
                    "block" => 3,
                    "ask" => 2,
                    "warn" => 1,
                    _ => 0,
                })
                .unwrap_or("warn"),
            "hit_ids": hits.iter().map(|h| h.id.clone()).collect::<Vec<_>>(),
        }),
    );
    event = crate::event::attach_git_context(event, &paths.root);
    append_event(paths, &event)?;
    Ok(())
}
