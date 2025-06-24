// Imports pour les macros de logging
use log::{info, warn, debug, error};

pub async fn main_loop(socket: std::sync::Arc<tokio::net::UdpSocket>, state: std::sync::Arc<crate::AppState>) -> crate::error::Result<()> {
    let mut buf = [0; 2048];
    let local_ips: std::collections::HashMap<std::net::IpAddr, (String, pnet::ipnetwork::IpNetwork)> = pnet::datalink::interfaces()
        .into_iter()
        .flat_map(|iface| {
            iface.ips.into_iter().filter_map(move |ip_network| {
                if let std::net::IpAddr::V4(ipv4) = ip_network.ip() {
                    if !ipv4.is_loopback() {
                        Some((std::net::IpAddr::V4(ipv4), (ipv4.to_string(), ip_network)))
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
        })
        .collect();
    loop {
        let (len, src_addr) = socket.recv_from(&mut buf).await?;
        if local_ips.contains_key(&src_addr.ip()) {
            continue;
        }
        log::debug!("Received {} bytes from {}", len, src_addr);
        let (receiving_interface_ip, receiving_network) = match crate::net_utils::determine_receiving_interface(&src_addr.ip(), &local_ips) {
            Ok((ip, network)) => (ip, network),
            Err(e) => {
                log::error!("Failed to determine receiving interface: {}", e);
                continue;
            }
        };
        log::debug!("Receiving interface IP: {}, Network: {}", receiving_interface_ip, receiving_network);
        match serde_json::from_slice::<serde_json::Value>(&buf[..len]) {
            Ok(json) => {
                if let Some(message_type) = json.get("message_type").and_then(|v| v.as_u64()) {
                    log::debug!("Received message type: {}", message_type);
                    match message_type {
                        1 => {
                            // Vérifier si le protocole OSPF est activé avant de traiter les HELLO
                            if !state.is_enabled().await {
                                debug!("OSPF disabled, ignoring HELLO message");
                                continue;
                            }
                            
                            if let Ok(hello) = serde_json::from_value::<crate::types::HelloMessage>(json) {
                                log::info!("[RECV] HELLO from {} - {} (received on interface {})", 
                                    hello.router_ip, src_addr, receiving_interface_ip);
                                crate::neighbor::update_neighbor(&state, &hello.router_ip).await;
                                // Utiliser le préfixe réseau de l'interface pour la table de routage
                                let network_prefix = receiving_network.to_string(); // ex: "10.2.0.0/24"
                                let broadcast_addr = crate::net_utils::calculate_broadcast_for_interface(&receiving_interface_ip, &receiving_network, crate::PORT)?;
                                let seq_num = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_else(|_| std::time::Duration::from_secs(0))
                                    .as_secs() as u32;
                                if let Err(e) = crate::lsa::send_lsa(&socket, &broadcast_addr, &network_prefix, 
                                                        None, &network_prefix, std::sync::Arc::clone(&state), 
                                                        seq_num, vec![network_prefix.clone()]).await {
                                    log::error!("Failed to send LSA after HELLO: {}", e);
                                }
                            }
                        }
                        2 => {
                            // Vérifier si le protocole OSPF est activé avant de traiter les LSA
                            if !state.is_enabled().await {
                                debug!("OSPF disabled, ignoring LSA message");
                                continue;
                            }
                            
                            if let Ok(lsa) = serde_json::from_value::<crate::types::LSAMessage>(json) {
                                log::info!("[RECV] LSA from {} (originator: {}, last_hop: {:?}, seq: {}) on interface {}", 
                                    src_addr, lsa.originator, lsa.last_hop, lsa.seq_num, receiving_interface_ip);
                                let should_process = {
                                    let mut processed = state.processed_lsa.lock().await;
                                    let key = (lsa.originator.clone(), lsa.seq_num);
                                    if !processed.contains(&key) {
                                        processed.insert(key);
                                        true
                                    } else {
                                        false
                                    }
                                };
                                if should_process && lsa.ttl > 0 {
                                    if lsa.originator != receiving_interface_ip {
                                        let path_contains_us = lsa.path.contains(&receiving_interface_ip);
                                        if !path_contains_us {
                                            if let Err(e) = crate::lsa::update_routing_from_lsa(std::sync::Arc::clone(&state), &lsa, 
                                                                                  &src_addr.ip().to_string(), &socket).await {
                                                log::error!("Failed to update routing from LSA: {}", e);
                                            }
                                            if let Err(e) = crate::lsa::update_topology(std::sync::Arc::clone(&state), &lsa).await {
                                                log::error!("Failed to update topology: {}", e);
                                            }
                                            let broadcast_addr = crate::net_utils::calculate_broadcast_for_interface(&receiving_interface_ip, &receiving_network, crate::PORT)?;
                                            let mut new_path = lsa.path.clone();
                                            new_path.push(receiving_interface_ip.clone());
                                            if let Err(e) = crate::lsa::forward_lsa(&socket, &broadcast_addr, &receiving_interface_ip, 
                                                                      &lsa, new_path).await {
                                                log::error!("Failed to forward LSA: {}", e);
                                            }
                                        } else {
                                            log::debug!("Not forwarding LSA as it would create a loop");
                                        }
                                    } else {
                                        log::debug!("Not processing our own LSA");
                                    }
                                } else if !should_process {
                                    log::debug!("Ignoring duplicate LSA (originator: {}, seq: {})", lsa.originator, lsa.seq_num);
                                } else {
                                    log::debug!("LSA TTL expired, not forwarding");
                                }
                            }
                        }
                        3 => {
                            // Message de contrôle : enable/disable
                            if let Some(command) = json.get("command").and_then(|v| v.as_str()) {
                                log::info!("[CLI] Received control command from {}: {}", src_addr, command);
                                match command {
                                    "connexion" => {
                                        log::info!("[CLI] New connection from {}", src_addr);
                                        let response = "Connexion établie avec succès";
                                        if let Err(e) = crate::net_utils::send_text_response(&socket, &src_addr, response, "connexion").await {
                                            log::warn!("{}", e);
                                        }
                                    },
                                    "enable" => {
                                        state.enable().await;
                                        log::info!("[CLI] Protocole activé via commande réseau");
                                        let response = "Protocole OSPF activé";
                                        if let Err(e) = socket.send_to(response.as_bytes(), src_addr).await {
                                            log::warn!("[CLI] Failed to send enable confirmation: {}", e);
                                        }
                                    },
                                    "disable" => {
                                        state.disable().await;
                                        log::info!("[CLI] Protocole désactivé via commande réseau");
                                        let response = "Protocole OSPF désactivé";
                                        if let Err(e) = socket.send_to(response.as_bytes(), src_addr).await {
                                            log::warn!("[CLI] Failed to send disable confirmation: {}", e);
                                        }
                                    },
                                    "routing-table" => {
                                        let routing_table = state.routing_table.lock().await;
                                        let table_str = if routing_table.is_empty() {
                                            "Table de routage vide".to_string()
                                        } else {
                                            routing_table.iter()
                                                .map(|(key, (next_hop, state))| format!("{} -> {} ({:?})", key, next_hop, state))
                                                .collect::<Vec<_>>()
                                                .join("\n")
                                        };
                                        log::info!("[CLI] Routing table requested, sending to {}", src_addr);
                                        if let Err(e) = socket.send_to(table_str.as_bytes(), src_addr).await {
                                            log::warn!("[CLI] Failed to send routing table: {}", e);
                                        }
                                    },
                                    "neighbors" => {
                                        let neighbors = state.neighbors.lock().await;
                                        let neighbors_str = if neighbors.is_empty() {
                                            "Aucun voisin détecté".to_string()
                                        } else {
                                            neighbors.iter()
                                                .map(|(ip, neighbor)| {
                                                    let current_time = std::time::SystemTime::now()
                                                        .duration_since(std::time::UNIX_EPOCH)
                                                        .unwrap_or_else(|_| std::time::Duration::from_secs(0))
                                                        .as_secs();
                                                    let age = current_time.saturating_sub(neighbor.last_seen);
                                                    format!("{} (dernière activité: il y a {} secondes)", ip, age)
                                                })
                                                .collect::<Vec<_>>()
                                                .join("\n")
                                        };
                                        log::info!("[CLI] Neighbors list requested, sending to {}", src_addr);
                                        if let Err(e) = socket.send_to(neighbors_str.as_bytes(), src_addr).await {
                                            log::warn!("[CLI] Failed to send neighbors list: {}", e);
                                        }
                                    },
                                    _ => {
                                        log::warn!("[CLI] Commande de contrôle inconnue: {}", command);
                                        let response = format!("Commande inconnue: '{}'. Utilisez 'help' pour voir les commandes disponibles.", command);
                                        if let Err(e) = socket.send_to(response.as_bytes(), src_addr).await {
                                            log::warn!("[CLI] Failed to send error response: {}", e);
                                        }
                                    }
                                }
                            } else {
                                log::warn!("[CLI] Message de contrôle sans champ 'command'");
                                let response = "Erreur: message de contrôle sans commande";
                                if let Err(e) = socket.send_to(response.as_bytes(), src_addr).await {
                                    log::warn!("[CLI] Failed to send error response: {}", e);
                                }
                            }
                        }
                        _ => log::warn!("[CLI] Unknown message type: {}", message_type),
                    }
                } else {
                    log::warn!("No message_type field in received JSON");
                }
            }
            Err(e) => {
                log::error!("Failed to parse JSON: {}", e);
            }
        }
    }
}