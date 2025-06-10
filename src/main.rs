// This is the entry point of the application for the Rust routing protocol.
// It initializes the routing protocol, sets up the client and server components,
// and manages the overall execution flow.

mod client;
mod server;
mod core;
mod protocol;
mod utils;
mod error;

use log::{info, error};
use std::env;
use rust_routing_protocol::server::protocol_server::ProtocolServer;
use rust_routing_protocol::utils::{init_logger, Config};
use rust_routing_protocol::error::ProtocolError;

#[tokio::main]
async fn main() -> Result<(), ProtocolError> {
    init_logger();
    
    let config = Config::load_from_file("config.json")
        .unwrap_or_else(|_| {
            info!("Using default configuration");
            Config::default()
        });
    
    info!("Starting Rust Routing Protocol");
    info!("Router ID: {}", config.router_id);
    info!("Bind address: {}", config.bind_address);
    info!("Broadcast address: {}", config.broadcast_address);
    
    let server = ProtocolServer::new(&config.bind_address, &config.broadcast_address).await?;
    
    if let Err(e) = server.start().await {
        error!("Server error: {}", e);
        return Err(e);
    }
    
    Ok(())
}