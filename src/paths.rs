use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ContextPaths {
    pub root: PathBuf,
    pub context: PathBuf,
    pub objects: PathBuf,
    pub events: PathBuf,
    pub proposals: PathBuf,
    pub cache: PathBuf,
    pub config_file: PathBuf,
    pub render_lock: PathBuf,
    pub version_file: PathBuf,
}

impl ContextPaths {
    pub fn new(root: impl AsRef<Path>) -> Self {
        let root = root.as_ref().to_path_buf();
        let context = root.join(".context");
        Self {
            objects: context.join("objects"),
            events: context.join("events"),
            proposals: context.join("proposals"),
            cache: context.join(".cache"),
            config_file: context.join("config.yaml"),
            render_lock: context.join("render.lock"),
            version_file: context.join("VERSION"),
            context,
            root,
        }
    }

    pub fn object_dir(&self, object_type: &str) -> PathBuf {
        self.objects.join(object_type)
    }

    pub fn agents_md(&self) -> PathBuf {
        self.root.join("AGENTS.md")
    }

    pub fn claude_md(&self) -> PathBuf {
        self.root.join("CLAUDE.md")
    }

    pub fn cursor_rules_dir(&self) -> PathBuf {
        self.root.join(".cursor").join("rules")
    }

    pub fn copilot_instructions(&self) -> PathBuf {
        self.root.join(".github").join("copilot-instructions.md")
    }

    pub fn gemini_md(&self) -> PathBuf {
        self.root.join("GEMINI.md")
    }

    pub fn cline_memory_bank(&self) -> PathBuf {
        self.root
            .join(".cline")
            .join("memory-bank")
            .join("contextvc.md")
    }
}
