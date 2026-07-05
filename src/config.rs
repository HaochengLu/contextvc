use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub version: u32,
    pub targets: Vec<String>,
    pub resident_token_budget: u32,
    pub gate: GateConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GateConfig {
    pub default_enforcement: String,
    pub fail_closed_for_block: bool,
}

impl Default for ProjectConfig {
    fn default() -> Self {
        Self {
            version: 0,
            targets: vec![
                "agents_md".into(),
                "claude_md".into(),
                "cursor_mdc".into(),
                "copilot_instructions".into(),
                "gemini_md".into(),
                "cline_memory_bank".into(),
            ],
            resident_token_budget: 1200,
            gate: GateConfig {
                default_enforcement: "warn".into(),
                fail_closed_for_block: true,
            },
        }
    }
}

impl ProjectConfig {
    pub fn load(path: &std::path::Path) -> anyhow::Result<Self> {
        let raw = std::fs::read_to_string(path)?;
        Ok(serde_yaml::from_str(&raw)?)
    }

    pub fn save(&self, path: &std::path::Path) -> anyhow::Result<()> {
        let raw = serde_yaml::to_string(self)?;
        std::fs::write(path, raw)?;
        Ok(())
    }
}
