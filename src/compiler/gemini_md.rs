use super::managed::{block_hash, merge_into_file, wrap_managed};
use super::{resident_objects, CompileContext, TargetOutput};
use anyhow::Result;
use std::fs;

pub fn render(ctx: &CompileContext<'_>) -> Result<TargetOutput> {
    let residents = resident_objects(ctx.objects);
    let mut body = String::from("# Gemini Project Context (ContextVC)\n\n");
    for obj in &residents {
        body.push_str(&format!(
            "- **{}** [{}]: {}\n",
            obj.frontmatter.title,
            obj.frontmatter.object_type,
            summarize(&obj.body)
        ));
    }
    let hash = block_hash(&body);
    let managed = wrap_managed("gemini-resident", &hash, body.trim());
    let path = ctx.paths.gemini_md();
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let content = merge_into_file(&existing, &managed);
    let ids = residents
        .iter()
        .map(|o| o.frontmatter.id.clone())
        .collect::<Vec<_>>();
    Ok(TargetOutput {
        target: "gemini_md".into(),
        path,
        content,
        object_ids: ids,
    })
}

fn summarize(body: &str) -> String {
    body.lines()
        .find(|l| !l.trim().is_empty() && !l.starts_with('#'))
        .unwrap_or("")
        .chars()
        .take(160)
        .collect()
}
