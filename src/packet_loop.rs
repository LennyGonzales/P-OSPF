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
        match serde_json::from_slice::<serde_json::Value>(&buf[..len]) {
            Ok(json) => {
                if let Some(message_type) = json.get("message_type").and_then(|v| v.as_u64()) {
                    match message_type {
                        1 => {
                            if let Ok(hello) = serde_json::from_value::<crate::types::HelloMessage>(json) {
                                log::info!("[RECV] HELLO from {} - {} (received on interface {})", 
                                    hello.router_ip, src_addr, receiving_interface_ip);
                                crate::neighbor::update_neighbor(&state, &hello.router_ip).await;
                                let broadcast_addr = crate::net_utils::calculate_broadcast_for_interface(&receiving_interface_ip, &receiving_network, crate::PORT)?;
                                let seq_num = std::time::SystemTime::now()
                                    .duration_since(std::time::UNIX_EPOCH)
                                    .unwrap_or_else(|_| std::time::Duration::from_secs(0))
                                    .as_secs() as u32;
                                if let Err(e) = crate::lsa::send_lsa(&socket, &broadcast_addr, &receiving_interface_ip, 
                                                        None, &receiving_interface_ip, std::sync::Arc::clone(&state), 
                                                        seq_num, vec![receiving_interface_ip.clone()]).await {
                                    log::error!("Failed to send LSA after HELLO: {}", e);
                                }
                            }
                        }
                        2 => {
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
                        _ => log::warn!("Unknown message type: {}", message_type),
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