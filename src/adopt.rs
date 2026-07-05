use crate::object::{
    hash_id, load_all_objects, new_object, slugify, Binding, KnowledgeObject, ObjectType,
};
use crate::paths::ContextPaths;
use anyhow::Result;
use std::fs;
use std::path::Path;

pub fn adopt_existing(paths: &ContextPaths) -> Result<usize> {
    let _lock = crate::lockfile::OperationLock::acquire(paths)?;
    let mut count = 0;
    if paths.agents_md().exists() {
        count += adopt_file(
            paths,
            &paths.agents_md(),
            ObjectType::Constraint,
            vec!["**".into()],
        )?;
    }
    if paths.claude_md().exists() {
        count += adopt_file(
            paths,
            &paths.claude_md(),
            ObjectType::Preference,
            vec!["**".into()],
        )?;
    }
    if paths.cursor_rules_dir().exists() {
        for entry in fs::read_dir(&paths.cursor_rules_dir())? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "mdc") {
                let scope = cursor_mdc_scope(&path)?.unwrap_or_else(|| vec!["**".into()]);
                count += adopt_file(paths, &path, ObjectType::Howto, scope)?;
            }
        }
    }
    Ok(count)
}

fn adopt_file(
    paths: &ContextPaths,
    source: &Path,
    object_type: ObjectType,
    scope: Vec<String>,
) -> Result<usize> {
    validate_in_repo_regular_file(paths, source)?;
    let content = fs::read_to_string(source)?;
    let body_content = strip_yaml_frontmatter(&content)
        .map(|(_, body)| body)
        .unwrap_or(content.as_str());
    let (_, human) = split_managed(body_content);
    let body_source = human.trim();
    if body_source.is_empty() {
        return Ok(0);
    }

    let source_key = source_key(paths, source);
    let title = format!("Adopted from {source_key}");
    let source_binding = source_binding(&source_key, body_source);

    if let Some(mut existing) = existing_adopted_object(paths, object_type, &source_key)? {
        let before = existing.to_markdown();
        existing.frontmatter.title = title;
        existing.frontmatter.scope = scope;
        existing.frontmatter.status = "active".into();
        existing.body = body_source.to_string();
        upsert_source_binding(&mut existing, source_binding);
        if existing.to_markdown() != before {
            existing.save()?;
            return Ok(1);
        }
        return Ok(0);
    }

    let mut obj = new_object(object_type, &title, body_source, scope, "active");
    obj.frontmatter.id = hash_id(object_type, &format!("adopt:{source_key}"));
    obj.frontmatter.bindings = vec![source_binding];
    obj.path = paths.object_dir(object_type.as_str()).join(format!(
        "{}-{}.md",
        slugify(&title),
        &obj.frontmatter.id[2..]
    ));
    fs::create_dir_all(obj.path.parent().unwrap())?;
    obj.save()?;
    Ok(1)
}

pub fn adopt_drift(paths: &ContextPaths, target: &Path) -> Result<Option<KnowledgeObject>> {
    let _lock = crate::lockfile::OperationLock::acquire(paths)?;
    if !target.exists() {
        return Ok(None);
    }
    validate_in_repo_regular_file(paths, target)?;
    let content = fs::read_to_string(target)?;
    let (managed, _) = split_managed(&content);
    if managed.trim().is_empty() {
        return Ok(None);
    }
    let obj = new_object(
        ObjectType::Preference,
        &format!("Drift from {}", target.display()),
        managed.trim(),
        vec!["**".into()],
        "proposed",
    );
    let dest = paths
        .proposals
        .join("preferences")
        .join(format!("drift-{}.md", &obj.frontmatter.id[2..]));
    fs::create_dir_all(dest.parent().unwrap())?;
    let mut saved = obj;
    saved.path = dest;
    saved.save()?;
    Ok(Some(saved))
}

use crate::compiler::managed::split_managed;

fn source_key(paths: &ContextPaths, source: &Path) -> String {
    let rel = source.strip_prefix(&paths.root).unwrap_or(source);
    rel.to_string_lossy().replace('\\', "/")
}

fn validate_in_repo_regular_file(paths: &ContextPaths, source: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(source)?;
    if metadata.file_type().is_symlink() {
        anyhow::bail!("refusing to adopt symlink source: {}", source.display());
    }
    if !metadata.is_file() {
        anyhow::bail!("refusing to adopt non-file source: {}", source.display());
    }
    let root = paths.root.canonicalize()?;
    let source = source.canonicalize()?;
    if !source.starts_with(&root) {
        anyhow::bail!("refusing to adopt out-of-repo source: {}", source.display());
    }
    Ok(())
}

fn source_binding(source_key: &str, body: &str) -> Binding {
    Binding {
        kind: "source".into(),
        path: Some(source_key.into()),
        name: None,
        sha: None,
        hash: Some(crate::lockfile::digest_content(body)[..12].to_string()),
        pattern: None,
        enforcement: None,
    }
}

fn existing_adopted_object(
    paths: &ContextPaths,
    object_type: ObjectType,
    source_key: &str,
) -> Result<Option<KnowledgeObject>> {
    Ok(load_all_objects(&paths.objects)?.into_iter().find(|obj| {
        obj.type_enum() == Some(object_type)
            && obj.frontmatter.bindings.iter().any(|binding| {
                binding.kind == "source" && binding.path.as_deref() == Some(source_key)
            })
    }))
}

fn upsert_source_binding(obj: &mut KnowledgeObject, binding: Binding) {
    obj.frontmatter.bindings.retain(|existing| {
        !(existing.kind == "source" && existing.path.as_deref() == binding.path.as_deref())
    });
    obj.frontmatter.bindings.push(binding);
}

fn cursor_mdc_scope(path: &Path) -> Result<Option<Vec<String>>> {
    let content = fs::read_to_string(path)?;
    let Some((frontmatter, _)) = strip_yaml_frontmatter(&content) else {
        return Ok(None);
    };
    if let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(frontmatter) {
        if let Some(globs) = value.get("globs").and_then(globs_from_yaml_value) {
            return Ok(Some(globs));
        }
    }
    Ok(globs_from_frontmatter_line(frontmatter))
}

fn globs_from_yaml_value(value: &serde_yaml::Value) -> Option<Vec<String>> {
    match value {
        serde_yaml::Value::String(s) => Some(split_globs(s)),
        serde_yaml::Value::Sequence(items) => {
            let globs: Vec<_> = items
                .iter()
                .filter_map(|item| item.as_str())
                .flat_map(split_globs)
                .collect();
            (!globs.is_empty()).then_some(globs)
        }
        _ => None,
    }
}

fn globs_from_frontmatter_line(frontmatter: &str) -> Option<Vec<String>> {
    for line in frontmatter.lines() {
        let line = line.trim();
        if let Some(raw) = line.strip_prefix("globs:") {
            let raw = raw
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .trim_matches('[')
                .trim_matches(']');
            let globs = split_globs(raw);
            if !globs.is_empty() {
                return Some(globs);
            }
        }
    }
    None
}

fn split_globs(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(|part| part.trim().trim_matches('"').trim_matches('\''))
        .filter(|part| !part.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn strip_yaml_frontmatter(content: &str) -> Option<(&str, &str)> {
    let rest = content.strip_prefix("---")?;
    let rest = rest.strip_prefix('\n').unwrap_or(rest);
    let end = rest.find("\n---")?;
    let frontmatter = &rest[..end];
    let body = &rest[end + 4..];
    Some((frontmatter, body))
}
