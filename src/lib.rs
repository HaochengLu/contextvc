pub mod adopt;
pub mod backfill;
pub mod check;
pub mod compiler;
pub mod config;
pub mod distill;
pub mod doctor;
pub mod event;
pub mod gate;
pub mod harvest;
pub mod hooks;
pub mod index;
pub mod init;
pub mod lockfile;
pub mod matchers;
pub mod mcp;
pub mod object;
pub mod paths;
pub mod redact;
pub mod render;
pub mod repeatbench;
pub mod review;
pub mod status;
pub mod verify;
pub mod version;

pub use paths::ContextPaths;

use anyhow::{Context, Result};
use std::path::Path;

pub const OCL_VERSION: &str = "0";
pub const MANAGED_BEGIN: &str = "<!-- ctx:begin";
pub const MANAGED_END: &str = "<!-- ctx:end -->";

pub fn find_repo_root(start: &Path) -> Result<std::path::PathBuf> {
    let mut current = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    loop {
        if current.join(".context").join("VERSION").exists() {
            return Ok(current);
        }
        if !current.pop() {
            anyhow::bail!("not a ContextVC repo (no .context/VERSION found)");
        }
    }
}

pub fn ensure_repo(root: &Path) -> Result<ContextPaths> {
    let paths = ContextPaths::new(root);
    if !paths.version_file.exists() {
        anyhow::bail!("not initialized: run `ctx init` in {}", root.display());
    }
    Ok(paths)
}

pub fn run_git(args: &[&str], cwd: &Path) -> Result<String> {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .context("failed to run git")?;
    if !output.status.success() {
        anyhow::bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn is_git_repo(path: &Path) -> bool {
    path.join(".git").exists() || run_git(&["rev-parse", "--git-dir"], path).is_ok()
}
