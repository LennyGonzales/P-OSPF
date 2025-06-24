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

/// Récupère tous les réseaux IP locaux avec leurs interfaces
pub fn get_local_networks() -> Result<HashMap<String, (String, IpNetwork)>> {
    let mut networks = HashMap::new();
    let interfaces = datalink::interfaces();
    
    for interface in interfaces {
        if interface.is_up() && !interface.is_loopback() {
            for ip_network in interface.ips {
                if let IpNetwork::V4(ipv4_network) = ip_network {
                    let network_addr = ipv4_network.network();
                    let prefix_len = ipv4_network.prefix();
                    let network_key = format!("{}/{}", network_addr, prefix_len);
                    
                    networks.insert(
                        network_key,
                        (interface.name.clone(), ip_network)
                    );
                }
            }
        }
    }
    
    if networks.is_empty() {
        return Err(AppError::NetworkError("Aucun réseau local trouvé".to_string()));
    }
    
    Ok(networks)
}

/// Vérifie si une adresse IP appartient à un réseau local
pub fn is_ip_in_local_network(ip: &IpAddr, networks: &HashMap<String, (String, IpNetwork)>) -> Option<String> {
    if let IpAddr::V4(ipv4) = ip {
        for (network_key, (_, ip_network)) in networks {
            if let IpNetwork::V4(ipv4_network) = ip_network {
                if ipv4_network.contains(*ipv4) {
                    return Some(network_key.clone());
                }
            }
        }
    }
    None
}

/// Obtient la liste des routeurs voisins directement connectés
pub fn get_directly_connected_routers(
    neighbors: &HashMap<String, crate::types::Neighbor>
) -> Vec<(String, bool)> {
    neighbors.iter()
        .map(|(ip, neighbor)| (ip.clone(), neighbor.link_up))
        .collect()
}

/// Calcule le score d'un chemin basé sur le nombre de sauts et l'état des liens
pub fn calculate_path_score(path: &[String], neighbors: &HashMap<String, crate::types::Neighbor>) -> u32 {
    let mut score = path.len() as u32; // Base : nombre de sauts
    
    // Vérifier que tous les liens dans le chemin sont actifs
    for i in 0..path.len().saturating_sub(1) {
        let current_router = &path[i];
        let next_router = &path[i + 1];
        
        // Si c'est un voisin direct, vérifier l'état du lien
        if let Some(neighbor) = neighbors.get(next_router) {
            if !neighbor.link_up {
                return u32::MAX; // Chemin impossible
            }
        }
    }
    
    score
}
