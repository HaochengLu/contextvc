use crate::event::{append_event, attach_git_context, read_events, ContextEvent};
use crate::object::{load_all_objects, load_proposals, new_object, slugify, ObjectType};
use crate::paths::ContextPaths;
use anyhow::Result;
use serde_json::{json, Value};
use std::collections::HashSet;

pub fn distill_session(paths: &ContextPaths, session: Option<&str>) -> Result<usize> {
    let _lock = crate::lockfile::OperationLock::acquire(paths)?;
    let events = read_events(&paths.events)?;
    let mut processed = processed_event_ids(paths, &events)?;
    let filtered: Vec<_> = events
        .iter()
        .filter(|e| {
            session
                .map(|s| e.actor.session.as_deref() == Some(s))
                .unwrap_or(true)
        })
        .filter(|e| e.event_type == "attempt" || e.event_type == "outcome")
        .collect();

    let mut created = 0;
    for event in filtered {
        if processed.contains(&event.id) {
            continue;
        }
        if let Some(proposal) = proposal_from_event(paths, event)? {
            proposal.save()?;
            processed.insert(event.id.clone());
            created += 1;
        }
    }
    Ok(created)
}

fn processed_event_ids(paths: &ContextPaths, events: &[ContextEvent]) -> Result<HashSet<String>> {
    let mut ids = HashSet::new();
    for obj in load_all_objects(&paths.objects)?
        .into_iter()
        .chain(load_proposals(&paths.proposals)?.into_iter())
    {
        ids.extend(obj.frontmatter.evidence);
    }
    for event in events {
        if event.event_type == "distill_rejected" {
            if let Some(evidence) = event.payload.get("evidence").and_then(|v| v.as_array()) {
                ids.extend(
                    evidence
                        .iter()
                        .filter_map(|value| value.as_str().map(ToOwned::to_owned)),
                );
            }
        }
    }
    Ok(ids)
}

fn proposal_from_event(
    paths: &ContextPaths,
    event: &ContextEvent,
) -> Result<Option<crate::object::KnowledgeObject>> {
    let outcome = event
        .payload
        .get("outcome")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if outcome != "failed" {
        return Ok(None);
    }
    let target = event
        .payload
        .get("target")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    let action = event
        .payload
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("attempt");
    let evidence = event
        .payload
        .get("evidence")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let has_machine_evidence = !evidence.is_empty()
        && (evidence.contains("failed") || evidence.contains("error") || evidence.contains("exit"));

    let object_type = ObjectType::Failure;
    let title = format!("Failed: {action} on {target}");
    let body = format!(
        "## Outcome\n{outcome}\n\n## Target\n`{target}`\n\n## Evidence\n{evidence}\n\n## Source\nAuto-distilled from event {}",
        event.id
    );
    let mut obj = new_object(
        object_type,
        &title,
        &body,
        vec![target.into()],
        if has_machine_evidence {
            "proposed"
        } else {
            "proposed"
        },
    );
    obj.frontmatter.evidence = vec![event.id.clone()];
    obj.frontmatter.trust = if has_machine_evidence {
        "agent_verified".into()
    } else {
        "agent_auto".into()
    };
    obj.path = paths.proposals.join("failures").join(format!(
        "{}-{}.md",
        slugify(&title),
        &obj.frontmatter.id[2..]
    ));
    std::fs::create_dir_all(obj.path.parent().unwrap())?;
    Ok(Some(obj))
}

pub fn log_attempt(paths: &ContextPaths, payload: Value) -> Result<String> {
    let mut event = ContextEvent::new("attempt", "ctx-hook", payload);
    event = attach_git_context(event, &paths.root);
    append_event(paths, &event)?;
    Ok(event.id)
}

pub fn log_outcome(paths: &ContextPaths, payload: Value) -> Result<String> {
    let mut event = ContextEvent::new("outcome", "ctx-hook", payload);
    event = attach_git_context(event, &paths.root);
    append_event(paths, &event)?;
    Ok(event.id)
}

pub fn handle_session_start(paths: &ContextPaths) -> Result<String> {
    crate::gate::brief(paths, None)
}

pub fn handle_pre_tool(paths: &ContextPaths, input: &str) -> Result<Value> {
    let parsed: Value = serde_json::from_str(input).unwrap_or(json!({}));
    let tool = parsed
        .get("tool_name")
        .or_else(|| parsed.get("tool"))
        .and_then(|t| t.as_str())
        .unwrap_or("");
    let file = parsed
        .get("tool_input")
        .and_then(|ti| ti.get("file_path"))
        .and_then(|f| f.as_str())
        .or_else(|| parsed.get("path").and_then(|p| p.as_str()))
        .unwrap_or("");
    let command = parsed
        .get("tool_input")
        .and_then(|ti| ti.get("command"))
        .and_then(|c| c.as_str())
        .or_else(|| parsed.get("command").and_then(|c| c.as_str()))
        .or_else(|| parsed.get("cmd").and_then(|c| c.as_str()))
        .or_else(|| {
            parsed
                .get("payload")
                .and_then(|p| p.get("command"))
                .and_then(|c| c.as_str())
        })
        .unwrap_or("");

    let result = if tool == "Bash" || !command.is_empty() {
        crate::gate::precheck(paths, None, Some(command))?
    } else if !file.is_empty() {
        crate::gate::precheck(paths, Some(file), None)?
    } else if !tool.is_empty() {
        crate::gate::precheck(paths, Some(tool), None)?
    } else {
        return Ok(json!({
            "permissionDecision": "allow",
            "permission": "allow"
        }));
    };

    hook_gate_response(&result)
}

pub fn handle_post_tool(paths: &ContextPaths, input: &str) -> Result<()> {
    let parsed: Value = serde_json::from_str(input).unwrap_or(json!({}));
    let success = parsed
        .get("success")
        .and_then(|s| s.as_bool())
        .unwrap_or(true);
    if !success {
        let payload = json!({
            "target": parsed.get("path").or_else(|| parsed.get("tool_input").and_then(|t| t.get("file_path"))),
            "action": parsed.get("tool_name"),
            "outcome": "failed",
            "evidence": parsed.get("error").unwrap_or(&json!("tool failed"))
        });
        log_outcome(paths, payload)?;
    }
    Ok(())
}

pub fn handle_stop(paths: &ContextPaths) -> Result<usize> {
    distill_session(paths, None)
}

pub fn handle_user_prompt_submit(paths: &ContextPaths, input: &str) -> Result<Value> {
    let parsed: Value = serde_json::from_str(input).unwrap_or(json!({}));
    let prompt = parsed
        .get("prompt")
        .or_else(|| parsed.get("message"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if prompt.trim().is_empty() {
        return Ok(json!({}));
    }
    let brief = crate::gate::brief(paths, Some(prompt))?;
    Ok(json!({
        "additionalContext": brief,
        "agent_message": brief,
    }))
}

pub fn handle_precompact(paths: &ContextPaths, _input: &str) -> Result<Value> {
    Ok(json!({
        "additionalContext": crate::gate::brief(paths, None)?
    }))
}

fn hook_gate_response(result: &crate::gate::PrecheckResult) -> Result<Value> {
    if result.hits.is_empty() {
        return Ok(json!({
            "permissionDecision": "allow",
            "permission": "allow"
        }));
    }
    let hits = serde_json::to_string(&result.hits)?;
    let message = result
        .suggested_action
        .clone()
        .unwrap_or_else(|| "Review ContextVC gate hits before proceeding".into());
    match result.severity.as_str() {
        "block" => Ok(json!({
            "permissionDecision": "deny",
            "permissionDecisionReason": message,
            "permission": "deny",
            "agent_message": message,
            "additionalContext": hits,
        })),
        "ask" => Ok(json!({
            "permissionDecision": "ask",
            "permissionDecisionReason": message,
            "permission": "ask",
            "agent_message": message,
            "additionalContext": hits,
        })),
        _ => Ok(json!({
            "permissionDecision": "allow",
            "permission": "allow",
            "agent_message": message,
            "additionalContext": hits,
        })),
    }
}
