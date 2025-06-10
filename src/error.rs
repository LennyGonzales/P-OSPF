// This file defines custom error types and handling mechanisms for the application.

use std::fmt;
use thiserror::Error;

#[derive(Debug)]
pub enum RoutingError {
    NetworkError(String),
    ConfigurationError(String),
    PathCalculationError(String),
    NeighborDiscoveryError(String),
    Other(String),
}

impl fmt::Display for RoutingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RoutingError::NetworkError(msg) => write!(f, "Network Error: {}", msg),
            RoutingError::ConfigurationError(msg) => write!(f, "Configuration Error: {}", msg),
            RoutingError::PathCalculationError(msg) => write!(f, "Path Calculation Error: {}", msg),
            RoutingError::NeighborDiscoveryError(msg) => write!(f, "Neighbor Discovery Error: {}", msg),
            RoutingError::Other(msg) => write!(f, "Error: {}", msg),
        }
    }
}

impl std::error::Error for RoutingError {}

#[derive(Error, Debug)]
pub enum ProtocolError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),
    
    #[error("Address parse error: {0}")]
    AddrParse(#[from] std::net::AddrParseError),
    
    #[error("Network interface error: {0}")]
    NetworkInterface(String),
    
    #[error("Routing table error: {0}")]
    RoutingTable(String),
    
    #[error("Neighbor discovery error: {0}")]
    NeighborDiscovery(String),
    
    #[error("Protocol error: {0}")]
    Protocol(String),
}