pub mod agents_md;
pub mod claude_md;
pub mod cline_memory_bank;
pub mod copilot_instructions;
pub mod cursor_mdc;
pub mod gemini_md;
pub mod managed;

use crate::config::ProjectConfig;
use crate::object::{KnowledgeObject, ObjectType};
use crate::paths::ContextPaths;
use anyhow::Result;

pub struct CompileContext<'a> {
    pub paths: &'a ContextPaths,
    pub config: &'a ProjectConfig,
    pub objects: &'a [KnowledgeObject],
}

pub fn projectable_objects(objects: &[KnowledgeObject]) -> Vec<&KnowledgeObject> {
    let mut out: Vec<_> = objects.iter().filter(|o| o.is_projectable()).collect();
    out.sort_by(|a, b| {
        let wa = a.type_enum().map(|t| t.projection_weight()).unwrap_or(0);
        let wb = b.type_enum().map(|t| t.projection_weight()).unwrap_or(0);
        wb.cmp(&wa)
            .then_with(|| a.frontmatter.title.cmp(&b.frontmatter.title))
    });
    out
}

pub fn resident_objects<'a>(objects: &'a [KnowledgeObject]) -> Vec<&'a KnowledgeObject> {
    projectable_objects(objects)
        .into_iter()
        .filter(|o| {
            matches!(
                o.type_enum(),
                Some(ObjectType::Constraint | ObjectType::Decision | ObjectType::Preference)
            )
        })
        .take(30)
        .collect()
}

pub fn scoped_objects<'a>(objects: &'a [KnowledgeObject]) -> Vec<&'a KnowledgeObject> {
    projectable_objects(objects)
        .into_iter()
        .filter(|o| matches!(o.type_enum(), Some(ObjectType::Howto | ObjectType::Codemap)))
        .collect()
}

pub fn render_targets(ctx: &CompileContext<'_>) -> Result<Vec<TargetOutput>> {
    let mut outputs = Vec::new();
    for target in &ctx.config.targets {
        match target.as_str() {
            "agents_md" => outputs.push(agents_md::render(ctx)?),
            "claude_md" => outputs.push(claude_md::render(ctx)?),
            "cursor_mdc" => outputs.extend(cursor_mdc::render(ctx)?),
            "copilot_instructions" => outputs.push(copilot_instructions::render(ctx)?),
            "gemini_md" => outputs.push(gemini_md::render(ctx)?),
            "cline_memory_bank" => outputs.push(cline_memory_bank::render(ctx)?),
            other => anyhow::bail!("unknown render target: {other}"),
        }
    }
    Ok(outputs)
}

#[derive(Debug, Clone)]
pub struct TargetOutput {
    pub target: String,
    pub path: std::path::PathBuf,
    pub content: String,
    pub object_ids: Vec<String>,
}
