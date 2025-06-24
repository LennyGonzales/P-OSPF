// Fonctions utilitaires réseau et helpers

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use pnet::datalink::{self, NetworkInterface};
use pnet::ipnetwork::IpNetwork;
use crate::error::{AppError, Result};
use openssl::symm::{Cipher, Crypter, Mode};
use openssl::rand::rand_bytes;

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
    key: &[u8],
    log_prefix: &str
) -> crate::error::Result<()> {
    let serialized = serde_json::to_vec(message)
        .map_err(|e| crate::error::AppError::SerializationError(e))?;

    let encrypted = encrypt(&serialized, key)?;

    socket.send_to(&encrypted, addr).await
        .map_err(|e| crate::error::AppError::NetworkError(format!("Failed to send message: {}", e)))?;

    log::info!("{} Encrypted message sent to {}", log_prefix, addr);
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

/// Chiffre les données en utilisant AES-256-CBC et génère un IV aléatoire.
///
/// # Arguments
/// * `data` - Les données en clair à chiffrer.
/// * `key` - La clé de 32 octets.
///
/// # Returns
/// * `Result<(Vec<u8>, Vec<u8>)>` - Un tuple contenant l'IV et les données chiffrées.
pub fn encrypt(data: &[u8], key: &[u8]) -> Result<Vec<u8>> {
    let cipher = Cipher::aes_256_cbc();
    let mut iv = vec![0u8; cipher.iv_len().unwrap_or(16)];
    rand_bytes(&mut iv).map_err(|e| AppError::CryptoError(e.to_string()))?;

    let mut crypter = Crypter::new(cipher, Mode::Encrypt, key, Some(&iv))
        .map_err(|e| AppError::CryptoError(e.to_string()))?;

    let mut ciphertext = vec![0; data.len() + cipher.block_size()];
    let mut count = crypter.update(data, &mut ciphertext)
        .map_err(|e| AppError::CryptoError(e.to_string()))?;
    count += crypter.finalize(&mut ciphertext[count..])
        .map_err(|e| AppError::CryptoError(e.to_string()))?;

    ciphertext.truncate(count);

    // Préfixer l'IV au ciphertext
    let mut result = iv;
    result.extend_from_slice(&ciphertext);
    Ok(result)
}

/// Déchiffre les données en utilisant AES-256-CBC.
///
/// # Arguments
/// * `ciphertext` - Les données chiffrées à déchiffrer (IV préfixé).
/// * `key` - La clé de 32 octets.
///
/// # Returns
/// * `Result<Vec<u8>>` - Les données en clair.
pub fn decrypt(ciphertext: &[u8], key: &[u8]) -> Result<Vec<u8>> {
    let cipher = Cipher::aes_256_cbc();
    let iv_len = cipher.iv_len().unwrap_or(16);

    if ciphertext.len() < iv_len {
        return Err(AppError::CryptoError("Ciphertext too short to contain IV".to_string()));
    }

    let (iv, ciphertext) = ciphertext.split_at(iv_len);

    let mut crypter = Crypter::new(cipher, Mode::Decrypt, key, Some(iv))
        .map_err(|e| AppError::CryptoError(e.to_string()))?;

    let mut plaintext = vec![0; ciphertext.len() + cipher.block_size()];
    let mut count = crypter.update(ciphertext, &mut plaintext)
        .map_err(|e| AppError::CryptoError(e.to_string()))?;
    count += crypter.finalize(&mut plaintext[count..])
        .map_err(|e| AppError::CryptoError(e.to_string()))?;

    plaintext.truncate(count);
    Ok(plaintext)
}
