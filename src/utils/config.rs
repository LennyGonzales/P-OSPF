use crate::error::ProtocolError;
use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub bind_address: String,
    pub broadcast_address: String,
    pub router_id: String,
    pub hello_interval: u64,
    pub dead_interval: u64,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            bind_address: "0.0.0.0:8080".to_string(),
            broadcast_address: "255.255.255.255:8080".to_string(),
            router_id: uuid::Uuid::new_v4().to_string(),
            hello_interval: 30,
            dead_interval: 120,
        }
    }
}

impl Config {
    pub fn load_from_file(path: &str) -> Result<Self, ProtocolError> {
        let contents = fs::read_to_string(path).map_err(|e| ProtocolError::Io(e))?;

        let config: Config = serde_json::from_str(&contents)?;
        Ok(config)
    }

    pub fn save_to_file(&self, path: &str) -> Result<(), ProtocolError> {
        let contents = serde_json::to_string_pretty(self)?;
        fs::write(path, contents).map_err(|e| ProtocolError::Io(e))?;
        Ok(())
    }
}