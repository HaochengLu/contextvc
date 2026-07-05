use clap::{Parser, Subcommand};
use ctx::{
    adopt, backfill, check, distill, doctor, ensure_repo, find_repo_root, gate, harvest, hooks,
    init, mcp, render, repeatbench, review, status, verify, version,
};
use std::env;
use std::io::{self, Read};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "ctx",
    about = "ContextVC — Git-native context control plane",
    version
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[arg(long, global = true)]
    repo: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize .context/ in the current repo
    Init {
        #[arg(long)]
        skip_adopt: bool,
        #[arg(long)]
        skip_backfill: bool,
        #[arg(long)]
        skip_render: bool,
        #[arg(long)]
        install_hooks: bool,
    },
    /// Import drift or existing agent files into objects
    Adopt {
        #[arg(long)]
        file: Option<PathBuf>,
    },
    /// Backfill codemap from git history
    Backfill,
    /// Render projection targets from objects
    Render {
        #[arg(long)]
        force: bool,
    },
    /// Show context health summary
    Status {
        #[arg(long)]
        json: bool,
    },
    /// CI guard: lockfile freshness and drift
    Check {
        #[arg(long)]
        json: bool,
    },
    /// Run local MCP server (stdio)
    ServeMcp,
    /// Install hooks and MCP configs for agents
    Install {
        #[arg(value_enum)]
        target: Option<InstallTarget>,
    },
    /// Review proposal queue
    Review {
        #[command(subcommand)]
        action: ReviewAction,
    },
    /// Harvest session events into proposals
    Harvest,
    /// Run semantic merge over .context/objects
    Merge {
        #[arg(long)]
        json: bool,
    },
    /// Staleness verification
    Verify {
        #[arg(long)]
        mark: bool,
    },
    /// Temporarily suppress a gate hit
    Snooze {
        id: String,
        #[arg(long, default_value_t = 1)]
        days: u32,
    },
    /// Diagnose and optionally repair local ContextVC wiring
    Doctor {
        #[arg(long)]
        json: bool,
        #[arg(long)]
        fix: bool,
    },
    /// Print or write the OCL v0 object JSON schema
    Schema {
        #[arg(long)]
        output: Option<PathBuf>,
    },
    /// Run RepeatBench scenarios
    #[command(name = "repeatbench")]
    Repeatbench {
        #[arg(long)]
        scenarios: Option<PathBuf>,
        #[arg(long)]
        output: Option<PathBuf>,
        #[arg(long)]
        json: bool,
    },
    /// Knowledge timeline
    Log {
        #[arg(long)]
        scope: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Blame {
        query: String,
        #[arg(long)]
        json: bool,
    },
    Diff {
        #[arg(long)]
        scope: Option<String>,
        #[arg(long)]
        from: Option<String>,
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Revert {
        id: String,
    },
    /// Append event to ledger
    LogEvent {
        #[arg(long)]
        event_type: String,
        #[arg(long)]
        payload: String,
    },
    /// Hook subcommands (invoked by agent adapters)
    Hook {
        #[command(subcommand)]
        event: HookEvent,
    },
    /// Gate precheck (CLI)
    Precheck {
        #[arg(long)]
        path: Option<String>,
        #[arg(long)]
        command: Option<String>,
        #[arg(long)]
        json: bool,
    },
    Brief {
        #[arg(long)]
        task: Option<String>,
    },
}

#[derive(Subcommand)]
enum ReviewAction {
    List,
    Accept { id: String },
    Reject { id: String },
}

#[derive(Clone, Copy, clap::ValueEnum)]
enum InstallTarget {
    Claude,
    Cursor,
    Codex,
    Git,
    Mcp,
    All,
}

#[derive(Subcommand)]
enum HookEvent {
    SessionStart,
    PreTool,
    PostTool,
    UserPromptSubmit,
    PreCompact,
    Stop,
}

fn repo_root(hint: &Option<PathBuf>) -> anyhow::Result<PathBuf> {
    if let Some(root) = hint {
        return Ok(root.clone());
    }
    if let Ok(root) = env::var("CTX_REPO_ROOT") {
        return Ok(PathBuf::from(root));
    }
    find_repo_root(&env::current_dir()?)
}

fn explicit_or_current_root(hint: &Option<PathBuf>) -> anyhow::Result<PathBuf> {
    if let Some(root) = hint {
        return Ok(root.clone());
    }
    if let Ok(root) = env::var("CTX_REPO_ROOT") {
        return Ok(PathBuf::from(root));
    }
    Ok(env::current_dir()?)
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let repo_hint = cli.repo.clone();
    match cli.command {
        Commands::Init {
            skip_adopt,
            skip_backfill,
            skip_render,
            install_hooks,
        } => {
            let cwd = explicit_or_current_root(&repo_hint)?;
            let paths = init::init_repo(
                &cwd,
                init::InitOptions {
                    skip_adopt,
                    skip_backfill,
                    skip_render,
                },
            )?;
            if install_hooks {
                hooks::install_all(&paths)?;
            }
            println!("Initialized ContextVC at {}", paths.context.display());
        }
        Commands::Adopt { file } => {
            let root = repo_root(&repo_hint)?;
            let paths = ensure_repo(&root)?;
            if let Some(file) = file {
                adopt::adopt_drift(&paths, &file)?;
                println!("Adopted drift from {}", file.display());
            } else {
                let n = adopt::adopt_existing(&paths)?;
                println!("Adopted {n} object(s)");
            }
        }
        Commands::Backfill => {
            let root = repo_root(&repo_hint)?;
            let paths = ensure_repo(&root)?;
            let n = backfill::backfill(&paths)?;
            println!("Backfill created {n} codemap object(s)");
        }
        Commands::Render { force } => {
            let root = repo_root(&repo_hint)?;
            render::render_repo(&root, force)?;
            println!("Render complete");
        }
        Commands::Status { json } => {
            let root = repo_root(&repo_hint)?;
            let paths = ensure_repo(&root)?;
            let report = status::status(&paths)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("{}", status::format_human(&report));
            }
        }
        Commands::Check { json } => {
            let root = repo_root(&repo_hint)?;
            let paths = ensure_repo(&root)?;
            let report = check::check(&paths)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else if report.ok {
                println!("ctx check: OK");
                for w in &report.warnings {
                    println!("  warn: {w}");
                }
            } else {
                for e in &report.errors {
                    eprintln!("  error: {e}");
                }
                for w in &report.warnings {
                    eprintln!("  warn: {w}");
                }
                std::process::exit(1);
            }
        }
        Commands::ServeMcp => {
            let root = repo_root(&repo_hint)?;
            let paths = ensure_repo(&root)?;
            mcp::serve_stdio(&paths)?;
        }
        Commands::Install { target } => {
            let root = repo_root(&repo_hint)?;
            let paths = ensure_repo(&root)?;
            match target {
                None | Some(InstallTarget::All) => hooks::install_all(&paths)?,
                Some(InstallTarget::Claude) => hooks::install_claude(&paths)?,
                Some(InstallTarget::Cursor) => hooks::install_cursor(&paths)?,
                Some(InstallTarget::Codex) => hooks::install_codex(&paths)?,
                Some(InstallTarget::Git) => hooks::install_git_hooks(&paths)?,
                Some(InstallTarget::Mcp) => hooks::install_mcp_config(&paths)?,
            }
            println!("Install complete");
        }
        Commands::Review { action } => {
            let root = repo_root(&repo_hint)?;
            let paths = ensure_repo(&root)?;
            match action {
                ReviewAction::List => {
                    let list = review::review_list(&paths)?;
                    print!("{}", review::format_review_queue(&list));
                }
                ReviewAction::Accept { id } => {
                    let obj = review::review_accept(&paths, &id)?;
                    println!("Accepted {} → {}", id, obj.path.display());
                }
                ReviewAction::Reject { id } => {
                    review::review_reject(&paths, &id)?;
                    println!("Rejected {id}");
                }
            }
        }
        Commands::Harvest => {
            let root = repo_root(&repo_hint)?;
            let paths = ensure_repo(&root)?;
            let report = harvest::harvest(&paths)?;
            println!(
                "Harvest: {} proposal(s), {} merge update(s), {} conflict(s)",
                report.proposals_created,
                report.merge_updates,
                report.conflicted.len()
            );
        }
        Commands::Merge { json } => {
            let root = repo_root(&repo_hint)?;
            let paths = ensure_repo(&root)?;
            let report = harvest::semantic_merge(&paths)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Merge: {} update(s), {} conflict(s)",
                    report.updated,
                    report.conflicted.len()
                );
                for id in report.conflicted {
                    println!("  conflicted: {id}");
                }
            }
        }
        Commands::Verify { mark } => {
            let root = repo_root(&repo_hint)?;
            let paths = ensure_repo(&root)?;
            let report = verify::verify(&paths, mark)?;
            println!(
                "Verified {} object(s), {} stale",
                report.checked,
                report.stale.len()
            );
            for s in &report.stale {
                println!("  stale: {} — {}", s.id, s.reason);
            }
        }
        Commands::Snooze { id, days } => {
            let root = repo_root(&repo_hint)?;
            let paths = ensure_repo(&root)?;
            gate::snooze(&paths, &id, days)?;
            println!("Snoozed {id} for {days} day(s)");
        }
        Commands::Doctor { json, fix } => {
            let root = explicit_or_current_root(&repo_hint)?;
            let paths = ctx::ContextPaths::new(&root);
            let report = doctor::doctor(&paths, fix)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                if report.ok {
                    println!("ctx doctor: OK");
                } else {
                    println!("ctx doctor: issues found");
                }
                for fixed in &report.fixed {
                    println!("  fixed: {fixed}");
                }
                for finding in &report.findings {
                    println!(
                        "  {} [{}]: {} — {}",
                        finding.severity, finding.check, finding.message, finding.suggested_action
                    );
                }
            }
            if !report.ok {
                std::process::exit(1);
            }
        }
        Commands::Schema { output } => {
            let schema = ctx::object::object_frontmatter_schema_json()?;
            if let Some(output) = output {
                if let Some(parent) = output.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&output, schema)?;
                println!("Wrote schema to {}", output.display());
            } else {
                println!("{schema}");
            }
        }
        Commands::Repeatbench {
            scenarios,
            output,
            json,
        } => {
            let root = explicit_or_current_root(&repo_hint)?;
            let scenarios = scenarios.unwrap_or_else(|| {
                root.join("benchmarks")
                    .join("repeatbench")
                    .join("scenarios")
            });
            let report = repeatbench::run_with_output(&root, &scenarios, output.as_deref())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "RepeatBench: {}/{} passed, RFR {:.3}, TTC {}ms",
                    report.gate_hits, report.scenarios, report.repeat_failure_rate, report.ttc_ms
                );
                for result in &report.results {
                    println!(
                        "  {}: {} ({:?})",
                        result.id,
                        if result.passed { "pass" } else { "miss" },
                        result.actual_hit_ids
                    );
                }
            }
            if report.misses > 0 {
                std::process::exit(1);
            }
        }
        Commands::Log { scope, json } => {
            let root = repo_root(&repo_hint)?;
            let paths = ensure_repo(&root)?;
            let entries = version::log_scope(&paths, scope.as_deref())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&entries)?);
            } else {
                for e in entries {
                    println!("[{}] {} {} — {}", e.created, e.object_type, e.id, e.title);
                }
            }
        }
        Commands::Blame { query, json } => {
            let root = repo_root(&repo_hint)?;
            let paths = ensure_repo(&root)?;
            let report = version::blame(&paths, &query)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!("Objects: {:?}", report.objects);
                for ev in report.evidence {
                    println!(
                        "  {} @ {} by {}: {}",
                        ev.event_id, ev.ts, ev.actor, ev.summary
                    );
                }
            }
        }
        Commands::Diff {
            scope,
            from,
            to,
            json,
        } => {
            let root = repo_root(&repo_hint)?;
            let paths = ensure_repo(&root)?;
            let report = version::diff(&paths, scope.as_deref(), from.as_deref(), to.as_deref())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                println!(
                    "Diff: +{} proposals, deprecated {:?}, conflicted {:?}, git changes {}",
                    report.proposals,
                    report.deprecated,
                    report.conflicted,
                    report.git_changes.len()
                );
            }
        }
        Commands::Revert { id } => {
            let root = repo_root(&repo_hint)?;
            let paths = ensure_repo(&root)?;
            let obj = version::revert(&paths, &id)?;
            println!("Reverted {} (now deprecated)", obj.frontmatter.id);
            render::render_all(&paths, false)?;
        }
        Commands::LogEvent {
            event_type,
            payload,
        } => {
            let root = repo_root(&repo_hint)?;
            let paths = ensure_repo(&root)?;
            let val: serde_json::Value = serde_json::from_str(&payload)?;
            let id = if event_type == "attempt" {
                distill::log_attempt(&paths, val)?
            } else {
                distill::log_outcome(&paths, val)?
            };
            println!("{id}");
        }
        Commands::Hook { event } => {
            let root = repo_root(&repo_hint)?;
            let paths = ensure_repo(&root)?;
            let mut input = String::new();
            io::stdin().read_to_string(&mut input)?;
            match event {
                HookEvent::SessionStart => {
                    let brief = distill::handle_session_start(&paths)?;
                    println!(
                        "{}",
                        serde_json::to_string(&serde_json::json!({
                            "additionalContext": brief
                        }))?
                    );
                }
                HookEvent::PreTool => {
                    let out = distill::handle_pre_tool(&paths, &input)?;
                    println!("{}", serde_json::to_string(&out)?);
                }
                HookEvent::PostTool => {
                    distill::handle_post_tool(&paths, &input)?;
                }
                HookEvent::UserPromptSubmit => {
                    let out = distill::handle_user_prompt_submit(&paths, &input)?;
                    println!("{}", serde_json::to_string(&out)?);
                }
                HookEvent::PreCompact => {
                    let out = distill::handle_precompact(&paths, &input)?;
                    println!("{}", serde_json::to_string(&out)?);
                }
                HookEvent::Stop => {
                    let n = distill::handle_stop(&paths)?;
                    eprintln!("ContextVC distilled {n} proposal(s)");
                }
            }
        }
        Commands::Precheck {
            path,
            command,
            json,
        } => {
            let root = repo_root(&repo_hint)?;
            let paths = ensure_repo(&root)?;
            let result = gate::precheck(&paths, path.as_deref(), command.as_deref())?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                for hit in &result.hits {
                    println!("[{}] {}: {}", hit.enforcement, hit.id, hit.reason);
                }
            }
        }
        Commands::Brief { task } => {
            let root = repo_root(&repo_hint)?;
            let paths = ensure_repo(&root)?;
            println!("{}", gate::brief(&paths, task.as_deref())?);
        }
    }
    Ok(())
}
