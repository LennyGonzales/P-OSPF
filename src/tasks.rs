pub fn spawn_hello_and_lsa_tasks(socket: std::sync::Arc<tokio::net::UdpSocket>, state: std::sync::Arc<crate::AppState>) {
    let socket_clone = std::sync::Arc::clone(&socket);
    let state_clone = std::sync::Arc::clone(&state);
    tokio::spawn(async move {
        let mut hello_interval = tokio::time::interval(std::time::Duration::from_secs(crate::HELLO_INTERVAL_SEC));
        let mut lsa_interval = tokio::time::interval(std::time::Duration::from_secs(crate::LSA_INTERVAL_SEC));
        loop {
            tokio::select! {
                _ = hello_interval.tick() => {
                    // Vérifier si le protocole OSPF est activé avant d'envoyer des HELLO
                    if !state_clone.is_enabled().await {
                        continue;
                    }
                    
                    let broadcast_addrs = crate::net_utils::get_broadcast_addresses(crate::PORT);
                    for (local_ip, addr) in &broadcast_addrs {
                        if let Err(e) = crate::hello::send_hello(&socket_clone, addr, local_ip).await {
                            log::error!("Failed to send hello to {}: {}", addr, e);
                        }
                    }
                }
                _ = lsa_interval.tick() => {
                    // Vérifier si le protocole OSPF est activé avant d'envoyer des LSA
                    if !state_clone.is_enabled().await {
                        continue;
                    }
                    
                    let broadcast_addrs = crate::net_utils::get_broadcast_addresses(crate::PORT);
                    for (local_ip, addr) in &broadcast_addrs {
                        let seq_num = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_else(|_| std::time::Duration::from_secs(0))
                            .as_secs() as u32;
                        if let Err(e) = crate::lsa::send_lsa(&socket_clone, addr, local_ip, None, local_ip, std::sync::Arc::clone(&state_clone), seq_num, vec![]).await {
                            log::error!("Failed to send LSA: {}", e);
                        }
                    }
                }
            }
        }
    });
}

pub fn spawn_neighbor_timeout_task(state: std::sync::Arc<crate::AppState>) {
    let state_clone = std::sync::Arc::clone(&state);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(crate::NEIGHBOR_TIMEOUT_SEC / 2));
        loop {
            interval.tick().await;
            crate::neighbor::check_neighbor_timeouts(&state_clone).await;
        }
    });
}