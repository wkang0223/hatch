use anyhow::Result;
use nm_common::config::AgentConfig;
use std::path::Path;
use tracing::info;

pub fn load_or_create(path: &Path) -> Result<AgentConfig> {
    if path.exists() {
        let content = std::fs::read_to_string(path)?;
        let cfg: AgentConfig = toml::from_str(&content)
            .map_err(|e| anyhow::anyhow!("Config parse error in {}: {}", path.display(), e))?;
        Ok(cfg)
    } else {
        // Create default config
        let cfg = AgentConfig::default();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let toml_str = toml::to_string_pretty(&cfg)?;
        std::fs::write(path, &toml_str)?;
        info!(path = %path.display(), "Created default agent config");
        Ok(cfg)
    }
}
