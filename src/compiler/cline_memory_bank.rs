use super::managed::{block_hash, merge_into_file, wrap_managed};
use super::{projectable_objects, CompileContext, TargetOutput};
use anyhow::Result;
use std::fs;

pub fn render(ctx: &CompileContext<'_>) -> Result<TargetOutput> {
    let objects = projectable_objects(ctx.objects);
    let mut body = String::from("# Cline Memory Bank (ContextVC)\n\n");
    for obj in &objects {
        body.push_str(&format!(
            "## {} ({})\n\nScopes: {}\n\n{}\n\n",
            obj.frontmatter.title,
            obj.frontmatter.object_type,
            if obj.frontmatter.scope.is_empty() {
                "**".to_string()
            } else {
                obj.frontmatter.scope.join(", ")
            },
            obj.body
        ));
    }
    let hash = block_hash(&body);
    let managed = wrap_managed("cline-memory-bank", &hash, body.trim());
    let path = ctx.paths.cline_memory_bank();
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let content = merge_into_file(&existing, &managed);
    let ids = objects
        .iter()
        .map(|o| o.frontmatter.id.clone())
        .collect::<Vec<_>>();
    Ok(TargetOutput {
        target: "cline_memory_bank".into(),
        path,
        content,
        object_ids: ids,
    })
}
