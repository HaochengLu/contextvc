use crate::object::{new_object, slugify, ObjectType};
use crate::paths::ContextPaths;
use crate::{is_git_repo, run_git};
use anyhow::Result;
use std::collections::HashMap;
use std::fs;

pub fn backfill(paths: &ContextPaths) -> Result<usize> {
    if !is_git_repo(&paths.root) {
        return Ok(0);
    }
    if run_git(&["rev-parse", "--verify", "HEAD"], &paths.root).is_err() {
        return Ok(0);
    }

    let log = run_git(
        &[
            "log",
            "--pretty=format:",
            "--name-only",
            "--diff-filter=AM",
            "-n",
            "200",
        ],
        &paths.root,
    )?;
    let mut counts: HashMap<String, usize> = HashMap::new();
    for line in log.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('.') {
            continue;
        }
        *counts.entry(line.to_string()).or_default() += 1;
    }

    let mut created = 0;
    let mut ranked: Vec<_> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1));
    for (path, count) in ranked.into_iter().take(15) {
        if count < 2 {
            continue;
        }
        let title = format!("High churn: {path}");
        let body = format!(
            "## Summary\nFile `{path}` changed {count} times in recent git history.\n\n## Notes\nAuto-generated codemap entry from `ctx backfill`."
        );
        let mut obj = new_object(
            ObjectType::Codemap,
            &title,
            &body,
            vec![path.clone()],
            "active",
        );
        obj.frontmatter.trust = "agent_auto".into();
        obj.frontmatter.confidence = 0.6;
        obj.path = paths.object_dir("codemap").join(format!(
            "{}-{}.md",
            slugify(&path),
            &obj.frontmatter.id[2..]
        ));
        if obj.path.exists() {
            continue;
        }
        fs::create_dir_all(obj.path.parent().unwrap())?;
        obj.save()?;
        created += 1;
    }
    Ok(created)
}
