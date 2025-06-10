// This file defines custom error types and handling mechanisms for the application.

use std::fmt;

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