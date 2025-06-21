// Module de lecture de configuration basé sur le hostname

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use crate::error::{AppError, Result};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct InterfaceConfig {
    pub name: String,
    pub capacity_mbps: u32,
    #[serde(default = "default_link_active")]
    pub link_active: bool,
}

fn default_link_active() -> bool {
    true
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RouterConfig {
    #[serde(default)]
    pub interfaces: Vec<InterfaceConfig>,
}

/// Lit la configuration du routeur basée sur le hostname
pub fn read_router_config() -> Result<RouterConfig> {
    let hostname = get_hostname()?;
    let config_path = format!("src/conf/config_{}.toml", hostname);
    
    if !Path::new(&config_path).exists() {
        return Err(AppError::ConfigError(format!(
            "Config file not found: {}. Available configs: {}",
            config_path,
            list_available_configs()
        )));
    }
    
    let config_content = fs::read_to_string(&config_path)
        .map_err(|e| AppError::ConfigError(format!("Failed to read config file {}: {}", config_path, e)))?;
    
    let config: RouterConfig = toml::from_str(&config_content)
        .map_err(|e| AppError::ConfigError(format!("Failed to parse config file {}: {}", config_path, e)))?;
    
    log::info!("Loaded configuration for router: {}", hostname);
    log::debug!("Config: {:?}", config);
    
    Ok(config)
}

/// Obtient le hostname de la machine
fn get_hostname() -> Result<String> {
    hostname::get()
        .map_err(|e| AppError::ConfigError(format!("Failed to get hostname: {}", e)))?
        .to_string_lossy()
        .to_string()
        .split('.')
        .next()
        .ok_or_else(|| AppError::ConfigError("Invalid hostname".to_string()))
        .map(|s| s.to_string())
}

/// Liste les fichiers de configuration disponibles
fn list_available_configs() -> String {
    let config_dir = "src/conf";
    if let Ok(entries) = fs::read_dir(config_dir) {
        let configs: Vec<String> = entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.extension()? == "toml" {
                    path.file_name()?.to_str().map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect();
        configs.join(", ")
    } else {
        "Unable to list config directory".to_string()
    }
}
