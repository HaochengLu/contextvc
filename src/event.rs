use crate::paths::ContextPaths;
use crate::redact::redact_value;
use anyhow::{Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use ulid::Ulid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventActor {
    pub kind: String,
    pub name: String,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub host: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventGit {
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub head: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextEvent {
    pub id: String,
    pub ts: String,
    pub actor: EventActor,
    #[serde(rename = "type")]
    pub event_type: String,
    pub payload: Value,
    #[serde(default)]
    pub git: Option<EventGit>,
    #[serde(default)]
    pub redacted: bool,
}

impl ContextEvent {
    pub fn new(event_type: &str, actor_name: &str, payload: Value) -> Self {
        Self {
            id: format!("ev_{}", Ulid::new()),
            ts: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            actor: EventActor {
                kind: "agent".into(),
                name: actor_name.into(),
                session: None,
                host: hostname(),
            },
            event_type: event_type.into(),
            payload,
            git: None,
            redacted: false,
        }
    }
}

fn hostname() -> Option<String> {
    std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("COMPUTERNAME"))
        .ok()
}

pub fn append_event(paths: &ContextPaths, event: &ContextEvent) -> Result<PathBuf> {
    let _lock = crate::lockfile::FileLock::acquire(
        &paths.cache.join("events.lock"),
        "ctx event append operation",
    )?;
    let writer = event
        .actor
        .host
        .clone()
        .unwrap_or_else(|| "local".into())
        .replace('.', "-");
    let month = Utc::now().format("%Y-%m").to_string();
    let shard = paths.events.join(format!("{month}.{writer}.jsonl"));
    std::fs::create_dir_all(&paths.events)?;

    let mut payload = event.clone();
    payload.payload = redact_value(&payload.payload);
    payload.redacted = true;

    let mut line = serde_json::to_vec(&payload)?;
    line.push(b'\n');
    let mut file = OpenOptions::new().create(true).append(true).open(&shard)?;
    file.write_all(&line)?;
    file.sync_data()?;
    Ok(shard)
}

pub fn read_events(events_dir: &Path) -> Result<Vec<ContextEvent>> {
    let mut events = Vec::new();
    if !events_dir.exists() {
        return Ok(events);
    }
    for entry in std::fs::read_dir(events_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "jsonl") {
            let file = File::open(&path)?;
            for (line_no, line) in BufReader::new(file).lines().enumerate() {
                let line = line?;
                if line.trim().is_empty() {
                    continue;
                }
                let event = serde_json::from_str::<ContextEvent>(&line).with_context(|| {
                    format!("invalid event JSON in {}:{}", path.display(), line_no + 1)
                })?;
                events.push(event);
            }
        }
    }
    events.sort_by(|a, b| a.ts.cmp(&b.ts));
    Ok(events)
}

use std::path::PathBuf;

pub fn attach_git_context(mut event: ContextEvent, root: &Path) -> ContextEvent {
    if crate::is_git_repo(root) {
        let branch = crate::run_git(&["rev-parse", "--abbrev-ref", "HEAD"], root).ok();
        let head = crate::run_git(&["rev-parse", "HEAD"], root).ok();
        event.git = Some(EventGit { branch, head });
    }
    event
}
