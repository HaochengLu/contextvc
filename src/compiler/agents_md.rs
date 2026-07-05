use super::managed::{block_hash, merge_into_file, wrap_managed};
use super::{resident_objects, CompileContext, TargetOutput};
use anyhow::Result;
use std::fs;

pub fn render(ctx: &CompileContext<'_>) -> Result<TargetOutput> {
    let residents = resident_objects(ctx.objects);
    let mut body = String::from("# Agent Instructions (ContextVC)\n\n");
    body.push_str("> Managed by ContextVC. Edit `.context/objects/` or run `ctx review`.\n\n");
    for obj in &residents {
        body.push_str(&format!("## {}\n\n{}\n\n", obj.frontmatter.title, obj.body));
    }
    let hash = block_hash(&body);
    let managed = wrap_managed("agents-resident", &hash, body.trim());
    let path = ctx.paths.agents_md();
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let content = merge_into_file(&existing, &managed);
    let ids: Vec<_> = residents.iter().map(|o| o.frontmatter.id.clone()).collect();
    Ok(TargetOutput {
        target: "agents_md".into(),
        path,
        content,
        object_ids: ids,
    })
}
