use serde::Deserialize;
use std::fs;

#[derive(Debug, Deserialize)]
pub struct Interface {
    pub name: String,
    pub capacity_mbps: u32,
}

#[derive(Debug, Deserialize)]
pub struct Config {
    pub interfaces: Vec<Interface>,
}

pub fn read_interfaces_config(path: &str) -> Result<Config, Box<dyn std::error::Error>> {
    let config_str = fs::read_to_string(path)?;
    let config: Config = toml::from_str(&config_str)?;
    Ok(config)
}
