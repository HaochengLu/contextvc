use crate::config::ProjectConfig;
use crate::paths::ContextPaths;
use crate::OCL_VERSION;
use anyhow::Result;
use serde::Serialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Serialize)]
pub struct DoctorReport {
    pub ok: bool,
    pub findings: Vec<DoctorFinding>,
    pub fixed: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct DoctorFinding {
    pub severity: String,
    pub check: String,
    pub message: String,
    pub suggested_action: String,
}

pub fn doctor(paths: &ContextPaths, fix: bool) -> Result<DoctorReport> {
    let mut report = DoctorReport {
        ok: true,
        findings: Vec::new(),
        fixed: Vec::new(),
    };

    ensure_or_report(&mut report, fix, &paths.context, ".context directory")?;
    ensure_or_report(&mut report, fix, &paths.objects, ".context/objects")?;
    ensure_or_report(&mut report, fix, &paths.events, ".context/events")?;
    ensure_or_report(&mut report, fix, &paths.proposals, ".context/proposals")?;
    ensure_or_report(&mut report, fix, &paths.cache, ".context/.cache")?;
    for ty in crate::object::ObjectType::all() {
        ensure_or_report(
            &mut report,
            fix,
            &paths.object_dir(ty.as_str()),
            &format!(".context/objects/{}", ty.as_str()),
        )?;
    }

    if !paths.version_file.exists() {
        if fix && paths.context.exists() {
            fs::write(&paths.version_file, format!("{OCL_VERSION}\n"))?;
            report.fixed.push("wrote .context/VERSION".into());
        } else {
            finding(
                &mut report,
                "error",
                "version",
                "missing .context/VERSION",
                "run `ctx init` or `ctx doctor --fix` in a partially initialized repo",
            );
        }
    }

    if !paths.config_file.exists() {
        if fix {
            ProjectConfig::default().save(&paths.config_file)?;
            report
                .fixed
                .push("wrote default .context/config.yaml".into());
        } else {
            finding(
                &mut report,
                "error",
                "config",
                "missing .context/config.yaml",
                "run `ctx doctor --fix`",
            );
        }
    } else if let Err(err) = ProjectConfig::load(&paths.config_file) {
        finding(
            &mut report,
            "error",
            "config",
            &format!("invalid config: {err:#}"),
            "fix .context/config.yaml",
        );
    }

    if paths.version_file.exists() {
        if !paths.render_lock.exists() {
            if fix {
                match crate::render::render_all(paths, true) {
                    Ok(_) => report
                        .fixed
                        .push("rendered projections and render.lock".into()),
                    Err(err) => finding(
                        &mut report,
                        "error",
                        "render",
                        &format!("render failed during repair: {err:#}"),
                        "resolve object/config issues, then run `ctx render --force`",
                    ),
                }
            } else {
                finding(
                    &mut report,
                    "error",
                    "render",
                    "missing .context/render.lock",
                    "run `ctx render --force`",
                );
            }
        } else {
            match crate::check::check(paths) {
                Ok(check) => {
                    for err in check.errors {
                        finding(
                            &mut report,
                            "error",
                            "check",
                            &err,
                            "run `ctx check` for detail",
                        );
                    }
                    for warn in check.warnings {
                        finding(&mut report, "warn", "check", &warn, "review the object");
                    }
                }
                Err(err) => finding(
                    &mut report,
                    "error",
                    "check",
                    &format!("ctx check crashed: {err:#}"),
                    "inspect invalid objects/events and rerun `ctx check`",
                ),
            }
        }
    }

    scan_for_secrets(paths, &mut report)?;
    check_hook_config(paths, &mut report);

    report.ok = !report.findings.iter().any(|f| f.severity == "error");
    Ok(report)
}

fn ensure_or_report(report: &mut DoctorReport, fix: bool, path: &Path, label: &str) -> Result<()> {
    if path.exists() {
        return Ok(());
    }
    if fix {
        fs::create_dir_all(path)?;
        report.fixed.push(format!("created {label}"));
    } else {
        finding(
            report,
            "error",
            "layout",
            &format!("missing {label}"),
            "run `ctx doctor --fix`",
        );
    }
    Ok(())
}

fn scan_for_secrets(paths: &ContextPaths, report: &mut DoctorReport) -> Result<()> {
    for root in [&paths.objects, &paths.events] {
        if !root.exists() {
            continue;
        }
        for entry in walkdir::WalkDir::new(root)
            .into_iter()
            .filter_map(|entry| entry.ok())
            .filter(|entry| entry.file_type().is_file())
        {
            let content = fs::read_to_string(entry.path()).unwrap_or_default();
            if contains_secret_like(&content) {
                finding(
                    report,
                    "error",
                    "secret-scan",
                    &format!("possible unredacted secret in {}", entry.path().display()),
                    "rotate the secret, redact the file, and recommit",
                );
            }
        }
    }
    Ok(())
}

fn check_hook_config(paths: &ContextPaths, report: &mut DoctorReport) {
    if !paths.root.join(".mcp.json").exists() {
        finding(
            report,
            "warn",
            "mcp",
            "missing .mcp.json",
            "run `ctx install mcp`",
        );
    }
    if crate::is_git_repo(&paths.root)
        && !paths
            .root
            .join(".git")
            .join("hooks")
            .join("pre-commit")
            .exists()
    {
        finding(
            report,
            "warn",
            "git-hook",
            "missing git pre-commit hook",
            "run `ctx install git`",
        );
    }
}

fn contains_secret_like(content: &str) -> bool {
    content.contains("ghp_")
        || content.contains("sk-")
        || content.contains("AKIA")
        || content.to_ascii_lowercase().contains("api_key=")
        || content.to_ascii_lowercase().contains("password=")
}

fn finding(
    report: &mut DoctorReport,
    severity: &str,
    check: &str,
    message: &str,
    suggested_action: &str,
) {
    report.findings.push(DoctorFinding {
        severity: severity.into(),
        check: check.into(),
        message: message.into(),
        suggested_action: suggested_action.into(),
    });
}
