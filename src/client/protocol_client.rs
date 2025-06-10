// This file implements the client-side logic for emitting requests to the server,
// including functions to initiate communication and handle responses.

use tokio::net::UdpSocket;
use std::net::SocketAddr;
use std::sync::Arc;
use crate::protocol::message_types::*;
use crate::error::ProtocolError;
use log::{info, error};

pub struct ProtocolClient {
    socket: Arc<UdpSocket>,
    server_addr: SocketAddr,
}

impl ProtocolClient {
    pub async fn new(server_addr: &str) -> Result<Self, ProtocolError> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        let server_addr: SocketAddr = server_addr.parse()?;
        
        Ok(Self {
            socket: Arc::new(socket),
            server_addr,
        })
    }
    
    pub async fn send_route_request(&self, destination: String) -> Result<(), ProtocolError> {
        let request = ProtocolMessage::RouteRequest(RouteRequest {
            destination,
            source: "client".to_string(),
            request_id: 1,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        });
        
        let msg_str = serde_json::to_string(&request)?;
        self.socket.send_to(msg_str.as_bytes(), self.server_addr).await?;
        
        info!("Sent route request for destination: {}", request.destination);
        Ok(())
    }
}