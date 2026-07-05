use crate::config::ProjectConfig;
use crate::event::{append_event, attach_git_context, ContextEvent};
use crate::object::{load_all_objects, load_proposals, KnowledgeObject};
use crate::paths::ContextPaths;
use anyhow::Result;
use serde_json::json;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub fn review_list(paths: &ContextPaths) -> Result<Vec<KnowledgeObject>> {
    load_proposals(&paths.proposals)
}

pub fn review_accept(paths: &ContextPaths, id: &str) -> Result<KnowledgeObject> {
    let _lock = crate::lockfile::OperationLock::acquire(paths)?;
    let preflight = crate::check::check(paths)?;
    if !preflight.ok {
        anyhow::bail!(
            "cannot accept proposal while ctx check is failing: {}",
            preflight.errors.join("; ")
        );
    }

    let proposals = load_proposals(&paths.proposals)?;
    let proposal = proposals
        .into_iter()
        .find(|p| p.frontmatter.id == id)
        .ok_or_else(|| anyhow::anyhow!("proposal not found: {id}"))?;
    let proposal_path = proposal.path.clone();

    let dest_dir = paths.object_dir(proposal.type_enum().unwrap().as_str());
    fs::create_dir_all(&dest_dir)?;
    let filename = proposal
        .path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("invalid proposal path"))?
        .to_owned();
    let dest = dest_dir.join(filename);

    let mut activated = proposal;
    activated.frontmatter.status = "active".into();
    activated.path = dest.clone();
    if dest.exists() {
        let existing = KnowledgeObject::load(&dest)?;
        if existing.frontmatter.id != activated.frontmatter.id {
            anyhow::bail!(
                "destination object already exists with a different id: {}",
                dest.display()
            );
        }
    }
    let previous_dest = fs::read(&dest).ok();
    activated.save()?;
    let render_snapshots = match snapshot_render_outputs(paths) {
        Ok(snapshots) => snapshots,
        Err(err) => {
            restore_accepted_object(&dest, previous_dest.as_deref())?;
            anyhow::bail!("accepted object rolled back before render: {err}");
        }
    };

    if let Err(err) = crate::render::render_all_unlocked(paths, true) {
        restore_snapshots(&render_snapshots)?;
        if let Some(previous) = previous_dest {
            fs::write(&dest, previous)?;
        } else if dest.exists() {
            fs::remove_file(&dest)?;
        }
        anyhow::bail!("accepted object rolled back after render failure: {err}");
    }

    if proposal_path.exists() && proposal_path != dest {
        fs::remove_file(&proposal_path)?;
    }
    Ok(activated)
}

#[derive(Debug)]
struct PathSnapshot {
    path: PathBuf,
    state: SnapshotState,
}

#[derive(Debug)]
enum SnapshotState {
    File(Vec<u8>),
    Directory,
    Missing,
}

fn snapshot_render_outputs(paths: &ContextPaths) -> Result<Vec<PathSnapshot>> {
    let config = ProjectConfig::load(&paths.config_file)?;
    let objects = load_all_objects(&paths.objects)?;
    let ctx = crate::compiler::CompileContext {
        paths,
        config: &config,
        objects: &objects,
    };
    let outputs = crate::compiler::render_targets(&ctx)?;
    let mut seen = HashSet::new();
    let mut snapshots = Vec::new();
    for path in outputs
        .into_iter()
        .map(|out| out.path)
        .chain(std::iter::once(paths.render_lock.clone()))
    {
        if seen.insert(path.clone()) {
            snapshots.push(snapshot_path(path)?);
        }
    }
    Ok(snapshots)
}

fn snapshot_path(path: PathBuf) -> Result<PathSnapshot> {
    let state = if path.is_file() {
        SnapshotState::File(fs::read(&path)?)
    } else if path.is_dir() {
        SnapshotState::Directory
    } else {
        SnapshotState::Missing
    };
    Ok(PathSnapshot { path, state })
}

fn restore_snapshots(snapshots: &[PathSnapshot]) -> Result<()> {
    for snapshot in snapshots {
        match &snapshot.state {
            SnapshotState::File(bytes) => {
                if snapshot.path.is_dir() {
                    fs::remove_dir_all(&snapshot.path)?;
                }
                if let Some(parent) = snapshot.path.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::write(&snapshot.path, bytes)?;
            }
            SnapshotState::Directory => {
                if snapshot.path.is_file() {
                    fs::remove_file(&snapshot.path)?;
                }
                fs::create_dir_all(&snapshot.path)?;
            }
            SnapshotState::Missing => {
                remove_path_if_exists(&snapshot.path)?;
            }
        }
    }
    Ok(())
}

fn restore_accepted_object(dest: &Path, previous: Option<&[u8]>) -> Result<()> {
    if let Some(previous) = previous {
        fs::write(dest, previous)?;
    } else {
        remove_path_if_exists(dest)?;
    }
    Ok(())
}

fn remove_path_if_exists(path: &Path) -> Result<()> {
    if path.is_dir() {
        fs::remove_dir_all(path)?;
    } else if path.exists() {
        fs::remove_file(path)?;
    }
    Ok(())
}

pub fn review_reject(paths: &ContextPaths, id: &str) -> Result<()> {
    let _lock = crate::lockfile::OperationLock::acquire(paths)?;
    let proposals = load_proposals(&paths.proposals)?;
    if let Some(proposal) = proposals.into_iter().find(|p| p.frontmatter.id == id) {
        let event = ContextEvent::new(
            "distill_rejected",
            "ctx-review",
            json!({
                "proposal_id": id,
                "evidence": proposal.frontmatter.evidence.clone(),
            }),
        );
        let event = attach_git_context(event, &paths.root);
        append_event(paths, &event)?;
        fs::remove_file(&proposal.path)?;
    }
    Ok(())
}

pub fn format_review_queue(proposals: &[KnowledgeObject]) -> String {
    if proposals.is_empty() {
        return "No pending proposals.".into();
    }
    let mut out = String::from("Pending proposals:\n");
    for p in proposals {
        out.push_str(&format!(
            "  [{}] {} — {} ({})\n",
            p.frontmatter.id, p.frontmatter.object_type, p.frontmatter.title, p.frontmatter.status
        ));
    }
    out.push_str("\nUse: ctx review accept <id> | ctx review reject <id>\n");
    out
}
