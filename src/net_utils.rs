// Fonctions utilitaires réseau et helpers

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use pnet::datalink::{self, NetworkInterface};
use pnet::ipnetwork::IpNetwork;
use crate::error::{AppError, Result};
use aes::Aes256;
use cbc::{Encryptor, Decryptor};
use cipher::{KeyIvInit, block_padding::Pkcs7, BlockEncryptMut, BlockDecryptMut};
use rand::{RngCore, rngs::OsRng};

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
) -> Result<()> {
    let serialized = serde_json::to_vec(message)
        .map_err(|e| AppError::SerializationError(e))?;

    let encrypted = encrypt(&serialized, key)?;

    socket.send_to(&encrypted, addr).await
        .map_err(|e| AppError::NetworkError(format!("Failed to send message: {}", e)))?;

    log::info!("{} Encrypted message sent to {}", log_prefix, addr);
    Ok(())
}

/// Chiffre les données en utilisant AES-256-CBC et génère un IV aléatoire.
///
/// # Arguments
/// * `data` - Les données en clair à chiffrer.
/// * `key` - La clé de 32 octets.
///
/// # Returns
/// * `Result<Vec<u8>>` - Les données chiffrées avec l'IV préfixé.
pub fn encrypt(data: &[u8], key: &[u8]) -> Result<Vec<u8>> {
    // Vérifier que la clé fait 32 octets (256 bits)
    if key.len() != 32 {
        return Err(AppError::CryptoError("La clé doit faire 32 octets".to_string()));
    }
    
    // Générer un IV aléatoire
    let mut iv = vec![0u8; 16]; // AES utilise toujours un bloc de 16 octets
    OsRng.fill_bytes(&mut iv);
    
    // Convertir le slice en tableau de taille fixe pour aes/cbc
    let key_array: &[u8; 32] = key.try_into()
        .map_err(|_| AppError::CryptoError("Erreur de conversion de clé".to_string()))?;
    let iv_array: &[u8; 16] = iv.as_slice().try_into()
        .map_err(|_| AppError::CryptoError("Erreur de conversion d'IV".to_string()))?;
    
    // Chiffrer les données
    let encryptor = Encryptor::<Aes256>::new(key_array.into(), iv_array.into());
    // Allouer un buffer pour les données + padding maximal possible
    let block_size = 16;
    let padding = block_size - (data.len() % block_size);
    let mut buffer = Vec::with_capacity(data.len() + padding);
    buffer.extend_from_slice(data);
    buffer.resize(data.len() + padding, 0u8);

    let ciphertext_len = encryptor
        .encrypt_padded_mut::<Pkcs7>(&mut buffer, data.len())
        .map_err(|e| AppError::CryptoError(format!("Erreur de chiffrement: {}", e)))?
        .len();

    // Préfixer l'IV au ciphertext
    let mut result = iv;
    result.extend_from_slice(&buffer[..ciphertext_len]);

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
    // Vérifier que la clé fait 32 octets (256 bits)
    if key.len() != 32 {
        return Err(AppError::CryptoError("La clé doit faire 32 octets".to_string()));
    }
    
    // Taille IV fixe pour AES
    let iv_len = 16;
    
    // Vérifier que le ciphertext est assez long
    if ciphertext.len() < iv_len {
        return Err(AppError::CryptoError("Ciphertext trop court pour contenir l'IV".to_string()));
    }
    
    // Séparer l'IV et le ciphertext
    let (iv, encrypted_data) = ciphertext.split_at(iv_len);
    
    // Convertir le slice en tableau de taille fixe pour aes/cbc
    let key_array: &[u8; 32] = key.try_into()
        .map_err(|_| AppError::CryptoError("Erreur de conversion de clé".to_string()))?;
    let iv_array: &[u8; 16] = iv.try_into()
        .map_err(|_| AppError::CryptoError("Erreur de conversion d'IV".to_string()))?;
    
    // Déchiffrer les données
    let decryptor = Decryptor::<Aes256>::new(key_array.into(), iv_array.into());
    let mut buffer = encrypted_data.to_vec();
    let decrypted = decryptor
        .decrypt_padded_mut::<Pkcs7>(&mut buffer)
        .map_err(|e| AppError::CryptoError(format!("Erreur de déchiffrement: {}", e)))?;
    
    Ok(decrypted.to_vec())
}
