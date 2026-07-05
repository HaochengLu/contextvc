use crate::compiler::{self, CompileContext};
use crate::config::ProjectConfig;
use crate::lockfile::RenderLock;
use crate::object::{load_all_objects, objects_digest};
use crate::paths::ContextPaths;
use crate::{ensure_repo, OCL_VERSION};
use anyhow::Result;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

pub fn render_all(paths: &ContextPaths, force: bool) -> Result<RenderLock> {
    let _lock = crate::lockfile::OperationLock::acquire(paths)?;
    render_all_unlocked(paths, force)
}

pub(crate) fn render_all_unlocked(paths: &ContextPaths, force: bool) -> Result<RenderLock> {
    let config = ProjectConfig::load(&paths.config_file)?;
    let objects = load_all_objects(&paths.objects)?;
    if let Some(obj) = objects.iter().find(|obj| {
        obj.frontmatter.status == "conflicted"
            && obj.type_enum() == Some(crate::object::ObjectType::Constraint)
    }) {
        anyhow::bail!(
            "cannot render with conflicted constraint {} ({})",
            obj.frontmatter.id,
            obj.frontmatter.title
        );
    }
    let ctx = CompileContext {
        paths,
        config: &config,
        objects: &objects,
    };
    let outputs = compiler::render_targets(&ctx)?;

    if !force {
        for out in &outputs {
            if out.path.exists() {
                let existing = fs::read_to_string(&out.path)?;
                if has_drift(&existing, out) {
                    anyhow::bail!(
                        "drift detected in {} — run `ctx adopt` or `ctx render --force`",
                        out.path.display()
                    );
                }
            }
        }
    }

    let mut targets = HashMap::new();
    for out in outputs {
        if let Some(parent) = out.path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&out.path, &out.content)?;
        targets.insert(
            out.target.clone() + ":" + out.path.file_name().unwrap().to_str().unwrap_or(""),
            crate::lockfile::TargetLock {
                path: out.path.to_string_lossy().into_owned(),
                content_digest: target_content_digest(&out),
                object_ids: out.object_ids,
            },
        );
    }

    let lock = RenderLock {
        ocl_version: OCL_VERSION.into(),
        objects_digest: objects_digest(&objects),
        targets,
    };
    lock.save(&paths.render_lock)?;

    let _ = crate::index::rebuild_index(paths);
    Ok(lock)
}

pub fn render_repo(root: &Path, force: bool) -> Result<RenderLock> {
    let paths = ensure_repo(root)?;
    render_all(&paths, force)
}

fn has_drift(existing: &str, out: &compiler::TargetOutput) -> bool {
    let (managed, _) = crate::compiler::managed::split_managed(existing);
    if managed.trim().is_empty() {
        return false;
    }
    let new_managed = crate::compiler::managed::split_managed(&out.content).0;
    managed.trim() != new_managed.trim()
}

fn target_content_digest(out: &compiler::TargetOutput) -> String {
    if out.target == "cursor_mdc" {
        crate::compiler::cursor_mdc::frontmatter_managed_digest(&out.content)
    } else {
        crate::compiler::managed::managed_digest(&out.content)
    }
}
