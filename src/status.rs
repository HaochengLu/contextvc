use crate::check;
use crate::object::{load_all_objects, load_proposals};
use crate::paths::ContextPaths;
use anyhow::Result;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct StatusReport {
    pub ocl_version: String,
    pub object_counts: ObjectCounts,
    pub proposals: usize,
    pub stale: usize,
    pub conflicted: usize,
    pub render_fresh: bool,
    pub events_shards: usize,
}

#[derive(Debug, Serialize)]
pub struct ObjectCounts {
    pub constraints: usize,
    pub decisions: usize,
    pub failures: usize,
    pub howtos: usize,
    pub codemap: usize,
    pub preferences: usize,
    pub total: usize,
}

pub fn status(paths: &ContextPaths) -> Result<StatusReport> {
    let objects = load_all_objects(&paths.objects)?;
    let proposals = load_proposals(&paths.proposals)?;
    let mut counts = ObjectCounts {
        constraints: 0,
        decisions: 0,
        failures: 0,
        howtos: 0,
        codemap: 0,
        preferences: 0,
        total: objects.len(),
    };
    let mut stale = 0;
    let mut conflicted = 0;
    for obj in &objects {
        match obj.frontmatter.status.as_str() {
            "stale" => stale += 1,
            "conflicted" => conflicted += 1,
            _ => {}
        }
        match obj.type_enum() {
            Some(crate::object::ObjectType::Constraint) => counts.constraints += 1,
            Some(crate::object::ObjectType::Decision) => counts.decisions += 1,
            Some(crate::object::ObjectType::Failure) => counts.failures += 1,
            Some(crate::object::ObjectType::Howto) => counts.howtos += 1,
            Some(crate::object::ObjectType::Codemap) => counts.codemap += 1,
            Some(crate::object::ObjectType::Preference) => counts.preferences += 1,
            None => {}
        }
    }

    let render_fresh = if paths.render_lock.exists() {
        check::check(paths).map(|r| r.ok).unwrap_or(false)
    } else {
        false
    };

    let events_shards = std::fs::read_dir(&paths.events)
        .map(|rd| rd.filter_map(Result::ok).count())
        .unwrap_or(0);

    let ocl_version = std::fs::read_to_string(&paths.version_file)
        .unwrap_or_default()
        .trim()
        .to_string();

    Ok(StatusReport {
        ocl_version,
        object_counts: counts,
        proposals: proposals.len(),
        stale,
        conflicted,
        render_fresh,
        events_shards,
    })
}

pub fn format_human(report: &StatusReport) -> String {
    format!(
        "ContextVC status (OCL v{})\n\
         Objects: {} total (c:{} d:{} f:{} h:{} m:{} p:{})\n\
         Proposals: {} | Stale: {} | Conflicted: {}\n\
         Render fresh: {}\n\
         Event shards: {}",
        report.ocl_version,
        report.object_counts.total,
        report.object_counts.constraints,
        report.object_counts.decisions,
        report.object_counts.failures,
        report.object_counts.howtos,
        report.object_counts.codemap,
        report.object_counts.preferences,
        report.proposals,
        report.stale,
        report.conflicted,
        if report.render_fresh { "yes" } else { "no" },
        report.events_shards,
    )
}
