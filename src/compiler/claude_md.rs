use super::managed::{block_hash, merge_into_file, wrap_managed};
use super::{resident_objects, CompileContext, TargetOutput};
use anyhow::Result;
use std::fs;

pub fn render(ctx: &CompileContext<'_>) -> Result<TargetOutput> {
    let residents = resident_objects(ctx.objects);
    let mut body = String::from("@AGENTS.md\n\n# Claude Project Context (ContextVC)\n\n");
    for obj in residents.iter().take(5) {
        body.push_str(&format!(
            "- **{}**: {}\n",
            obj.frontmatter.title,
            summarize(&obj.body)
        ));
    }
    let hash = block_hash(&body);
    let managed = wrap_managed("claude-resident", &hash, body.trim());
    let path = ctx.paths.claude_md();
    let existing = fs::read_to_string(&path).unwrap_or_default();
    let content = merge_into_file(&existing, &managed);
    let ids: Vec<_> = residents.iter().map(|o| o.frontmatter.id.clone()).collect();
    Ok(TargetOutput {
        target: "claude_md".into(),
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
        .take(120)
        .collect()
}
