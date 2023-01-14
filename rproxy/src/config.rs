use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cfg {
    #[serde(alias = "Apps")]
    pub apps: Vec<App>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct App {
    #[serde(alias = "Name")]
    pub name: String,
    #[serde(alias = "Ports")]
    pub ports: Vec<u16>,
    #[serde(alias = "Targets")]
    pub targets: Vec<String>,
}

impl Cfg {
    pub fn try_build(path: &PathBuf) -> anyhow::Result<Self> {
        let file = std::fs::read(&path)?;
        let config: Self = serde_json::from_slice(&file)?;
        Ok(config)
    }
}
