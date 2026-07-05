use ctx::init::{init_repo, InitOptions};
use ctx::{
    adopt, check, distill, doctor, gate, harvest, hooks, render, repeatbench, review, version,
};
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use tempfile::TempDir;

#[test]
fn phase0_init_render_check() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(root)
        .output()
        .unwrap();

    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: false,
        },
    )
    .unwrap();

    assert!(paths.version_file.exists());
    assert!(paths.agents_md().exists());
    assert!(paths.claude_md().exists());
    assert!(paths.render_lock.exists());

    let report = check::check(&paths).unwrap();
    assert!(report.ok, "{:?}", report.errors);
}

#[test]
fn cli_init_honors_repo_argument() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("target-repo");
    let cwd = tmp.path().join("caller");
    fs::create_dir_all(&root).unwrap();
    fs::create_dir_all(&cwd).unwrap();

    let output = Command::new(ctx_bin())
        .args([
            "--repo",
            root.to_str().unwrap(),
            "init",
            "--skip-adopt",
            "--skip-backfill",
        ])
        .current_dir(&cwd)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(root.join(".context/VERSION").exists());
    assert!(!cwd.join(".context/VERSION").exists());
}

#[test]
fn managed_block_roundtrip() {
    let body = "test rules";
    let wrapped = ctx::compiler::managed::wrap_managed("test", "abcd1234", body);
    let merged = ctx::compiler::managed::merge_into_file("# Title\n", &wrapped);
    assert!(merged.contains("ctx:begin"));
    assert!(merged.contains(body));
}

#[test]
fn secret_redaction_on_save() {
    let fake_token_title = format!("token={}{}", "ghp_", "abcdefghijklmnopqrstuvwxyz1234567890");
    let mut obj = ctx::object::new_object(
        ctx::object::ObjectType::Constraint,
        &fake_token_title,
        "body",
        vec![],
        "active",
    );
    let tmp = TempDir::new().unwrap();
    obj.path = tmp.path().join("test.md");
    obj.save().unwrap();
    let saved = fs::read_to_string(obj.path).unwrap();
    assert!(saved.contains("[REDACTED]"));
}

#[test]
fn render_force_after_object_change() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: true,
        },
    )
    .unwrap();

    render::render_all(&paths, false).unwrap();
    let obj = ctx::object::new_object(
        ctx::object::ObjectType::Constraint,
        "Use pnpm",
        "Never run npm install in this repo.",
        vec!["**".into()],
        "active",
    );
    let dest = paths.object_dir("constraints").join("use-pnpm.md");
    std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
    let mut saved = obj;
    saved.path = dest;
    saved.save().unwrap();

    let stale = check::check(&paths).unwrap();
    assert!(!stale.ok);

    render::render_all(&paths, true).unwrap();
    let agents = fs::read_to_string(paths.agents_md()).unwrap();
    assert!(agents.contains("pnpm"));
    let report = check::check(&paths).unwrap();
    assert!(report.ok);
}

#[test]
fn render_reports_existing_operation_lock() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: false,
        },
    )
    .unwrap();

    fs::write(paths.cache.join("operation.lock"), "pid=test\n").unwrap();
    let err = render::render_all(&paths, true).unwrap_err();
    assert!(err.to_string().contains("another ctx write operation"));
}

#[test]
fn human_edit_outside_managed_passes_check() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: false,
        },
    )
    .unwrap();

    let mut agents = fs::read_to_string(paths.agents_md()).unwrap();
    agents.insert_str(0, "# Human preamble\n\n");
    fs::write(paths.agents_md(), agents).unwrap();

    let report = check::check(&paths).unwrap();
    assert!(report.ok, "{:?}", report.errors);
}

#[test]
fn backfill_creates_codemap_from_git() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(root)
        .output()
        .unwrap();
    let file = root.join("src/foo.rs");
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    for i in 0..3 {
        fs::write(&file, format!("fn main() {{ /* v{i} */ }}\n")).unwrap();
        std::process::Command::new("git")
            .args(["add", "."])
            .current_dir(root)
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["commit", "-m", "touch"])
            .env("GIT_AUTHOR_NAME", "test")
            .env("GIT_AUTHOR_EMAIL", "t@t.com")
            .env("GIT_COMMITTER_NAME", "test")
            .env("GIT_COMMITTER_EMAIL", "t@t.com")
            .current_dir(root)
            .output()
            .unwrap();
    }

    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: false,
            skip_render: true,
        },
    )
    .unwrap();

    let codemap_dir = paths.object_dir("codemap");
    let count = fs::read_dir(codemap_dir)
        .map(|rd| rd.filter_map(Result::ok).count())
        .unwrap_or(0);
    assert!(count >= 1, "expected backfill codemap objects");
}

#[test]
fn init_with_backfill_skips_empty_git_history() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(root)
        .output()
        .unwrap();

    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: false,
            skip_render: false,
        },
    )
    .unwrap();

    assert!(paths.version_file.exists());
    let report = check::check(&paths).unwrap();
    assert!(report.ok, "{:?}", report.errors);
}

#[test]
fn phase1_write_loop_is_idempotent_and_accepts_to_active_object() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: false,
        },
    )
    .unwrap();

    let event_id = distill::log_attempt(
        &paths,
        serde_json::json!({
            "target": "src/parser.rs",
            "action": "cargo test parser",
            "outcome": "failed",
            "evidence": "cargo test parser failed with exit code 101"
        }),
    )
    .unwrap();

    assert_eq!(distill::handle_stop(&paths).unwrap(), 1);
    assert_eq!(distill::handle_stop(&paths).unwrap(), 0);

    let proposals = review::review_list(&paths).unwrap();
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].frontmatter.evidence, vec![event_id.clone()]);

    let accepted = review::review_accept(&paths, &proposals[0].frontmatter.id).unwrap();
    assert_eq!(accepted.frontmatter.status, "active");
    assert!(accepted.path.starts_with(paths.object_dir("failures")));

    assert_eq!(distill::handle_stop(&paths).unwrap(), 0);
    assert!(review::review_list(&paths).unwrap().is_empty());
    assert!(check::check(&paths).unwrap().ok);
}

#[test]
fn review_accept_projectable_proposal_renders_fresh() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: false,
        },
    )
    .unwrap();

    let mut proposal = ctx::object::new_object(
        ctx::object::ObjectType::Decision,
        "Prefer cargo nextest",
        "Use cargo nextest for local Rust test loops.",
        vec!["**".into()],
        "proposed",
    );
    let proposal_id = proposal.frontmatter.id.clone();
    proposal.path = paths
        .proposals
        .join("decisions")
        .join("prefer-cargo-nextest.md");
    fs::create_dir_all(proposal.path.parent().unwrap()).unwrap();
    proposal.save().unwrap();

    let accepted = review::review_accept(&paths, &proposal_id).unwrap();
    assert_eq!(accepted.frontmatter.status, "active");
    assert!(!paths
        .proposals
        .join("decisions/prefer-cargo-nextest.md")
        .exists());

    let agents = fs::read_to_string(paths.agents_md()).unwrap();
    assert!(agents.contains("Use cargo nextest for local Rust test loops."));
    assert!(check::check(&paths).unwrap().ok);
}

#[test]
fn review_accept_rejects_existing_destination_with_different_id() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: true,
        },
    )
    .unwrap();

    let mut active = ctx::object::new_object(
        ctx::object::ObjectType::Decision,
        "Existing decision",
        "Keep this object.",
        vec!["**".into()],
        "active",
    );
    active.path = paths.object_dir("decisions").join("same-name.md");
    active.save().unwrap();
    render::render_all(&paths, true).unwrap();

    let mut proposal = ctx::object::new_object(
        ctx::object::ObjectType::Decision,
        "Different proposal",
        "Do not overwrite the existing object.",
        vec!["**".into()],
        "proposed",
    );
    let proposal_id = proposal.frontmatter.id.clone();
    let proposal_path = paths.proposals.join("decisions").join("same-name.md");
    proposal.path = proposal_path.clone();
    fs::create_dir_all(proposal.path.parent().unwrap()).unwrap();
    proposal.save().unwrap();

    let err = review::review_accept(&paths, &proposal_id).unwrap_err();
    assert!(err
        .to_string()
        .contains("destination object already exists"));
    assert!(proposal_path.exists());
    let active_after =
        fs::read_to_string(paths.object_dir("decisions").join("same-name.md")).unwrap();
    assert!(active_after.contains("Keep this object."));
}

#[test]
fn review_accept_keeps_proposal_when_preflight_check_fails() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: false,
        },
    )
    .unwrap();

    let mut proposal = ctx::object::new_object(
        ctx::object::ObjectType::Decision,
        "Prefer cargo nextest",
        "Use cargo nextest for local Rust test loops.",
        vec!["**".into()],
        "proposed",
    );
    let proposal_id = proposal.frontmatter.id.clone();
    let proposal_path = paths
        .proposals
        .join("decisions")
        .join("prefer-cargo-nextest.md");
    proposal.path = proposal_path.clone();
    fs::create_dir_all(proposal.path.parent().unwrap()).unwrap();
    proposal.save().unwrap();

    let agents = fs::read_to_string(paths.agents_md()).unwrap();
    fs::write(
        paths.agents_md(),
        agents.replace("Agent Instructions", "Tampered Instructions"),
    )
    .unwrap();

    let err = review::review_accept(&paths, &proposal_id).unwrap_err();
    assert!(err.to_string().contains("ctx check is failing"));
    assert!(proposal_path.exists());
    assert!(!paths
        .object_dir("decisions")
        .join("prefer-cargo-nextest.md")
        .exists());
}

#[test]
fn review_accept_rolls_back_outputs_when_render_fails_after_activation() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: false,
        },
    )
    .unwrap();
    let agents_before = fs::read_to_string(paths.agents_md()).unwrap();
    let lock_before = fs::read_to_string(&paths.render_lock).unwrap();

    let mut proposal = ctx::object::new_object(
        ctx::object::ObjectType::Howto,
        "API howto",
        "Use typed API clients.",
        vec!["src/api/**".into()],
        "proposed",
    );
    let proposal_id = proposal.frontmatter.id.clone();
    let proposal_path = paths.proposals.join("howtos").join("api-howto.md");
    proposal.path = proposal_path.clone();
    fs::create_dir_all(proposal.path.parent().unwrap()).unwrap();
    proposal.save().unwrap();

    let future_cursor_target = paths.cursor_rules_dir().join("ctx-src-api-all.mdc");
    fs::create_dir_all(&future_cursor_target).unwrap();

    let err = review::review_accept(&paths, &proposal_id).unwrap_err();
    assert!(err.to_string().contains("rolled back"));
    assert!(proposal_path.exists());
    assert!(!paths.object_dir("howtos").join("api-howto.md").exists());
    assert_eq!(
        fs::read_to_string(paths.agents_md()).unwrap(),
        agents_before
    );
    assert_eq!(fs::read_to_string(&paths.render_lock).unwrap(), lock_before);
    assert!(check::check(&paths).unwrap().ok);
}

#[test]
fn rejected_distilled_event_does_not_reappear() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: false,
        },
    )
    .unwrap();

    distill::log_attempt(
        &paths,
        serde_json::json!({
            "target": "src/parser.rs",
            "action": "cargo test parser",
            "outcome": "failed",
            "evidence": "cargo test parser failed with exit code 101"
        }),
    )
    .unwrap();
    assert_eq!(distill::handle_stop(&paths).unwrap(), 1);
    let proposals = review::review_list(&paths).unwrap();
    assert_eq!(proposals.len(), 1);
    review::review_reject(&paths, &proposals[0].frontmatter.id).unwrap();

    assert_eq!(distill::handle_stop(&paths).unwrap(), 0);
    assert!(review::review_list(&paths).unwrap().is_empty());
}

#[test]
fn install_git_hook_chains_existing_hook_idempotently() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    std::process::Command::new("git")
        .args(["init"])
        .current_dir(root)
        .output()
        .unwrap();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: true,
        },
    )
    .unwrap();

    let hook = root.join(".git/hooks/pre-commit");
    fs::write(&hook, "#!/usr/bin/env bash\necho existing-hook\n").unwrap();

    hooks::install_git_hooks(&paths).unwrap();
    hooks::install_git_hooks(&paths).unwrap();

    let installed = fs::read_to_string(&hook).unwrap();
    assert!(installed.contains("echo existing-hook"));
    assert_eq!(installed.matches("ctx check").count(), 1);
}

#[test]
fn installed_precommit_runs_before_existing_early_exit() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    Command::new("git")
        .args(["init"])
        .current_dir(root)
        .output()
        .unwrap();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: false,
        },
    )
    .unwrap();

    let hook = root.join(".git/hooks/pre-commit");
    fs::write(&hook, "#!/usr/bin/env bash\nexit 0\n").unwrap();
    hooks::install_git_hooks(&paths).unwrap();

    let agents = fs::read_to_string(paths.agents_md()).unwrap();
    fs::write(
        paths.agents_md(),
        agents.replace("Agent Instructions", "Tampered Instructions"),
    )
    .unwrap();

    let output = Command::new(&hook)
        .current_dir(root)
        .env("PATH", path_with_ctx_bin())
        .output()
        .unwrap();
    assert!(!output.status.success());
}

#[test]
fn install_claude_preserves_existing_hook_arrays_idempotently() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: true,
        },
    )
    .unwrap();

    let settings_path = root.join(".claude/settings.json");
    fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
    fs::write(
        &settings_path,
        serde_json::to_string_pretty(&serde_json::json!({
            "hooks": {
                "PreToolUse": [
                    {
                        "matcher": "Bash",
                        "hooks": [
                            { "type": "command", "command": "echo existing" }
                        ]
                    }
                ]
            }
        }))
        .unwrap(),
    )
    .unwrap();

    hooks::install_claude(&paths).unwrap();
    hooks::install_claude(&paths).unwrap();

    let settings: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
    let pre_tool = settings["hooks"]["PreToolUse"].as_array().unwrap();
    assert_eq!(pre_tool.len(), 2);
    assert!(pre_tool.iter().any(|entry| {
        entry["hooks"][0]["command"]
            .as_str()
            .is_some_and(|cmd| cmd == "echo existing")
    }));
    assert_eq!(
        pre_tool
            .iter()
            .filter(|entry| entry["hooks"][0]["command"]
                .as_str()
                .is_some_and(|cmd| cmd.contains("ContextVC") || cmd.contains("contextvc-hook.sh")))
            .count(),
        1
    );
}

#[test]
fn installed_editor_hook_command_quotes_space_paths_and_runs() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("repo with space");
    fs::create_dir_all(&root).unwrap();
    let paths = init_repo(
        &root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: false,
        },
    )
    .unwrap();

    hooks::install_claude(&paths).unwrap();
    let settings_path = root.join(".claude/settings.json");
    let settings: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&settings_path).unwrap()).unwrap();
    let command = settings["hooks"]["SessionStart"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap();
    assert!(command.contains("'"));

    let output = Command::new("sh")
        .arg("-c")
        .arg(format!("CTX_BIN={} {command}", shell_quote(ctx_bin())))
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(value["additionalContext"]
        .as_str()
        .unwrap()
        .contains("ContextVC Brief"));
}

#[test]
fn cursor_fail_closed_pre_hook_denies_when_ctx_unavailable() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: false,
        },
    )
    .unwrap();
    hooks::install_cursor(&paths).unwrap();
    let hook = root.join(".cursor/contextvc-hook.sh");
    let output = Command::new(&hook)
        .env("CTX_HOOK_AGENT", "cursor")
        .env("CTX_HOOK_EVENT", "beforeShellExecution")
        .env("CTX_BIN", "/definitely/missing/ctx")
        .current_dir(root)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["permissionDecision"], "deny");
}

#[test]
fn adopt_existing_upserts_by_source_and_preserves_cursor_globs() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::write(root.join("AGENTS.md"), "Use pnpm.\n").unwrap();
    let rules = root.join(".cursor/rules");
    fs::create_dir_all(&rules).unwrap();
    fs::write(
        rules.join("api.mdc"),
        "---\ndescription: API rules\nglobs: src/api/**\nalwaysApply: false\n---\n\nUse typed API clients.\n",
    )
    .unwrap();

    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: true,
        },
    )
    .unwrap();

    assert_eq!(adopt::adopt_existing(&paths).unwrap(), 2);
    assert_eq!(adopt::adopt_existing(&paths).unwrap(), 0);

    let objects = ctx::object::load_all_objects(&paths.objects).unwrap();
    assert_eq!(objects.len(), 2);
    let howto = objects
        .iter()
        .find(|o| o.frontmatter.object_type == "howto")
        .unwrap();
    assert_eq!(howto.frontmatter.scope, vec!["src/api/**".to_string()]);

    fs::write(
        root.join("AGENTS.md"),
        "Use pnpm.\nRun cargo fmt before commit.\n",
    )
    .unwrap();
    assert_eq!(adopt::adopt_existing(&paths).unwrap(), 1);

    let objects = ctx::object::load_all_objects(&paths.objects).unwrap();
    assert_eq!(objects.len(), 2);
    let adopted_agents = objects
        .iter()
        .find(|o| o.frontmatter.title.contains("AGENTS.md"))
        .unwrap();
    assert!(adopted_agents.body.contains("cargo fmt"));
}

#[test]
fn adopted_source_binding_drift_fails_check() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::write(root.join("AGENTS.md"), "Use pnpm.\n").unwrap();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: false,
            skip_backfill: true,
            skip_render: false,
        },
    )
    .unwrap();
    assert!(check::check(&paths).unwrap().ok);

    let agents = fs::read_to_string(paths.agents_md()).unwrap();
    fs::write(paths.agents_md(), agents.replace("Use pnpm.", "Use npm.")).unwrap();

    let report = check::check(&paths).unwrap();
    assert!(!report.ok);
    assert!(report.errors.iter().any(|e| e.contains("stale binding")));
}

#[cfg(unix)]
#[test]
fn adopt_rejects_symlink_sources() {
    use std::os::unix::fs::symlink;

    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let outside = tmp.path().join("outside-rules.mdc");
    fs::write(&outside, "Do not import me.\n").unwrap();
    let rules = root.join(".cursor/rules");
    fs::create_dir_all(&rules).unwrap();
    symlink(&outside, rules.join("leak.mdc")).unwrap();

    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: true,
        },
    )
    .unwrap();
    let err = adopt::adopt_existing(&paths).unwrap_err();
    assert!(err.to_string().contains("symlink"));
}

#[test]
fn adopt_existing_ignores_contextvc_managed_projection_blocks() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: false,
        },
    )
    .unwrap();

    assert_eq!(adopt::adopt_existing(&paths).unwrap(), 0);
    assert!(ctx::object::load_all_objects(&paths.objects)
        .unwrap()
        .is_empty());
}

#[test]
fn check_fails_on_stale_file_binding() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: true,
        },
    )
    .unwrap();

    let source = root.join("src/lib.rs");
    fs::create_dir_all(source.parent().unwrap()).unwrap();
    fs::write(&source, "pub fn answer() -> i32 { 42 }\n").unwrap();
    let sha = sha12(&fs::read(&source).unwrap());

    let mut obj = ctx::object::new_object(
        ctx::object::ObjectType::Decision,
        "Bound decision",
        "Keep the answer implementation stable.",
        vec!["src/**".into()],
        "active",
    );
    obj.frontmatter.bindings.push(ctx::object::Binding {
        kind: "file".into(),
        path: Some("src/lib.rs".into()),
        name: None,
        sha: Some(sha),
        hash: None,
        pattern: None,
        enforcement: None,
    });
    obj.path = paths.object_dir("decisions").join("bound-decision.md");
    obj.save().unwrap();

    render::render_all(&paths, true).unwrap();
    fs::write(&source, "pub fn answer() -> i32 { 43 }\n").unwrap();

    let report = check::check(&paths).unwrap();
    assert!(!report.ok);
    assert!(
        report
            .errors
            .iter()
            .any(|e| e.contains("stale binding") && e.contains("src/lib.rs")),
        "{:?}",
        report.errors
    );
}

#[test]
fn check_fails_on_stale_status_missing_binding_and_corrupt_events() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: true,
        },
    )
    .unwrap();

    let mut stale = ctx::object::new_object(
        ctx::object::ObjectType::Decision,
        "Stale decision",
        "This should fail check.",
        vec!["**".into()],
        "stale",
    );
    stale.path = paths.object_dir("decisions").join("stale-decision.md");
    stale.save().unwrap();

    let mut missing = ctx::object::new_object(
        ctx::object::ObjectType::Decision,
        "Missing binding",
        "This should fail check too.",
        vec!["**".into()],
        "active",
    );
    missing.frontmatter.bindings.push(ctx::object::Binding {
        kind: "file".into(),
        path: Some("missing.rs".into()),
        name: None,
        sha: Some("deadbeefdead".into()),
        hash: None,
        pattern: None,
        enforcement: None,
    });
    missing.path = paths.object_dir("decisions").join("missing-binding.md");
    missing.save().unwrap();

    fs::create_dir_all(&paths.events).unwrap();
    fs::write(paths.events.join("2026-07.bad.jsonl"), "{not-json}\n").unwrap();
    render::render_all(&paths, true).unwrap();

    let report = check::check(&paths).unwrap();
    assert!(!report.ok);
    assert!(report.errors.iter().any(|e| e.contains("stale object")));
    assert!(report
        .errors
        .iter()
        .any(|e| e.contains("bound file missing")));
    assert!(report
        .errors
        .iter()
        .any(|e| e.contains("invalid event ledger")));
}

#[test]
fn check_rejects_unsafe_binding_paths_without_hashing_them() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: true,
        },
    )
    .unwrap();

    let mut obj = ctx::object::new_object(
        ctx::object::ObjectType::Decision,
        "Unsafe binding",
        "This binding should never read outside the repo.",
        vec!["**".into()],
        "active",
    );
    obj.frontmatter.bindings.push(ctx::object::Binding {
        kind: "file".into(),
        path: Some("/etc/passwd".into()),
        name: None,
        sha: Some("deadbeefdead".into()),
        hash: None,
        pattern: None,
        enforcement: None,
    });
    obj.path = paths.object_dir("decisions").join("unsafe-binding.md");
    obj.save().unwrap();
    render::render_all(&paths, true).unwrap();

    let report = check::check(&paths).unwrap();
    assert!(!report.ok);
    assert!(report
        .errors
        .iter()
        .any(|e| e.contains("unsafe absolute binding path")));
    assert!(!report.errors.iter().any(|e| e.contains("got ")));
}

#[test]
fn cursor_mdc_render_preserves_human_text_outside_managed_block() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: true,
        },
    )
    .unwrap();

    let mut obj = ctx::object::new_object(
        ctx::object::ObjectType::Howto,
        "API howto",
        "Use typed API clients.",
        vec!["src/api/**".into()],
        "active",
    );
    obj.path = paths.object_dir("howtos").join("api-howto.md");
    obj.save().unwrap();

    render::render_all(&paths, true).unwrap();
    let cursor_rule = paths.cursor_rules_dir().join("ctx-src-api-all.mdc");
    let existing = fs::read_to_string(&cursor_rule).unwrap();
    let mut existing = existing.replace(
        "<!-- ctx:begin",
        "Human note before managed block.\n\n<!-- ctx:begin",
    );
    existing.push_str("\nHuman note after managed block.\n");
    fs::write(&cursor_rule, existing).unwrap();

    render::render_all(&paths, false).unwrap();
    let rendered = fs::read_to_string(&cursor_rule).unwrap();
    assert!(rendered.contains("Use typed API clients."));
    assert!(rendered.contains("Human note before managed block."));
    assert!(rendered.contains("Human note after managed block."));
    assert!(check::check(&paths).unwrap().ok);

    fs::write(
        &cursor_rule,
        rendered.replace("globs: src/api/**", "globs: wrong/**"),
    )
    .unwrap();
    assert!(!check::check(&paths).unwrap().ok);
    render::render_all(&paths, false).unwrap();
    let rerendered = fs::read_to_string(&cursor_rule).unwrap();
    assert!(rerendered.contains("globs: src/api/**"));
    assert!(check::check(&paths).unwrap().ok);
}

#[test]
fn cursor_mdc_render_preserves_multiple_scopes() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: true,
        },
    )
    .unwrap();

    let mut obj = ctx::object::new_object(
        ctx::object::ObjectType::Howto,
        "API howto",
        "Use typed API clients.",
        vec!["src/api/**".into(), "tests/api/**".into()],
        "active",
    );
    obj.path = paths.object_dir("howtos").join("api-howto.md");
    obj.save().unwrap();

    render::render_all(&paths, true).unwrap();
    let rendered = fs::read_dir(paths.cursor_rules_dir())
        .unwrap()
        .filter_map(Result::ok)
        .find_map(|entry| fs::read_to_string(entry.path()).ok())
        .unwrap();
    assert!(rendered.contains("globs: src/api/**,tests/api/**"));
    assert!(check::check(&paths).unwrap().ok);
}

#[test]
fn mcp_stdio_sanitizes_log_and_nested_proposal_scope() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: true,
        },
    )
    .unwrap();

    let mut child = Command::new(ctx_bin())
        .args(["--repo", root.to_str().unwrap(), "serve-mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    {
        let stdin = child.stdin.as_mut().unwrap();
        writeln!(stdin, "{{not-json").unwrap();
        writeln!(
            stdin,
            "{}",
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 1,
                "method": "tools/call",
                "params": {
                    "name": "context_log",
                    "arguments": {
                        "event": {
                            "id": "ev_fake",
                            "actor": { "kind": "human", "name": "mallory" },
                            "type": "attempt",
                            "payload": { "outcome": "failed", "evidence": "failed" }
                        }
                    }
                }
            })
        )
        .unwrap();
        writeln!(
            stdin,
            "{}",
            serde_json::json!({
                "jsonrpc": "2.0",
                "id": 2,
                "method": "tools/call",
                "params": {
                    "name": "context_propose",
                    "arguments": {
                        "object": {
                            "type": "decision",
                            "title": "Scoped proposal",
                            "body": "Only API files.",
                            "scope": ["src/api/**"],
                            "status": "active",
                            "trust": "human"
                        }
                    }
                }
            })
        )
        .unwrap();
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let responses: Vec<serde_json::Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(responses[0]["error"]["code"], -32700);

    let events = ctx::event::read_events(&paths.events).unwrap();
    assert_eq!(events[0].actor.kind, "agent");
    assert_ne!(events[0].id, "ev_fake");

    let proposals = ctx::object::load_proposals(&paths.proposals).unwrap();
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].frontmatter.status, "proposed");
    assert_eq!(proposals[0].frontmatter.trust, "agent_auto");
    assert_eq!(
        proposals[0].frontmatter.scope,
        vec!["src/api/**".to_string()]
    );
}

#[test]
fn gate_command_matching_ask_snooze_and_events() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: true,
        },
    )
    .unwrap();

    let mut obj = ctx::object::new_object(
        ctx::object::ObjectType::Constraint,
        "Ask before npm install",
        "Use pnpm install unless a human approves npm install.",
        vec!["**".into()],
        "active",
    );
    let id = obj.frontmatter.id.clone();
    obj.frontmatter.bindings.push(ctx::object::Binding {
        kind: "command".into(),
        path: None,
        name: None,
        sha: None,
        hash: None,
        pattern: Some("npm install".into()),
        enforcement: Some("ask".into()),
    });
    obj.path = paths.object_dir("constraints").join("ask-npm-install.md");
    obj.save().unwrap();
    render::render_all(&paths, true).unwrap();

    let miss = gate::precheck(&paths, None, Some("pnpm install")).unwrap();
    assert!(miss.hits.is_empty());
    let miss = gate::precheck(&paths, None, Some("npm-check-updates")).unwrap();
    assert!(miss.hits.is_empty());

    let hit = gate::precheck(&paths, None, Some("npm install --ignore-scripts")).unwrap();
    assert_eq!(hit.severity, "ask");
    assert_eq!(hit.hits[0].id, id);

    let hook = distill::handle_pre_tool(
        &paths,
        &serde_json::json!({
            "tool_name": "Bash",
            "tool_input": { "command": "npm install" }
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(hook["permissionDecision"], "ask");
    assert_eq!(hook["permission"], "ask");

    let cursor_top_level_miss = distill::handle_pre_tool(
        &paths,
        &serde_json::json!({ "command": "pnpm install" }).to_string(),
    )
    .unwrap();
    assert_eq!(cursor_top_level_miss["permissionDecision"], "allow");

    let cursor_top_level_hit = distill::handle_pre_tool(
        &paths,
        &serde_json::json!({ "command": "npm install" }).to_string(),
    )
    .unwrap();
    assert_eq!(cursor_top_level_hit["permissionDecision"], "ask");

    let edit = distill::handle_pre_tool(
        &paths,
        &serde_json::json!({
            "tool_name": "Edit",
            "tool_input": { "file_path": "src/main.rs" }
        })
        .to_string(),
    )
    .unwrap();
    assert_eq!(edit["permissionDecision"], "allow");

    gate::snooze(&paths, &id, 1).unwrap();
    let snoozed = gate::precheck(&paths, None, Some("npm install")).unwrap();
    assert!(snoozed.hits.is_empty());

    let events = ctx::event::read_events(&paths.events).unwrap();
    assert!(events.iter().any(|e| e.event_type == "gate_hit"));
    assert!(events.iter().any(|e| e.event_type == "gate_snooze"));
}

#[test]
fn semantic_merge_persists_conflicted_constraints_and_blocks_render() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: true,
        },
    )
    .unwrap();

    for title in ["Use pnpm", "Run cargo fmt"] {
        let mut obj = ctx::object::new_object(
            ctx::object::ObjectType::Constraint,
            title,
            &format!("{title} before committing."),
            vec!["**".into()],
            "active",
        );
        obj.path = paths
            .object_dir("constraints")
            .join(format!("{}.md", title.to_lowercase().replace(' ', "-")));
        obj.save().unwrap();
    }

    render::render_all(&paths, true).unwrap();
    let report = harvest::semantic_merge(&paths).unwrap();
    assert_eq!(report.updated, 0);
    assert!(report.conflicted.is_empty());
    assert!(check::check(&paths).unwrap().ok);

    let conflict_specs = [
        ("Do not run npm install", "Never run npm install.", -1),
        ("Always run npm install", "Always run npm install.", 1),
    ];
    for (title, body, _polarity) in conflict_specs {
        let mut obj = ctx::object::new_object(
            ctx::object::ObjectType::Constraint,
            title,
            body,
            vec!["**".into()],
            "active",
        );
        obj.frontmatter.bindings.push(ctx::object::Binding {
            kind: "command".into(),
            path: None,
            name: None,
            sha: None,
            hash: None,
            pattern: Some("npm install".into()),
            enforcement: Some("block".into()),
        });
        obj.path = paths
            .object_dir("constraints")
            .join(format!("{}.md", title.to_lowercase().replace(' ', "-")));
        obj.save().unwrap();
    }
    render::render_all(&paths, true).unwrap();
    let report = harvest::semantic_merge(&paths).unwrap();
    assert_eq!(report.updated, 2);
    assert_eq!(report.conflicted.len(), 2);
    let again = harvest::semantic_merge(&paths).unwrap();
    assert_eq!(again.updated, 0);

    let objects = ctx::object::load_all_objects(&paths.objects).unwrap();
    assert_eq!(
        objects
            .iter()
            .filter(|o| o.frontmatter.status == "conflicted")
            .count(),
        2
    );
    let check = check::check(&paths).unwrap();
    assert!(!check.ok);
    assert!(check
        .errors
        .iter()
        .any(|e| e.contains("conflicted constraint blocks render")));
    let err = render::render_all(&paths, true).unwrap_err();
    assert!(err.to_string().contains("conflicted constraint"));
}

#[test]
fn extended_projection_targets_render_and_detect_drift() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: false,
        },
    )
    .unwrap();

    assert!(paths.copilot_instructions().exists());
    assert!(paths.gemini_md().exists());
    assert!(paths.cline_memory_bank().exists());
    assert!(check::check(&paths).unwrap().ok);

    let gemini = fs::read_to_string(paths.gemini_md()).unwrap();
    fs::write(
        paths.gemini_md(),
        gemini.replace("Gemini Project Context", "Tampered Gemini Context"),
    )
    .unwrap();
    assert!(!check::check(&paths).unwrap().ok);
    render::render_all(&paths, true).unwrap();
    assert!(check::check(&paths).unwrap().ok);
}

#[test]
fn golden_path_fixture_init_adopts_globs_and_checks_clean() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path().join("golden");
    copy_dir(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/golden-path")
            .as_path(),
        &root,
    );

    let paths = init_repo(
        &root,
        InitOptions {
            skip_adopt: false,
            skip_backfill: true,
            skip_render: false,
        },
    )
    .unwrap();
    assert!(check::check(&paths).unwrap().ok);

    let objects = ctx::object::load_all_objects(&paths.objects).unwrap();
    assert_eq!(objects.len(), 3);
    assert!(objects.iter().any(|obj| {
        obj.frontmatter.object_type == "howto"
            && obj.frontmatter.scope == vec!["src/api/**".to_string(), "tests/api/**".to_string()]
    }));
    assert!(objects.iter().any(|obj| {
        obj.frontmatter.object_type == "howto"
            && obj.frontmatter.scope == vec!["src/ui/**".to_string(), "tests/ui/**".to_string()]
    }));
}

#[test]
fn doctor_fix_repairs_partial_init_and_schema_validation_fails_check() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    fs::create_dir_all(root.join(".context/objects/decisions")).unwrap();

    let paths = ctx::ContextPaths::new(root);
    let report = doctor::doctor(&paths, true).unwrap();
    assert!(paths.version_file.exists());
    assert!(paths.config_file.exists());
    assert!(paths.render_lock.exists());
    assert!(report.fixed.iter().any(|f| f.contains("VERSION")));

    let mut bad = ctx::object::new_object(
        ctx::object::ObjectType::Decision,
        "Bad schema",
        "This should fail schema validation.",
        vec!["**".into()],
        "active",
    );
    bad.frontmatter.status = "active-ish".into();
    bad.frontmatter.trust = "ghost".into();
    bad.frontmatter.bindings.push(ctx::object::Binding {
        kind: "mystery".into(),
        path: None,
        name: None,
        sha: None,
        hash: None,
        pattern: None,
        enforcement: Some("maybe".into()),
    });
    bad.path = paths.object_dir("decisions").join("bad-schema.md");
    bad.save().unwrap();

    let check = check::check(&paths).unwrap();
    assert!(!check.ok);
    assert!(check.errors.iter().any(|e| e.contains("invalid status")));
    assert!(check.errors.iter().any(|e| e.contains("invalid trust")));
    assert!(check
        .errors
        .iter()
        .any(|e| e.contains("invalid binding kind")));
    let schema_path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("docs/schema/ocl-object-v0.schema.json");
    assert!(schema_path.exists());
    let schema: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(schema_path).unwrap()).unwrap();
    assert!(schema["properties"]["status"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "active"));
    assert!(schema["$defs"]["Binding"]["properties"]["kind"]["enum"]
        .as_array()
        .unwrap()
        .iter()
        .any(|value| value == "command"));
}

#[test]
fn version_blame_diff_and_revert_are_git_aware_and_cascade() {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    Command::new("git")
        .args(["init"])
        .current_dir(root)
        .output()
        .unwrap();
    let paths = init_repo(
        root,
        InitOptions {
            skip_adopt: true,
            skip_backfill: true,
            skip_render: true,
        },
    )
    .unwrap();

    let mut decision = ctx::object::new_object(
        ctx::object::ObjectType::Decision,
        "Base architecture",
        "Keep the service layered.",
        vec!["src/**".into()],
        "active",
    );
    let decision_id = decision.frontmatter.id.clone();
    decision.path = paths.object_dir("decisions").join("base-architecture.md");
    decision.save().unwrap();
    git_commit(root, "base decision");
    let base_commit = git_rev_parse(root, "HEAD");

    decision.body = "Keep the service layered and dependency direction clear.".into();
    decision.save().unwrap();
    git_commit(root, "update base decision");

    decision.frontmatter.status = "stale".into();
    decision.save().unwrap();
    git_commit(root, "mark base decision stale");
    let stale_commit = git_rev_parse(root, "HEAD");

    decision.frontmatter.status = "active".into();
    decision.save().unwrap();
    git_commit(root, "reactivate base decision");

    let mut howto = ctx::object::new_object(
        ctx::object::ObjectType::Howto,
        "Architecture howto",
        &format!("Follow decision {decision_id}."),
        vec!["src/**".into()],
        "active",
    );
    howto.frontmatter.evidence.push(decision_id.clone());
    howto.path = paths.object_dir("howtos").join("architecture-howto.md");
    howto.save().unwrap();
    git_commit(root, "dependent howto");

    let blame = version::blame(&paths, "Base architecture").unwrap();
    assert_eq!(blame.objects, vec![decision_id.clone()]);
    assert!(blame.git.len() >= 4);
    assert_eq!(blame.git[0].object_id.as_str(), decision_id.as_str());

    let log = version::log_scope(&paths, None).unwrap();
    assert!(log.iter().any(|entry| entry.object_type == "git_commit"));

    let status_diff = version::diff(&paths, None, Some(&base_commit), Some(&stale_commit)).unwrap();
    assert!(status_diff.status_changes.iter().any(|change| {
        change.id == decision_id
            && change.from.as_deref() == Some("active")
            && change.to.as_deref() == Some("stale")
    }));

    let diff = version::diff(&paths, None, Some(&base_commit), Some("HEAD")).unwrap();
    assert!(diff
        .git_changes
        .iter()
        .any(|change| change.path.contains("architecture-howto.md")));

    version::revert(&paths, &decision_id).unwrap();
    let objects = ctx::object::load_all_objects(&paths.objects).unwrap();
    let reverted = objects
        .iter()
        .find(|obj| obj.frontmatter.id == decision_id)
        .unwrap();
    assert_eq!(reverted.frontmatter.status, "deprecated");
    let dependent = objects
        .iter()
        .find(|obj| obj.frontmatter.title == "Architecture howto")
        .unwrap();
    assert_eq!(dependent.frontmatter.status, "conflicted");
    let events = ctx::event::read_events(&paths.events).unwrap();
    assert!(events.iter().any(|event| event.event_type == "revert"));
}

#[test]
fn repeatbench_runner_reports_expected_gate_hit() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let tmp = TempDir::new().unwrap();
    let output = tmp.path().join("repeatbench-results.jsonl");
    let report = repeatbench::run_with_output(
        root,
        &root.join("benchmarks/repeatbench/scenarios"),
        Some(&output),
    )
    .unwrap();
    assert_eq!(report.scenarios, 1);
    assert_eq!(report.gate_hits, 1);
    assert_eq!(report.misses, 0);
    assert_eq!(report.results[0].actual_hit_ids, vec!["c-repeat01"]);
    assert_eq!(report.results[0].hook_permission, "deny");
    assert_eq!(report.results[0].false_positive_misses, 1);
    let jsonl = fs::read_to_string(output).unwrap();
    assert!(jsonl.contains("\"id\":\"npm-install-repeat\""));
}

fn sha12(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())[..12].to_string()
}

fn ctx_bin() -> &'static str {
    env!("CARGO_BIN_EXE_ctx")
}

fn shell_quote(raw: &str) -> String {
    format!("'{}'", raw.replace('\'', "'\\''"))
}

fn path_with_ctx_bin() -> String {
    let bin_dir = std::path::Path::new(ctx_bin()).parent().unwrap();
    let old_path = std::env::var("PATH").unwrap_or_default();
    format!("{}:{old_path}", bin_dir.display())
}

fn copy_dir(src: &std::path::Path, dst: &std::path::Path) {
    if dst.exists() {
        fs::remove_dir_all(dst).unwrap();
    }
    fs::create_dir_all(dst).unwrap();
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.unwrap();
        let rel = entry.path().strip_prefix(src).unwrap();
        if rel.as_os_str().is_empty() {
            continue;
        }
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&target).unwrap();
        } else {
            fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn git_commit(root: &std::path::Path, message: &str) {
    Command::new("git")
        .args(["add", "."])
        .current_dir(root)
        .output()
        .unwrap();
    let output = Command::new("git")
        .args(["commit", "-m", message])
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "t@t.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "t@t.com")
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_rev_parse(root: &std::path::Path, rev: &str) -> String {
    let output = Command::new("git")
        .args(["rev-parse", rev])
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}
