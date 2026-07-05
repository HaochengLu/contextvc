use crate::object::{load_all_objects, KnowledgeObject};
use crate::paths::ContextPaths;
use anyhow::Result;
use sha2::{Digest, Sha256};

const MAX_BINDING_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, serde::Serialize)]
pub struct VerifyReport {
    pub checked: usize,
    pub stale: Vec<StaleObject>,
}

#[derive(Debug, serde::Serialize)]
pub struct StaleObject {
    pub id: String,
    pub title: String,
    pub reason: String,
}

pub fn verify(paths: &ContextPaths, mark: bool) -> Result<VerifyReport> {
    let _lock = if mark {
        Some(crate::lockfile::OperationLock::acquire(paths)?)
    } else {
        None
    };
    let objects = load_all_objects(&paths.objects)?;
    let mut stale = Vec::new();
    for obj in &objects {
        if obj.frontmatter.status != "active" {
            continue;
        }
        if let Some(reason) = check_bindings(paths, obj)? {
            stale.push(StaleObject {
                id: obj.frontmatter.id.clone(),
                title: obj.frontmatter.title.clone(),
                reason,
            });
            if mark {
                mark_stale(paths, obj)?;
            }
        }
    }
    Ok(VerifyReport {
        checked: objects.len(),
        stale,
    })
}

fn check_bindings(paths: &ContextPaths, obj: &KnowledgeObject) -> Result<Option<String>> {
    for binding in &obj.frontmatter.bindings {
        if binding.kind == "file" || binding.kind == "source" {
            if let Some(path) = &binding.path {
                let resolved = match resolve_binding_path(paths, path) {
                    Ok(resolved) => resolved,
                    Err(reason) => return Ok(Some(reason)),
                };
                let expected = binding.sha.as_ref().or(binding.hash.as_ref());
                if let Some(expected) = expected {
                    let content = if binding.kind == "source" {
                        source_binding_content(&resolved)?
                    } else {
                        std::fs::read(&resolved)?
                    };
                    let mut hasher = Sha256::new();
                    hasher.update(&content);
                    let actual = hex::encode(hasher.finalize())[..12].to_string();
                    if &actual != expected {
                        return Ok(Some(format!(
                            "file sha mismatch for {path}: expected {expected}, got {actual}"
                        )));
                    }
                }
            }
        }
    }
    Ok(None)
}

pub fn safe_file_sha(paths: &ContextPaths, path: &str) -> Result<String> {
    let resolved = resolve_binding_path(paths, path).map_err(|reason| anyhow::anyhow!(reason))?;
    let content = std::fs::read(&resolved)?;
    let mut hasher = Sha256::new();
    hasher.update(&content);
    Ok(hex::encode(hasher.finalize())[..12].to_string())
}

fn resolve_binding_path(
    paths: &ContextPaths,
    path: &str,
) -> std::result::Result<std::path::PathBuf, String> {
    let raw = std::path::Path::new(path);
    if raw.is_absolute() {
        return Err(format!("unsafe absolute binding path: {path}"));
    }
    if raw
        .components()
        .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(format!("unsafe traversal binding path: {path}"));
    }
    let candidate = paths.root.join(raw);
    if !candidate.exists() {
        return Err(format!("bound file missing: {path}"));
    }
    let root = paths
        .root
        .canonicalize()
        .map_err(|err| format!("cannot canonicalize repo root: {err}"))?;
    let resolved = candidate
        .canonicalize()
        .map_err(|err| format!("cannot canonicalize binding path {path}: {err}"))?;
    if !resolved.starts_with(&root) {
        return Err(format!("unsafe out-of-repo binding path: {path}"));
    }
    let metadata = std::fs::metadata(&resolved)
        .map_err(|err| format!("cannot stat binding path {path}: {err}"))?;
    if !metadata.is_file() {
        return Err(format!("binding path is not a regular file: {path}"));
    }
    if metadata.len() > MAX_BINDING_BYTES {
        return Err(format!("binding file too large to verify: {path}"));
    }
    Ok(resolved)
}

fn source_binding_content(path: &std::path::Path) -> Result<Vec<u8>> {
    let content = std::fs::read_to_string(path)?;
    let body_content = strip_yaml_frontmatter(&content)
        .map(|(_, body)| body)
        .unwrap_or(content.as_str());
    let (_, human) = crate::compiler::managed::split_managed(body_content);
    Ok(human.trim().as_bytes().to_vec())
}

fn strip_yaml_frontmatter(content: &str) -> Option<(&str, &str)> {
    let rest = content.strip_prefix("---")?;
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    let end = rest.find("\n---")?;
    let frontmatter = &rest[..end];
    let body = &rest[end + 4..];
    Some((frontmatter, body))
}

fn mark_stale(paths: &ContextPaths, obj: &KnowledgeObject) -> Result<()> {
    let rel = obj
        .path
        .strip_prefix(&paths.root)
        .unwrap_or(&obj.path)
        .to_path_buf();
    let full = if obj.path.is_absolute() {
        obj.path.clone()
    } else {
        paths.root.join(&obj.path)
    };
    let mut updated = KnowledgeObject::load(&full)?;
    updated.frontmatter.status = "stale".into();
    updated.save()?;
    let _ = rel;
    Ok(())
}
