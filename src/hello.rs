use crate::types::HelloMessage;
use crate::error::{AppError, Result};
use tokio::net::UdpSocket;
use log::info;
use std::net::SocketAddr;

/// Envoie un message Hello pour découvrir des voisins
pub async fn send_hello(socket: &UdpSocket, addr: &SocketAddr, router_ip: &str) -> Result<()> {
    let message = HelloMessage {
        message_type: 1,
        router_ip: router_ip.to_string(),
    };
    let serialized = serde_json::to_vec(&message).map_err(AppError::from)?;
    socket.send_to(&serialized, addr).await.map_err(AppError::from)?;
    info!("[SEND] HELLO from {} to {}", router_ip, addr);
    Ok(())
}
