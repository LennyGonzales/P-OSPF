// Fonctions utilitaires réseau et helpers

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use pnet::datalink::{self, NetworkInterface};
use pnet::ipnetwork::IpNetwork;
use crate::error::{AppError, Result};

pub fn get_broadcast_addresses(port: u16) -> Vec<(String, SocketAddr)> {
    let interfaces = datalink::interfaces();
    interfaces
        .into_iter()
        .flat_map(|iface: NetworkInterface| {
            iface.ips.into_iter().filter_map(move |ip_network| {
                if let IpAddr::V4(ip) = ip_network.ip() {
                    if !ip.is_loopback() {
                        if let IpNetwork::V4(ipv4_network) = ip_network {
                            let broadcast = ipv4_network.broadcast();
                            Some((ip.to_string(), SocketAddr::new(IpAddr::V4(broadcast), port)))
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
        })
        .collect()
}

pub fn get_local_ip() -> Result<String> {
    let interfaces = datalink::interfaces();
    for interface in interfaces {
        for ip_network in interface.ips {
            if let IpAddr::V4(ipv4) = ip_network.ip() {
                if !ipv4.is_loopback() && !ipv4.is_unspecified() {
                    return Ok(ipv4.to_string());
                }
            }
        }
    }
    Err(AppError::ConfigError("No valid IP address found".to_string()))
}

pub fn determine_receiving_interface(
    sender_ip: &IpAddr,
    local_ips: &HashMap<IpAddr, (String, IpNetwork)>,
) -> Result<(String, IpNetwork)> {
    if let IpAddr::V4(sender_ipv4) = sender_ip {
        for (local_ip, (local_ip_str, ip_network)) in local_ips {
            if let IpNetwork::V4(ipv4_network) = ip_network {
                if ipv4_network.contains(*sender_ipv4) {
                    return Ok((local_ip_str.clone(), ip_network.clone()));
                }
            }
        }
    }
    for (local_ip, (local_ip_str, ip_network)) in local_ips {
        if let IpAddr::V4(ipv4) = local_ip {
            if !ipv4.is_loopback() && !ipv4.is_unspecified() {
                return Ok((local_ip_str.clone(), ip_network.clone()));
            }
        }
    }
    Err(AppError::NetworkError("No valid receiving interface found".to_string()))
}

pub fn calculate_broadcast_for_interface(interface_ip: &str, ip_network: &IpNetwork, port: u16) -> Result<SocketAddr> {
    if let IpNetwork::V4(ipv4_network) = ip_network {
        let broadcast_addr = ipv4_network.broadcast();
        Ok(SocketAddr::new(IpAddr::V4(broadcast_addr), port))
    } else {
        Err(AppError::NetworkError("Invalid IPv4 network".to_string()))
    }
}

/// Fonction générique pour envoyer n'importe quel type de message sérialisable
/// 
/// # Arguments
/// * `socket` - Le socket UDP à utiliser pour l'envoi
/// * `addr` - L'adresse de destination
/// * `message` - Le message à envoyer (doit implémenter Serialize)
/// * `message_type` - Type du message (1: HELLO, 2: LSA, 3: Commande)
/// * `log_prefix` - Préfixe pour les logs (ex: "[SEND]", "[CLI]")
/// 
/// # Returns
/// * `Result<()>` - Ok si le message a été envoyé, Err sinon
pub async fn send_message<T: serde::Serialize>(
    socket: &tokio::net::UdpSocket,
    addr: &std::net::SocketAddr,
    message: &T,
    log_prefix: &str
) -> crate::error::Result<()> {
    let serialized = serde_json::to_vec(message)
        .map_err(|e| crate::error::AppError::SerializationError(e))?;
    
    socket.send_to(&serialized, addr).await
        .map_err(|e| crate::error::AppError::NetworkError(format!("Failed to send message: {}", e)))?;
    
    log::info!("{} Message sent to {}", log_prefix, addr);
    Ok(())
}

/// Fonction d'aide pour envoyer un message texte simple (réponses CLI)
pub async fn send_text_response(
    socket: &tokio::net::UdpSocket,
    addr: &std::net::SocketAddr,
    response: &str,
    log_context: &str
) -> crate::error::Result<()> {
    socket.send_to(response.as_bytes(), addr).await
        .map_err(|e| crate::error::AppError::NetworkError(
            format!("Failed to send {} response: {}", log_context, e)
        ))?;
    
    log::debug!("[CLI] Sent {} response to {}", log_context, addr);
    Ok(())
}
