use crate::paths::ContextPaths;
use anyhow::Result;
use std::fs;
use std::path::PathBuf;

const PRECOMMIT_BEGIN: &str = "# ctx:begin pre-commit";
const PRECOMMIT_END: &str = "# ctx:end pre-commit";
const POSTMERGE_BEGIN: &str = "# ctx:begin post-merge";
const POSTMERGE_END: &str = "# ctx:end post-merge";

pub fn install_all(paths: &ContextPaths) -> Result<()> {
    install_claude(paths)?;
    install_cursor(paths)?;
    install_codex(paths)?;
    install_git_hooks(paths)?;
    install_mcp_config(paths)?;
    Ok(())
}

fn hook_script() -> &'static str {
    r#"#!/usr/bin/env bash
set -euo pipefail
# ContextVC hook adapter — fail-open except explicit block
INPUT=$(cat)
CTX_BIN="${CTX_BIN:-ctx}"
export CTX_REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
cd "$CTX_REPO_ROOT" || exit 0

EVENT="${CTX_HOOK_EVENT:-SessionStart}"
AGENT="${CTX_HOOK_AGENT:-unknown}"
fail_closed_event() {
  case "$EVENT" in
    PreToolUse|preToolUse|beforeShellExecution|beforeMCPExecution) [[ "$AGENT" == "cursor" || "$AGENT" == "codex" ]] ;;
    *) return 1 ;;
  esac
}
deny_json() {
  printf '{"permissionDecision":"deny","permission":"deny","permissionDecisionReason":"ContextVC gate unavailable in fail-closed hook","agent_message":"ContextVC gate unavailable in fail-closed hook"}\n'
}
if ! command -v "$CTX_BIN" >/dev/null 2>&1; then
  if fail_closed_event; then deny_json; exit 1; fi
  exit 0
fi
if [[ ! -f .context/VERSION ]]; then
  if fail_closed_event; then deny_json; exit 1; fi
  exit 0
fi

case "$EVENT" in
  SessionStart|sessionStart)
    "$CTX_BIN" hook session-start <<< "$INPUT" || true
    ;;
  PreToolUse|preToolUse|beforeShellExecution|beforeMCPExecution)
    "$CTX_BIN" hook pre-tool <<< "$INPUT" || { if fail_closed_event; then deny_json; exit 1; fi; true; }
    ;;
  PostToolUse|postToolUse|afterFileEdit)
    "$CTX_BIN" hook post-tool <<< "$INPUT" || true
    ;;
  UserPromptSubmit|userPromptSubmit)
    "$CTX_BIN" hook user-prompt-submit <<< "$INPUT" || true
    ;;
  PreCompact|preCompact)
    "$CTX_BIN" hook pre-compact <<< "$INPUT" || true
    ;;
  Stop|stop)
    "$CTX_BIN" hook stop <<< "$INPUT" || true
    ;;
  *)
    exit 0
    ;;
esac
"#
}

fn write_hook_script(path: PathBuf) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, hook_script())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

pub fn install_claude(paths: &ContextPaths) -> Result<()> {
    let dir = paths.root.join(".claude");
    fs::create_dir_all(&dir)?;
    let hook_path = dir.join("contextvc-hook.sh");
    write_hook_script(hook_path.clone())?;

    let settings_path = dir.join("settings.json");
    let cmd = shell_quote_path(&hook_path);
    let settings = serde_json::json!({
        "hooks": {
            "SessionStart": [{ "matcher": "", "hooks": [{ "type": "command", "command": format!("CTX_HOOK_AGENT=claude CTX_HOOK_EVENT=SessionStart {cmd}") }] }],
            "PreToolUse": [{ "matcher": "Edit|Write|Bash", "hooks": [{ "type": "command", "command": format!("CTX_HOOK_AGENT=claude CTX_HOOK_EVENT=PreToolUse {cmd}") }] }],
            "PostToolUse": [{ "matcher": "", "hooks": [{ "type": "command", "command": format!("CTX_HOOK_AGENT=claude CTX_HOOK_EVENT=PostToolUse {cmd}") }] }],
            "UserPromptSubmit": [{ "matcher": "", "hooks": [{ "type": "command", "command": format!("CTX_HOOK_AGENT=claude CTX_HOOK_EVENT=UserPromptSubmit {cmd}") }] }],
            "PreCompact": [{ "matcher": "", "hooks": [{ "type": "command", "command": format!("CTX_HOOK_AGENT=claude CTX_HOOK_EVENT=PreCompact {cmd}") }] }],
            "Stop": [{ "matcher": "", "hooks": [{ "type": "command", "command": format!("CTX_HOOK_AGENT=claude CTX_HOOK_EVENT=Stop {cmd}") }] }]
        }
    });
    merge_json_file(&settings_path, settings)?;
    Ok(())
}

pub fn install_cursor(paths: &ContextPaths) -> Result<()> {
    let hook_path = paths.root.join(".cursor").join("contextvc-hook.sh");
    write_hook_script(hook_path.clone())?;
    let hooks_json = paths.root.join(".cursor").join("hooks.json");
    let cmd = shell_quote_path(&hook_path);
    let config = serde_json::json!({
        "version": 1,
        "hooks": {
            "sessionStart": [{ "command": format!("CTX_HOOK_AGENT=cursor CTX_HOOK_EVENT=sessionStart {cmd}") }],
            "preToolUse": [{ "command": format!("CTX_HOOK_AGENT=cursor CTX_HOOK_EVENT=preToolUse {cmd}"), "failClosed": true }],
            "beforeShellExecution": [{ "command": format!("CTX_HOOK_AGENT=cursor CTX_HOOK_EVENT=beforeShellExecution {cmd}"), "failClosed": true }],
            "beforeMCPExecution": [{ "command": format!("CTX_HOOK_AGENT=cursor CTX_HOOK_EVENT=beforeMCPExecution {cmd}"), "failClosed": true }],
            "postToolUse": [{ "command": format!("CTX_HOOK_AGENT=cursor CTX_HOOK_EVENT=postToolUse {cmd}") }],
            "afterFileEdit": [{ "command": format!("CTX_HOOK_AGENT=cursor CTX_HOOK_EVENT=afterFileEdit {cmd}") }],
            "userPromptSubmit": [{ "command": format!("CTX_HOOK_AGENT=cursor CTX_HOOK_EVENT=userPromptSubmit {cmd}") }],
            "preCompact": [{ "command": format!("CTX_HOOK_AGENT=cursor CTX_HOOK_EVENT=preCompact {cmd}") }],
            "stop": [{ "command": format!("CTX_HOOK_AGENT=cursor CTX_HOOK_EVENT=stop {cmd}") }]
        }
    });
    merge_json_file(&hooks_json, config)?;
    Ok(())
}

pub fn install_codex(paths: &ContextPaths) -> Result<()> {
    let hook_path = paths.root.join(".codex").join("contextvc-hook.sh");
    write_hook_script(hook_path.clone())?;
    let hooks_json = paths.root.join(".codex").join("hooks.json");
    let cmd = shell_quote_path(&hook_path);
    let config = serde_json::json!({
        "hooks": {
            "SessionStart": [{ "command": format!("CTX_HOOK_AGENT=codex CTX_HOOK_EVENT=SessionStart {cmd}") }],
            "PreToolUse": [{ "command": format!("CTX_HOOK_AGENT=codex CTX_HOOK_EVENT=PreToolUse {cmd}") }],
            "PostToolUse": [{ "command": format!("CTX_HOOK_AGENT=codex CTX_HOOK_EVENT=PostToolUse {cmd}") }],
            "UserPromptSubmit": [{ "command": format!("CTX_HOOK_AGENT=codex CTX_HOOK_EVENT=UserPromptSubmit {cmd}") }],
            "PreCompact": [{ "command": format!("CTX_HOOK_AGENT=codex CTX_HOOK_EVENT=PreCompact {cmd}") }],
            "Stop": [{ "command": format!("CTX_HOOK_AGENT=codex CTX_HOOK_EVENT=Stop {cmd}") }]
        }
    });
    merge_json_file(&hooks_json, config)?;
    Ok(())
}

pub fn install_git_hooks(paths: &ContextPaths) -> Result<()> {
    if !crate::is_git_repo(&paths.root) {
        return Ok(());
    }
    let hook_dir = paths.root.join(".git").join("hooks");
    fs::create_dir_all(&hook_dir)?;
    let hook = hook_dir.join("pre-commit");
    if let Some(parent) = hook.parent() {
        fs::create_dir_all(parent)?;
    }
    let existing = fs::read_to_string(&hook).unwrap_or_default();
    let script = merge_precommit_hook(&existing, &precommit_block());
    write_executable_hook(&hook, script)?;

    let post_merge = hook_dir.join("post-merge");
    let existing = fs::read_to_string(&post_merge).unwrap_or_default();
    let script = merge_postmerge_hook(&existing, &postmerge_block());
    write_executable_hook(&post_merge, script)?;
    Ok(())
}

fn precommit_block() -> String {
    format!(
        r#"{PRECOMMIT_BEGIN}
if command -v ctx >/dev/null 2>&1 && [[ -f .context/VERSION ]]; then
  ctx check || {{
    echo "ContextVC check failed — run ctx render or ctx adopt"
    exit 1
  }}
fi
"#
    )
}

fn postmerge_block() -> String {
    format!(
        r#"{POSTMERGE_BEGIN}
if command -v ctx >/dev/null 2>&1 && [[ -f .context/VERSION ]]; then
  ctx merge || exit 1
  ctx verify --mark || exit 1
  ctx check || exit 1
fi
"#
    )
}

fn write_executable_hook(path: &std::path::Path, script: String) -> Result<()> {
    fs::write(path, script)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn merge_precommit_hook(existing: &str, block: &str) -> String {
    let block = format!("{}\n{PRECOMMIT_END}\n", block.trim_end());
    if existing.trim().is_empty() {
        return format!("#!/usr/bin/env bash\n\n{block}");
    }

    if let Some(start) = existing.find(PRECOMMIT_BEGIN) {
        if let Some(relative_end) = existing[start..].find(PRECOMMIT_END) {
            let end = start + relative_end + PRECOMMIT_END.len();
            let before = existing[..start].trim_end();
            let after = existing[end..].trim_start_matches(['\r', '\n']);
            return if after.is_empty() {
                format!("{before}\n\n{block}")
            } else {
                format!("{before}\n\n{block}\n{after}")
            };
        }
    }

    if let Some((shebang, rest)) = existing
        .split_once('\n')
        .filter(|(line, _)| line.starts_with("#!"))
    {
        return format!(
            "{shebang}\n\n{block}\n{}",
            rest.trim_start_matches(['\r', '\n'])
        );
    }

    format!(
        "#!/usr/bin/env bash\n\n{block}\n{}",
        existing.trim_start_matches(['\r', '\n'])
    )
}

fn merge_postmerge_hook(existing: &str, block: &str) -> String {
    merge_shell_hook_block(existing, block, POSTMERGE_BEGIN, POSTMERGE_END)
}

fn merge_shell_hook_block(existing: &str, block: &str, begin: &str, end_marker: &str) -> String {
    let block = format!("{}\n{end_marker}\n", block.trim_end());
    if existing.trim().is_empty() {
        return format!("#!/usr/bin/env bash\n\n{block}");
    }

    if let Some(start) = existing.find(begin) {
        if let Some(relative_end) = existing[start..].find(end_marker) {
            let end = start + relative_end + end_marker.len();
            let before = existing[..start].trim_end();
            let after = existing[end..].trim_start_matches(['\r', '\n']);
            return if after.is_empty() {
                format!("{before}\n\n{block}")
            } else {
                format!("{before}\n\n{block}\n{after}")
            };
        }
    }

    if let Some((shebang, rest)) = existing
        .split_once('\n')
        .filter(|(line, _)| line.starts_with("#!"))
    {
        return format!(
            "{shebang}\n\n{block}\n{}",
            rest.trim_start_matches(['\r', '\n'])
        );
    }

    format!(
        "#!/usr/bin/env bash\n\n{block}\n{}",
        existing.trim_start_matches(['\r', '\n'])
    )
}

pub fn install_mcp_config(paths: &ContextPaths) -> Result<()> {
    let mcp = paths.root.join(".mcp.json");
    let entry = serde_json::json!({
        "mcpServers": {
            "contextvc": {
                "command": "ctx",
                "args": ["serve-mcp"],
                "env": {
                    "CTX_REPO_ROOT": paths.root.to_string_lossy()
                }
            }
        }
    });
    merge_json_file(&mcp, entry.clone())?;

    let claude_mcp = paths.root.join(".claude").join(".mcp.json");
    merge_json_file(&claude_mcp, entry.clone())?;
    Ok(())
}

fn merge_json_file(path: &std::path::Path, new_value: serde_json::Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut existing = if path.exists() {
        serde_json::from_str::<serde_json::Value>(&fs::read_to_string(path)?)?
    } else {
        serde_json::json!({})
    };
    deep_merge(&mut existing, new_value);
    fs::write(path, serde_json::to_string_pretty(&existing)?)?;
    Ok(())
}

fn deep_merge(base: &mut serde_json::Value, overlay: serde_json::Value) {
    match (base, overlay) {
        (serde_json::Value::Object(a), serde_json::Value::Object(b)) => {
            for (k, v) in b {
                deep_merge(a.entry(k).or_insert(serde_json::Value::Null), v);
            }
        }
        (serde_json::Value::Array(a), serde_json::Value::Array(b)) => {
            if b.iter().all(|item| item.is_object()) {
                for item in b {
                    if !a.contains(&item) {
                        a.push(item);
                    }
                }
            } else {
                *a = b;
            }
        }
        (a, b) => *a = b,
    }
}

fn shell_quote_path(path: &std::path::Path) -> String {
    let raw = path.to_string_lossy();
    format!("'{}'", raw.replace('\'', "'\\''"))
}
