use crate::config::ProjectConfig;
use crate::paths::ContextPaths;
use crate::{is_git_repo, run_git, OCL_VERSION};
use anyhow::Result;
use std::fs;
use std::path::Path;

pub struct InitOptions {
    pub skip_adopt: bool,
    pub skip_backfill: bool,
    pub skip_render: bool,
}

impl Default for InitOptions {
    fn default() -> Self {
        Self {
            skip_adopt: false,
            skip_backfill: false,
            skip_render: false,
        }
    }
}

pub fn init_repo(root: &Path, opts: InitOptions) -> Result<ContextPaths> {
    let paths = ContextPaths::new(root);
    if paths.version_file.exists() {
        anyhow::bail!(".context already initialized");
    }

    fs::create_dir_all(&paths.context)?;
    for ty in crate::object::ObjectType::all() {
        fs::create_dir_all(paths.object_dir(ty.as_str()))?;
    }
    fs::create_dir_all(&paths.events)?;
    fs::create_dir_all(&paths.proposals)?;
    fs::create_dir_all(&paths.cache)?;

    ProjectConfig::default().save(&paths.config_file)?;
    write_gitignore(&paths)?;
    write_gitattributes(&paths)?;

    if !opts.skip_adopt {
        crate::adopt::adopt_existing(&paths)?;
    }
    if !opts.skip_backfill {
        crate::backfill::backfill(&paths)?;
    }
    if !opts.skip_render {
        crate::render::render_all(&paths, false)?;
    }

    fs::write(&paths.version_file, format!("{OCL_VERSION}\n"))?;

    if is_git_repo(root) {
        let _ = run_git(&["add", ".context"], root);
    }

    Ok(paths)
}

fn write_gitignore(paths: &ContextPaths) -> Result<()> {
    let content = r#".cache/
*.sqlite
sessions/
"#;
    fs::write(paths.context.join(".gitignore"), content)?;
    Ok(())
}

fn write_gitattributes(paths: &ContextPaths) -> Result<()> {
    let content = r#"events/*.jsonl merge=union
"#;
    fs::write(paths.context.join(".gitattributes"), content)?;
    Ok(())
}

pub fn install_agents(paths: &ContextPaths) -> Result<()> {
    crate::hooks::install_all(paths)?;
    Ok(())
}
