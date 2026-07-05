use crate::event::read_events;
use crate::matchers::scope_matches;
use crate::object::{load_all_objects, KnowledgeObject};
use crate::paths::ContextPaths;
use anyhow::Result;
use serde_json::json;

pub fn log_scope(paths: &ContextPaths, scope: Option<&str>) -> Result<Vec<LogEntry>> {
    let objects = load_all_objects(&paths.objects)?;
    let events = read_events(&paths.events)?;
    let mut entries = Vec::new();

    for obj in objects {
        if let Some(scope) = scope {
            if !scope_matches(&obj.frontmatter.scope, scope) {
                continue;
            }
        }
        entries.push(LogEntry {
            id: obj.frontmatter.id.clone(),
            object_type: obj.frontmatter.object_type.clone(),
            title: obj.frontmatter.title.clone(),
            status: obj.frontmatter.status.clone(),
            created: obj.frontmatter.created.clone(),
        });
    }

    for event in events {
        if let Some(scope) = scope {
            let target = event
                .payload
                .get("target")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if !target.is_empty() && !scope_matches(&[scope.into()], target) {
                continue;
            }
        }
        entries.push(LogEntry {
            id: event.id.clone(),
            object_type: event.event_type.clone(),
            title: event
                .payload
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("event")
                .into(),
            status: event
                .payload
                .get("outcome")
                .and_then(|v| v.as_str())
                .unwrap_or("-")
                .into(),
            created: event.ts.clone(),
        });
    }
    entries.extend(git_log_entries(paths, scope)?);

    entries.sort_by(|a, b| a.created.cmp(&b.created));
    Ok(entries)
}

#[derive(Debug, serde::Serialize)]
pub struct LogEntry {
    pub id: String,
    pub object_type: String,
    pub title: String,
    pub status: String,
    pub created: String,
}

pub fn blame(paths: &ContextPaths, query: &str) -> Result<BlameReport> {
    let objects = load_all_objects(&paths.objects)?;
    let events = read_events(&paths.events)?;
    let matched: Vec<_> = objects
        .iter()
        .filter(|o| {
            o.frontmatter
                .title
                .to_lowercase()
                .contains(&query.to_lowercase())
                || o.frontmatter.id.contains(query)
                || o.body.to_lowercase().contains(&query.to_lowercase())
        })
        .collect();

    let mut evidence = Vec::new();
    for obj in &matched {
        for ev_id in &obj.frontmatter.evidence {
            if let Some(ev) = events.iter().find(|e| &e.id == ev_id) {
                evidence.push(EvidenceLine {
                    event_id: ev.id.clone(),
                    ts: ev.ts.clone(),
                    actor: ev.actor.name.clone(),
                    summary: ev.payload.to_string(),
                });
            }
        }
    }

    Ok(BlameReport {
        objects: matched.iter().map(|o| o.frontmatter.id.clone()).collect(),
        evidence,
        git: matched
            .iter()
            .flat_map(|obj| git_blame_lines(paths, obj).unwrap_or_default())
            .collect(),
    })
}

#[derive(Debug, serde::Serialize)]
pub struct BlameReport {
    pub objects: Vec<String>,
    pub evidence: Vec<EvidenceLine>,
    pub git: Vec<GitLine>,
}

#[derive(Debug, serde::Serialize)]
pub struct EvidenceLine {
    pub event_id: String,
    pub ts: String,
    pub actor: String,
    pub summary: String,
}

#[derive(Debug, serde::Serialize)]
pub struct GitLine {
    pub object_id: String,
    pub path: String,
    pub commit: String,
    pub author: String,
    pub date: String,
}

pub fn diff(
    paths: &ContextPaths,
    scope: Option<&str>,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<DiffReport> {
    let objects = load_all_objects(&paths.objects)?;
    let proposals = crate::object::load_proposals(&paths.proposals)?;
    let mut added = Vec::new();
    let mut deprecated = Vec::new();
    let mut conflicted = Vec::new();

    for obj in &objects {
        if let Some(scope) = scope {
            if !scope_matches(&obj.frontmatter.scope, scope) {
                continue;
            }
        }
        match obj.frontmatter.status.as_str() {
            "deprecated" => deprecated.push(obj.frontmatter.id.clone()),
            "conflicted" => conflicted.push(obj.frontmatter.id.clone()),
            _ => {}
        }
    }
    let proposal_count = proposals.len();
    for p in proposals {
        added.push(p.frontmatter.id.clone());
    }

    let historical = if from.is_some() || to.is_some() {
        Some(frontmatter_diff(paths, from, to)?)
    } else {
        None
    };

    Ok(DiffReport {
        added: historical
            .as_ref()
            .map(|diff| diff.added.clone())
            .unwrap_or(added),
        deprecated,
        conflicted,
        proposals: proposal_count,
        git_changes: git_diff(paths, from, to).unwrap_or_default(),
        status_changes: historical
            .map(|diff| diff.status_changes)
            .unwrap_or_default(),
    })
}

#[derive(Debug, serde::Serialize)]
pub struct DiffReport {
    pub added: Vec<String>,
    pub deprecated: Vec<String>,
    pub conflicted: Vec<String>,
    pub proposals: usize,
    pub git_changes: Vec<GitChange>,
    pub status_changes: Vec<ObjectStatusChange>,
}

#[derive(Debug, serde::Serialize)]
pub struct GitChange {
    pub status: String,
    pub path: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ObjectStatusChange {
    pub id: String,
    pub from: Option<String>,
    pub to: Option<String>,
}

pub fn revert(paths: &ContextPaths, obj_id: &str) -> Result<KnowledgeObject> {
    let _lock = crate::lockfile::OperationLock::acquire(paths)?;
    let objects = load_all_objects(&paths.objects)?;
    let obj = objects
        .iter()
        .find(|o| o.frontmatter.id == obj_id)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("object not found: {obj_id}"))?;
    let mut updated = obj.clone();
    updated.frontmatter.status = "deprecated".into();
    updated.save()?;

    let mut cascaded = Vec::new();
    for dependent in objects {
        if dependent.frontmatter.id == obj_id || dependent.frontmatter.status != "active" {
            continue;
        }
        let depends = dependent.frontmatter.supersedes.as_deref() == Some(obj_id)
            || dependent.frontmatter.evidence.iter().any(|ev| ev == obj_id)
            || dependent.body.contains(obj_id);
        if depends
            && matches!(
                dependent.type_enum(),
                Some(crate::object::ObjectType::Decision | crate::object::ObjectType::Howto)
            )
        {
            let mut dep = dependent.clone();
            dep.frontmatter.status = "conflicted".into();
            dep.save()?;
            cascaded.push(dep.frontmatter.id);
        }
    }

    let mut event = crate::event::ContextEvent::new(
        "revert",
        "ctx-version",
        json!({
            "object_id": obj_id,
            "cascaded_conflicts": cascaded,
        }),
    );
    event = crate::event::attach_git_context(event, &paths.root);
    crate::event::append_event(paths, &event)?;
    Ok(updated)
}

fn git_blame_lines(paths: &ContextPaths, obj: &KnowledgeObject) -> Result<Vec<GitLine>> {
    if !crate::is_git_repo(&paths.root) {
        return Ok(Vec::new());
    }
    let rel = obj.path.strip_prefix(&paths.root).unwrap_or(&obj.path);
    let rel_s = rel.to_string_lossy().into_owned();
    let output = crate::run_git(
        &["log", "--format=%H%x09%an%x09%aI", "--", &rel_s],
        &paths.root,
    )?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            Some(GitLine {
                object_id: obj.frontmatter.id.clone(),
                path: rel.to_string_lossy().into_owned(),
                commit: parts.next()?.into(),
                author: parts.next()?.into(),
                date: parts.next()?.into(),
            })
        })
        .collect())
}

fn git_diff(paths: &ContextPaths, from: Option<&str>, to: Option<&str>) -> Result<Vec<GitChange>> {
    if !crate::is_git_repo(&paths.root) {
        return Ok(Vec::new());
    }
    let range = match (from, to) {
        (Some(from), Some(to)) => format!("{from}..{to}"),
        (Some(from), None) => format!("{from}..HEAD"),
        (None, Some(to)) => to.to_string(),
        (None, None) => "--".into(),
    };
    let args = if range == "--" {
        vec!["diff", "--name-status", "--", ".context/objects"]
    } else {
        vec!["diff", "--name-status", &range, "--", ".context/objects"]
    };
    let output = crate::run_git(&args, &paths.root)?;
    Ok(output
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            Some(GitChange {
                status: parts.next()?.to_string(),
                path: parts.next()?.to_string(),
            })
        })
        .collect())
}

fn git_log_entries(paths: &ContextPaths, scope: Option<&str>) -> Result<Vec<LogEntry>> {
    if !crate::is_git_repo(&paths.root) {
        return Ok(Vec::new());
    }
    let output = crate::run_git(
        &[
            "log",
            "--format=%H%x09%aI%x09%an%x09%s",
            "--",
            ".context/objects",
        ],
        &paths.root,
    )
    .unwrap_or_default();
    let mut entries = Vec::new();
    for line in output.lines() {
        let mut parts = line.splitn(4, '\t');
        let commit = parts.next().unwrap_or("");
        let created = parts.next().unwrap_or("");
        let author = parts.next().unwrap_or("");
        let subject = parts.next().unwrap_or("");
        if let Some(scope) = scope {
            if !subject.contains(scope) {
                continue;
            }
        }
        entries.push(LogEntry {
            id: commit.to_string(),
            object_type: "git_commit".into(),
            title: format!("{subject} ({author})"),
            status: "-".into(),
            created: created.to_string(),
        });
    }
    Ok(entries)
}

#[derive(Default)]
struct FrontmatterDiff {
    added: Vec<String>,
    status_changes: Vec<ObjectStatusChange>,
}

fn frontmatter_diff(
    paths: &ContextPaths,
    from: Option<&str>,
    to: Option<&str>,
) -> Result<FrontmatterDiff> {
    let from_rev = from.unwrap_or("HEAD");
    let to_rev = to.unwrap_or("HEAD");
    let from_map = objects_at_rev(paths, from_rev)?;
    let to_map = objects_at_rev(paths, to_rev)?;
    let mut diff = FrontmatterDiff::default();
    for (id, to_status) in &to_map {
        match from_map.get(id) {
            None => diff.added.push(id.clone()),
            Some(from_status) if from_status != to_status => {
                diff.status_changes.push(ObjectStatusChange {
                    id: id.clone(),
                    from: Some(from_status.clone()),
                    to: Some(to_status.clone()),
                });
            }
            _ => {}
        }
    }
    for (id, from_status) in &from_map {
        if !to_map.contains_key(id) {
            diff.status_changes.push(ObjectStatusChange {
                id: id.clone(),
                from: Some(from_status.clone()),
                to: None,
            });
        }
    }
    diff.added.sort();
    diff.status_changes.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(diff)
}

fn objects_at_rev(
    paths: &ContextPaths,
    rev: &str,
) -> Result<std::collections::HashMap<String, String>> {
    let mut out = std::collections::HashMap::new();
    if !crate::is_git_repo(&paths.root) {
        return Ok(out);
    }
    let list = crate::run_git(
        &["ls-tree", "-r", "--name-only", rev, ".context/objects"],
        &paths.root,
    )
    .unwrap_or_default();
    for path in list.lines().filter(|line| line.ends_with(".md")) {
        let spec = format!("{rev}:{path}");
        let raw = crate::run_git(&["show", &spec], &paths.root).unwrap_or_default();
        if raw.trim().is_empty() {
            continue;
        }
        if let Ok(obj) = KnowledgeObject::parse(&raw, paths.root.join(path)) {
            out.insert(obj.frontmatter.id, obj.frontmatter.status);
        }
    }
    Ok(out)
}
