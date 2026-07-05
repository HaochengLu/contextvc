use crate::paths::ContextPaths;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Deserialize)]
pub struct Scenario {
    #[serde(default = "default_version")]
    pub version: u32,
    pub id: String,
    pub fixture: PathBuf,
    #[serde(default)]
    pub task: Option<String>,
    pub expected_gate_hit: String,
    #[serde(default = "default_expected_permission")]
    pub expected_permission: String,
    pub precheck: PrecheckSpec,
    #[serde(default)]
    pub forbidden_actions: Vec<ForbiddenAction>,
    #[serde(default)]
    pub false_positive_actions: Vec<ForbiddenAction>,
}

#[derive(Debug, Deserialize)]
pub struct PrecheckSpec {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct ForbiddenAction {
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub pattern: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RepeatBenchReport {
    pub scenarios: usize,
    pub gate_hits: usize,
    pub misses: usize,
    pub repeat_failure_rate: f64,
    pub ttc_ms: u128,
    pub results: Vec<ScenarioResult>,
}

#[derive(Debug, Serialize)]
pub struct ScenarioResult {
    pub id: String,
    pub passed: bool,
    pub expected_gate_hit: String,
    pub expected_permission: String,
    pub hook_permission: String,
    pub actual_hit_ids: Vec<String>,
    pub forbidden_actions_caught: usize,
    pub false_positive_misses: usize,
    pub elapsed_ms: u128,
}

pub fn run(root: &Path, scenarios_dir: &Path) -> Result<RepeatBenchReport> {
    run_with_output(root, scenarios_dir, None)
}

pub fn run_with_output(
    root: &Path,
    scenarios_dir: &Path,
    output_jsonl: Option<&Path>,
) -> Result<RepeatBenchReport> {
    let started = Instant::now();
    let mut results = Vec::new();
    for scenario_path in scenario_files(scenarios_dir)? {
        let scenario = load_scenario(&scenario_path)?;
        if scenario.version != 1 {
            anyhow::bail!(
                "unsupported RepeatBench scenario version {} in {}",
                scenario.version,
                scenario_path.display()
            );
        }
        results.push(run_scenario(root, &scenario_path, &scenario)?);
    }
    let gate_hits = results.iter().filter(|r| r.passed).count();
    let scenarios = results.len();
    let misses = scenarios.saturating_sub(gate_hits);
    let report = RepeatBenchReport {
        scenarios,
        gate_hits,
        misses,
        repeat_failure_rate: if scenarios == 0 {
            0.0
        } else {
            misses as f64 / scenarios as f64
        },
        ttc_ms: started.elapsed().as_millis(),
        results,
    };
    if let Some(output) = output_jsonl {
        write_results_jsonl(output, &report)?;
    }
    Ok(report)
}

fn run_scenario(root: &Path, scenario_path: &Path, scenario: &Scenario) -> Result<ScenarioResult> {
    let started = Instant::now();
    let fixture = if scenario.fixture.is_absolute() {
        scenario.fixture.clone()
    } else {
        scenario_path
            .parent()
            .unwrap_or(root)
            .join(&scenario.fixture)
    };
    let work = std::env::temp_dir().join(format!(
        "ctx-repeatbench-{}-{}",
        scenario.id,
        ulid::Ulid::new()
    ));
    copy_dir(&fixture, &work).with_context(|| {
        format!(
            "failed to copy fixture {} for scenario {}",
            fixture.display(),
            scenario.id
        )
    })?;

    if !crate::is_git_repo(&work) {
        let _ = std::process::Command::new("git")
            .args(["init"])
            .current_dir(&work)
            .output();
    }

    let paths = if ContextPaths::new(&work).version_file.exists() {
        crate::ensure_repo(&work)?
    } else {
        crate::init::init_repo(
            &work,
            crate::init::InitOptions {
                skip_adopt: true,
                skip_backfill: true,
                skip_render: true,
            },
        )?
    };
    crate::render::render_all(&paths, true)?;

    let result = crate::gate::precheck(
        &paths,
        scenario.precheck.path.as_deref(),
        scenario.precheck.command.as_deref(),
    )?;
    let hook_payload = if let Some(command) = &scenario.precheck.command {
        serde_json::json!({ "command": command })
    } else if let Some(path) = &scenario.precheck.path {
        serde_json::json!({
            "tool_name": "Edit",
            "tool_input": { "file_path": path }
        })
    } else {
        serde_json::json!({})
    };
    let hook = crate::distill::handle_pre_tool(&paths, &hook_payload.to_string())?;
    let hook_permission = hook
        .get("permission")
        .or_else(|| hook.get("permissionDecision"))
        .and_then(|v| v.as_str())
        .unwrap_or("allow")
        .to_string();
    let mut actual_hit_ids = result
        .hits
        .iter()
        .map(|hit| hit.id.clone())
        .collect::<Vec<_>>();
    actual_hit_ids.sort();

    let mut forbidden_actions_caught = 0usize;
    for action in &scenario.forbidden_actions {
        let gate =
            crate::gate::precheck(&paths, action.path.as_deref(), action.command.as_deref())?;
        let matched_gate = gate
            .hits
            .iter()
            .any(|hit| hit.id == scenario.expected_gate_hit);
        let matched_pattern = action.pattern.as_ref().map_or(true, |pattern| {
            action
                .command
                .as_deref()
                .map_or(false, |command| command.contains(pattern))
                || action
                    .path
                    .as_deref()
                    .map_or(false, |path| path.contains(pattern))
        });
        if matched_gate && matched_pattern {
            forbidden_actions_caught += 1;
        }
    }

    let mut false_positive_misses = 0usize;
    for action in &scenario.false_positive_actions {
        let gate =
            crate::gate::precheck(&paths, action.path.as_deref(), action.command.as_deref())?;
        if !gate
            .hits
            .iter()
            .any(|hit| hit.id == scenario.expected_gate_hit)
        {
            false_positive_misses += 1;
        }
    }

    let passed = actual_hit_ids
        .iter()
        .any(|id| id == &scenario.expected_gate_hit)
        && forbidden_actions_caught == scenario.forbidden_actions.len();
    let passed = passed
        && false_positive_misses == scenario.false_positive_actions.len()
        && hook_permission == scenario.expected_permission;

    let _ = fs::remove_dir_all(&work);
    Ok(ScenarioResult {
        id: scenario.id.clone(),
        passed,
        expected_gate_hit: scenario.expected_gate_hit.clone(),
        expected_permission: scenario.expected_permission.clone(),
        hook_permission,
        actual_hit_ids,
        forbidden_actions_caught,
        false_positive_misses,
        elapsed_ms: started.elapsed().as_millis(),
    })
}

fn scenario_files(dir: &Path) -> Result<Vec<PathBuf>> {
    if !dir.exists() {
        anyhow::bail!(
            "RepeatBench scenarios directory not found: {}",
            dir.display()
        );
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path
            .extension()
            .is_some_and(|ext| ext == "yaml" || ext == "yml")
        {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

fn load_scenario(path: &Path) -> Result<Scenario> {
    let raw = fs::read_to_string(path)?;
    Ok(serde_yaml::from_str(&raw)?)
}

fn write_results_jsonl(path: &Path, report: &RepeatBenchReport) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut out = String::new();
    for result in &report.results {
        out.push_str(&serde_json::to_string(result)?);
        out.push('\n');
    }
    fs::write(path, out)?;
    Ok(())
}

fn default_version() -> u32 {
    1
}

fn default_expected_permission() -> String {
    "deny".into()
}

fn copy_dir(src: &Path, dst: &Path) -> Result<()> {
    if dst.exists() {
        fs::remove_dir_all(dst)?;
    }
    fs::create_dir_all(dst)?;
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry?;
        let path = entry.path();
        let rel = path.strip_prefix(src)?;
        if rel.as_os_str().is_empty()
            || rel
                .components()
                .any(|c| c.as_os_str() == std::ffi::OsStr::new(".git"))
        {
            continue;
        }
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(path, target)?;
        }
    }
    Ok(())
}
