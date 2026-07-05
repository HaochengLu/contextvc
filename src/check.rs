use crate::lockfile::RenderLock;
use crate::object::{load_all_objects, objects_digest};
use crate::paths::ContextPaths;
use anyhow::Result;

#[derive(Debug, Default, serde::Serialize)]
pub struct CheckReport {
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
}

pub fn check(paths: &ContextPaths) -> Result<CheckReport> {
    let mut report = CheckReport {
        ok: true,
        ..Default::default()
    };

    if !paths.version_file.exists() {
        report.errors.push("missing .context/VERSION".into());
        report.ok = false;
        return Ok(report);
    }

    let objects = load_all_objects(&paths.objects)?;
    let digest = objects_digest(&objects);

    for obj in &objects {
        for err in crate::object::validate_object(obj) {
            report.errors.push(format!("schema: {err}"));
            report.ok = false;
        }
    }

    if let Err(err) = crate::event::read_events(&paths.events) {
        report.errors.push(format!("invalid event ledger: {err:#}"));
        report.ok = false;
    }

    if paths.render_lock.exists() {
        let lock = RenderLock::load(&paths.render_lock)?;
        if lock.objects_digest != digest {
            report.errors.push(format!(
                "render.lock stale: objects changed (expected digest {digest}, lock has {})",
                lock.objects_digest
            ));
            report.ok = false;
        }
        for (target_key, target) in &lock.targets {
            let path = std::path::Path::new(&target.path);
            if !path.exists() {
                report
                    .errors
                    .push(format!("missing rendered file: {}", target.path));
                report.ok = false;
                continue;
            }
            let content = std::fs::read_to_string(path)?;
            let actual = if target_key.starts_with("cursor_mdc:") {
                crate::compiler::cursor_mdc::frontmatter_managed_digest(&content)
            } else {
                crate::compiler::managed::managed_digest(&content)
            };
            if actual != target.content_digest {
                report.errors.push(format!(
                    "content drift in {} (run `ctx render` or `ctx adopt`)",
                    target.path
                ));
                report.ok = false;
            }
        }
    } else {
        report
            .errors
            .push("render.lock missing — run `ctx render`".into());
        report.ok = false;
    }

    for obj in &objects {
        if obj.frontmatter.evidence.is_empty()
            && obj.frontmatter.trust.starts_with("agent")
            && obj.frontmatter.status == "active"
            && matches!(
                obj.type_enum(),
                Some(crate::object::ObjectType::Failure | crate::object::ObjectType::Decision)
            )
        {
            report.warnings.push(format!(
                "object {} active without evidence",
                obj.frontmatter.id
            ));
        }
        if obj.frontmatter.status == "conflicted" {
            if obj.type_enum() == Some(crate::object::ObjectType::Constraint) {
                report.errors.push(format!(
                    "conflicted constraint blocks render: {} ({})",
                    obj.frontmatter.id, obj.frontmatter.title
                ));
                report.ok = false;
            } else {
                report
                    .warnings
                    .push(format!("conflicted object: {}", obj.frontmatter.id));
            }
        }
        if obj.frontmatter.status == "stale" {
            report.errors.push(format!(
                "stale object: {} ({})",
                obj.frontmatter.id, obj.frontmatter.title
            ));
            report.ok = false;
        }
    }

    let verify_report = crate::verify::verify(paths, false)?;
    for stale in verify_report.stale {
        report
            .errors
            .push(format!("stale binding in {}: {}", stale.id, stale.reason));
        report.ok = false;
    }

    Ok(report)
}
