use super::managed::{block_hash, merge_into_file, wrap_managed};
use super::{scoped_objects, CompileContext, TargetOutput};
use anyhow::Result;
use std::fs;

pub fn render(ctx: &CompileContext<'_>) -> Result<Vec<TargetOutput>> {
    let scoped = scoped_objects(ctx.objects);
    let rules_dir = ctx.paths.cursor_rules_dir();
    fs::create_dir_all(&rules_dir)?;

    let mut by_scope: std::collections::HashMap<String, Vec<_>> = std::collections::HashMap::new();
    for obj in scoped {
        let scopes = if obj.frontmatter.scope.is_empty() {
            vec!["**".into()]
        } else {
            obj.frontmatter.scope.clone()
        };
        by_scope.entry(scopes.join(",")).or_default().push(obj);
    }

    let mut outputs = Vec::new();
    for (scope, objects) in by_scope {
        let always = false;
        let mut body = String::new();
        for obj in &objects {
            body.push_str(&format!("## {}\n\n{}\n\n", obj.frontmatter.title, obj.body));
        }
        let hash = block_hash(&body);
        let managed_body = wrap_managed(&format!("cursor-{hash}"), &hash, body.trim());
        let filename = format!("ctx-{}.mdc", sanitize_scope(&scope));
        let frontmatter = format!(
            "---\ndescription: ContextVC scoped rules for {scope}\nalwaysApply: {always}\nglobs: {scope}\n---\n\n"
        );
        let path = rules_dir.join(&filename);
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let content = merge_cursor_content(&existing, &frontmatter, &managed_body);
        let ids: Vec<_> = objects.iter().map(|o| o.frontmatter.id.clone()).collect();
        outputs.push(TargetOutput {
            target: "cursor_mdc".into(),
            path,
            content,
            object_ids: ids,
        });
    }

    if outputs.is_empty() {
        let path = rules_dir.join("ctx-default.mdc");
        let body = "No scoped objects yet. Add howtos/codemap under `.context/objects/`.";
        let hash = block_hash(body);
        let managed = wrap_managed("cursor-default", &hash, body);
        let generated = format!(
            "---\ndescription: ContextVC default rules\nalwaysApply: false\nglobs: **/*\n---\n\n{managed}"
        );
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let frontmatter =
            "---\ndescription: ContextVC default rules\nalwaysApply: false\nglobs: **/*\n---\n\n";
        let content = if existing.trim().is_empty() {
            generated
        } else {
            merge_cursor_content(&existing, frontmatter, &managed)
        };
        outputs.push(TargetOutput {
            target: "cursor_mdc".into(),
            path,
            content,
            object_ids: vec![],
        });
    }

    Ok(outputs)
}

pub fn frontmatter_managed_digest(content: &str) -> String {
    let (frontmatter, body) = split_yaml_frontmatter(content).unwrap_or(("", content));
    let (managed, _) = super::managed::split_managed(body);
    crate::lockfile::digest_content(&format!("{}\n{}", frontmatter.trim(), managed.trim()))
}

fn merge_cursor_content(existing: &str, frontmatter: &str, managed: &str) -> String {
    if existing.trim().is_empty() {
        return format!("{frontmatter}{managed}");
    }
    let (_, body) = split_yaml_frontmatter(existing).unwrap_or(("", existing));
    let merged_body = if body.trim().is_empty() {
        managed.to_string()
    } else {
        merge_into_file(body, managed)
    };
    format!(
        "{frontmatter}{}",
        merged_body.trim_start_matches(['\r', '\n'])
    )
}

fn split_yaml_frontmatter(content: &str) -> Option<(&str, &str)> {
    content.strip_prefix("---")?;
    let end = content.find("\n---")? + 4;
    let frontmatter = &content[..end];
    let body = &content[end..];
    Some((frontmatter, body))
}

fn sanitize_scope(scope: &str) -> String {
    scope
        .replace("**", "all")
        .replace('/', "-")
        .replace('*', "x")
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '.')
        .collect()
}
