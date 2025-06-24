use crate::types::HelloMessage;
use crate::error::{AppError, Result};
use tokio::net::UdpSocket;
use log::info;
use std::net::SocketAddr;

/// Envoie un message Hello pour découvrir des voisins
pub async fn send_hello(socket: &UdpSocket, addr: &SocketAddr, router_ip: &str, key: &[u8]) -> Result<()> {
    let message = HelloMessage {
        message_type: 1,
        router_ip: router_ip.to_string(),
    };
    crate::net_utils::send_message(socket, addr, &message, key, "[SEND] HELLO").await
}
