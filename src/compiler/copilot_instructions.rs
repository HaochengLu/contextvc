use super::managed::{block_hash, merge_into_file, wrap_managed};
use super::{resident_objects, CompileContext, TargetOutput};
use anyhow::Result;
use std::fs;

pub fn render(ctx: &CompileContext<'_>) -> Result<TargetOutput> {
    let residents = resident_objects(ctx.objects);
    let mut body = String::from("# GitHub Copilot Instructions (ContextVC)\n\n");
    body.push_str("Use these project-local rules before suggesting edits.\n\n");
    for obj in &residents {
        body.push_str(&format!("## {}\n\n{}\n\n", obj.frontmatter.title, obj.body));
    }
    let hash = block_hash(&body);
    let managed = wrap_managed("copilot-resident", &hash, body.trim());
    let path = ctx.paths.copilot_instructions();
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let content = merge_into_file(&existing, &managed);
    let ids = residents
        .iter()
        .map(|o| o.frontmatter.id.clone())
        .collect::<Vec<_>>();
    Ok(TargetOutput {
        target: "copilot_instructions".into(),
        path,
        content,
        object_ids: ids,
    })
}
