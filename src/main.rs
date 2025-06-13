use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, ToSocketAddrs};
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use std::sync::Arc;
use net_route::{Route, Handle};
use pnet::datalink::{self, NetworkInterface};

#[derive(Debug, Serialize, Deserialize, Clone)]
struct HelloMessage {
    message_type: u8,
    router_ip: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Neighbor {
    neighbor_ip: String,
    link_up: bool,
    capacity: u32,
}

#[derive(Debug, Serialize, Deserialize)]
struct LSAMessage {
    message_type: u8,
    router_ip: String,
    neighbor_count: usize,
    neighbors: Vec<Neighbor>,
}

// Nouveau message pour demander la table de routage
#[derive(Debug, Serialize, Deserialize)]
struct RoutingTableRequest {
    message_type: u8,
    router_ip: String,
}

// Nouveau message pour répondre avec la table de routage
#[derive(Debug, Serialize, Deserialize, Clone)]
struct RoutingEntry {
    destination: String,
    next_hop: String,
    metric: u32,
    hop_count: u8,
}

#[derive(Debug, Serialize, Deserialize)]
struct RoutingTableResponse {
    message_type: u8,
    router_ip: String,
    routing_table: Vec<RoutingEntry>,
}

struct Router {
    router_ip: String,
    neighbors: Vec<Neighbor>,
}

struct AppState {
    topology: Mutex<HashMap<String, Router>>,
    neighbors: Mutex<HashMap<String, Neighbor>>,
    routing_table: Mutex<HashMap<String, RoutingEntry>>, // Nouvelle table de routage locale
}

fn get_broadcast_addresses_with_interface_ips(port: u16) -> Vec<(SocketAddr, String)> {
    let interfaces = datalink::interfaces();
    interfaces
        .into_iter()
        .flat_map(|iface: NetworkInterface| {
            iface.ips.into_iter().filter_map(move |ip_network| {
                if let IpAddr::V4(ip) = ip_network.ip() {
                    if !ip.is_loopback() && !ip.is_unspecified() {
                        let prefix_len = ip_network.prefix();
                        let mask = u32::MAX << (32 - prefix_len);
                        let broadcast = u32::from(ip) | !mask;
                        let broadcast_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::from(broadcast)), port);
                        Some((broadcast_addr, ip.to_string()))
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

fn get_local_ips() -> Vec<String> {
    let interfaces = datalink::interfaces();
    interfaces
        .into_iter()
        .flat_map(|interface| {
            interface.ips.into_iter().filter_map(|ip_network| {
                if let IpAddr::V4(ipv4) = ip_network.ip() {
                    if !ipv4.is_loopback() && !ipv4.is_unspecified() {
                        Some(ipv4.to_string())
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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let router_ip = get_local_ip()?;
    let local_ips = get_local_ips();
    println!("Router IP: {}", router_ip);
    println!("Local IPs: {:?}", local_ips);

    let socket = Arc::new(UdpSocket::bind("0.0.0.0:5000").await?);
    socket.set_broadcast(true)?;

    let broadcast_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255)), 5000);

    let state = Arc::new(AppState {
        topology: Mutex::new(HashMap::new()),
        neighbors: Mutex::new(HashMap::new()),
        routing_table: Mutex::new(HashMap::new()), // Initialiser la table de routage
    });

    // Task pour envoyer les HELLO messages
    let socket_clone = Arc::clone(&socket);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(20)).await;
            let broadcast_addrs_with_ips = get_broadcast_addresses_with_interface_ips(5000);

            for (addr, interface_ip) in &broadcast_addrs_with_ips {
                if let Err(e) = send_hello(&socket_clone, addr, interface_ip).await {
                    log::error!("Failed to send hello to {} from interface {}: {}", addr, interface_ip, e);
                }
            }
        }
    });

    // Nouvelle task pour demander les tables de routage
    let socket_clone2 = Arc::clone(&socket);
    let state_clone = Arc::clone(&state);
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
            let broadcast_addrs_with_ips = get_broadcast_addresses_with_interface_ips(5000);

            for (addr, interface_ip) in &broadcast_addrs_with_ips {
                if let Err(e) = send_routing_table_request(&socket_clone2, addr, interface_ip).await {
                    log::error!("Failed to send routing table request to {} from interface {}: {}", addr, interface_ip, e);
                }
            }
        }
    });

    let mut buf = [0; 4096]; // Augmenter la taille du buffer pour les tables de routage
    loop {
        let (len, src_addr) = socket.recv_from(&mut buf).await?;
        println!("Received {} bytes from {}", len, src_addr);

        match serde_json::from_slice::<serde_json::Value>(&buf[..len]) {
            Ok(json) => {
                if let Some(message_type) = json.get("message_type").and_then(|v| v.as_u64()) {
                    match message_type {
                        1 => {
                            println!("IN [RECV] HELLO");
                            if let Ok(hello) = serde_json::from_value::<HelloMessage>(json) {
                                // Vérifier si le message provient de nous-même
                                if local_ips.contains(&hello.router_ip) {
                                    println!("Ignoring HELLO from self: {}", hello.router_ip);
                                    continue;
                                }

                                println!("[RECV] HELLO from {} - {}", hello.router_ip, src_addr);
                                
                                let mut neighbors = state.neighbors.lock().await;
                                neighbors.insert(
                                    hello.router_ip.clone(),
                                    Neighbor {
                                        neighbor_ip: hello.router_ip.clone(),
                                        link_up: true,
                                        capacity: 100,
                                    },
                                );
                                drop(neighbors);

                                let broadcast_addrs_with_ips = get_broadcast_addresses_with_interface_ips(5000);
                                for (addr, interface_ip) in &broadcast_addrs_with_ips {
                                    if let Err(e) = send_lsa(&socket, addr, interface_ip, state.clone()).await {
                                        log::error!("Failed to send LSA to {} from interface {}: {}", addr, interface_ip, e);
                                    }
                                }
                            }
                        }
                        2 => {
                            if let Ok(lsa) = serde_json::from_value::<LSAMessage>(json) {
                                if local_ips.contains(&lsa.router_ip) {
                                    println!("Ignoring LSA from self: {}", lsa.router_ip);
                                    continue;
                                }

                                println!("[RECV] LSA from {} - {}", lsa.router_ip, src_addr);
                                if let Err(e) = update_topology(state.clone(), &lsa).await {
                                    log::error!("Failed to update topology: {}", e);
                                }
                                if let Err(e) = compute_shortest_paths(state.clone(), &lsa.router_ip).await {
                                    log::error!("Failed to compute shortest paths: {}", e);
                                }
                            }
                        }
                        3 => {
                            // Nouvelle gestion des demandes de table de routage
                            if let Ok(request) = serde_json::from_value::<RoutingTableRequest>(json) {
                                if local_ips.contains(&request.router_ip) {
                                    println!("Ignoring routing table request from self: {}", request.router_ip);
                                    continue;
                                }

                                println!("[RECV] Routing table request from {}", request.router_ip);
                                
                                // Répondre avec notre table de routage
                                let broadcast_addrs_with_ips = get_broadcast_addresses_with_interface_ips(5000);
                                for (addr, interface_ip) in &broadcast_addrs_with_ips {
                                    if let Err(e) = send_routing_table_response(&socket, addr, interface_ip, state.clone()).await {
                                        log::error!("Failed to send routing table response to {} from interface {}: {}", addr, interface_ip, e);
                                    }
                                }
                            }
                        }
                        4 => {
                            // Nouvelle gestion des réponses de table de routage
                            if let Ok(response) = serde_json::from_value::<RoutingTableResponse>(json) {
                                if local_ips.contains(&response.router_ip) {
                                    println!("Ignoring routing table response from self: {}", response.router_ip);
                                    continue;
                                }

                                println!("[RECV] Routing table response from {}", response.router_ip);
                                if let Err(e) = update_routing_table_from_neighbor(state.clone(), &response, &local_ips).await {
                                    log::error!("Failed to update routing table from neighbor: {}", e);
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

async fn send_hello(socket: &UdpSocket, addr: &SocketAddr, router_ip: &str) -> Result<(), Box<dyn std::error::Error>> {
    let message = HelloMessage {
        message_type: 1,
        router_ip: router_ip.to_string(),
    };
    let serialized = serde_json::to_vec(&message)?;
    socket.send_to(&serialized, addr).await?;
    println!("[SEND] HELLO from {} - {}", router_ip, addr);
    Ok(())
}

async fn send_lsa(socket: &UdpSocket, addr: &SocketAddr, router_ip: &str, state: Arc<AppState>) -> Result<(), Box<dyn std::error::Error>> {
    let neighbors = state.neighbors.lock().await;
    let neighbors_vec = neighbors.values().cloned().collect::<Vec<_>>();

    let message = LSAMessage {
        message_type: 2,
        router_ip: router_ip.to_string(),
        neighbor_count: neighbors_vec.len(),
        neighbors: neighbors_vec,
    };

    let serialized = serde_json::to_vec(&message)?;
    socket.send_to(&serialized, addr).await?;
    println!("[SEND] LSA from {}", router_ip);
    Ok(())
}

// Nouvelle fonction pour envoyer une demande de table de routage
async fn send_routing_table_request(socket: &UdpSocket, addr: &SocketAddr, router_ip: &str) -> Result<(), Box<dyn std::error::Error>> {
    let message = RoutingTableRequest {
        message_type: 3,
        router_ip: router_ip.to_string(),
    };
    let serialized = serde_json::to_vec(&message)?;
    socket.send_to(&serialized, addr).await?;
    println!("[SEND] Routing table request from {} - {}", router_ip, addr);
    Ok(())
}

// Nouvelle fonction pour envoyer une réponse de table de routage
async fn send_routing_table_response(socket: &UdpSocket, addr: &SocketAddr, router_ip: &str, state: Arc<AppState>) -> Result<(), Box<dyn std::error::Error>> {
    let routing_table = state.routing_table.lock().await;
    let routing_entries = routing_table.values().cloned().collect::<Vec<_>>();

    let message = RoutingTableResponse {
        message_type: 4,
        router_ip: router_ip.to_string(),
        routing_table: routing_entries,
    };

    let serialized = serde_json::to_vec(&message)?;
    socket.send_to(&serialized, addr).await?;
    println!("[SEND] Routing table response from {} with {} entries", router_ip, routing_table.len());
    Ok(())
}

// Nouvelle fonction pour mettre à jour la table de routage à partir des voisins
async fn update_routing_table_from_neighbor(
    state: Arc<AppState>, 
    response: &RoutingTableResponse, 
    local_ips: &[String]
) -> Result<(), Box<dyn std::error::Error>> {
    let mut routing_table = state.routing_table.lock().await;
    let neighbors = state.neighbors.lock().await;
    
    // Vérifier que le routeur qui répond est bien un voisin direct
    if !neighbors.contains_key(&response.router_ip) {
        log::warn!("Received routing table from non-neighbor: {}", response.router_ip);
        return Ok(());
    }
    
    for entry in &response.routing_table {
        // Éviter les boucles : ne pas ajouter de routes vers nous-mêmes
        if local_ips.contains(&entry.destination) {
            continue;
        }
        
        // Éviter les boucles : ne pas ajouter de routes qui passent par nous
        if local_ips.contains(&entry.next_hop) {
            continue;
        }
        
        // Calculer la nouvelle métrique (distance + 1 hop)
        let new_metric = entry.metric + 1;
        let new_hop_count = entry.hop_count + 1;
        
        // Éviter les boucles de routage en limitant le nombre de hops (algorithme RIP)
        if new_hop_count > 15 {
            continue;
        }
        
        let new_entry = RoutingEntry {
            destination: entry.destination.clone(),
            next_hop: response.router_ip.clone(), // Le prochain saut est le voisin qui nous a envoyé cette route
            metric: new_metric,
            hop_count: new_hop_count,
        };
        
        // Mettre à jour seulement si on n'a pas de route ou si la nouvelle route est meilleure
        match routing_table.get(&entry.destination) {
            Some(existing_entry) => {
                if new_metric < existing_entry.metric {
                    println!("Updating route to {} via {} (metric: {} -> {})", 
                             entry.destination, response.router_ip, existing_entry.metric, new_metric);
                    routing_table.insert(entry.destination.clone(), new_entry.clone());
                    
                    // Mettre à jour la table de routage système
                    if let Err(e) = update_routing_table(&entry.destination, &response.router_ip).await {
                        log::error!("Failed to update system routing table for {}: {}", entry.destination, e);
                    }
                }
            }
            None => {
                println!("Adding new route to {} via {} (metric: {})", 
                         entry.destination, response.router_ip, new_metric);
                routing_table.insert(entry.destination.clone(), new_entry.clone());
                
                // Mettre à jour la table de routage système
                if let Err(e) = update_routing_table(&entry.destination, &response.router_ip).await {
                    log::error!("Failed to update system routing table for {}: {}", entry.destination, e);
                }
            }
        }
    }
    
    drop(routing_table);
    drop(neighbors);
    
    // Afficher la table de routage mise à jour
    print_routing_table(state).await;
    Ok(())
}

// Nouvelle fonction pour afficher la table de routage
async fn print_routing_table(state: Arc<AppState>) {
    let routing_table = state.routing_table.lock().await;
    println!("\n=== Current Routing Table ===");
    for (destination, entry) in routing_table.iter() {
        println!("To {} via {} (metric: {}, hops: {})", 
                 destination, entry.next_hop, entry.metric, entry.hop_count);
    }
    println!("=============================\n");
}

fn get_broadcast_addresses(port: u16) -> Vec<SocketAddr> {
    let interfaces = datalink::interfaces();
    interfaces
        .into_iter()
        .flat_map(|iface: NetworkInterface| {
            iface.ips.into_iter().filter_map(move |ip_network| {
                if let IpAddr::V4(ip) = ip_network.ip() {
                    if !ip.is_loopback() && !ip.is_unspecified() {
                        let prefix_len = ip_network.prefix();
                        let mask = u32::MAX << (32 - prefix_len);
                        let broadcast = u32::from(ip) | !mask;
                        Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::from(broadcast)), port))
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

fn get_local_ip() -> Result<String, Box<dyn std::error::Error>> {
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
    Err("No valid IP address found".into())
}

async fn update_topology(state: Arc<AppState>, lsa: &LSAMessage) -> Result<(), Box<dyn std::error::Error>> {
    let mut topology = state.topology.lock().await;
    topology.insert(
        lsa.router_ip.clone(),
        Router {
            router_ip: lsa.router_ip.clone(),
            neighbors: lsa.neighbors.clone(),
        },
    );
    Ok(())
}

async fn update_routing_table(destination: &str, gateway: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Éviter d'ajouter des routes vers des gateways invalides
    if gateway == "0.0.0.0" || gateway.is_empty() {
        log::debug!("Skipping route to {} - invalid gateway: {}", destination, gateway);
        return Ok(());
    }

    let destination_ip: Ipv4Addr = destination.parse()?;
    let gateway_ip: Ipv4Addr = gateway.parse()?;

    // Vérifier si le gateway est dans un réseau accessible
    if !is_gateway_reachable(&gateway_ip).await {
        log::warn!("Gateway {} is not reachable, skipping route to {}", gateway, destination);
        return Ok(());
    }

    let handle = Handle::new()?;
    
    // Calculer l'adresse réseau en appliquant un masque /24 (255.255.255.0)
    let network_addr = Ipv4Addr::from(u32::from(destination_ip) & 0xFFFFFF00);
    
    let route = Route::new(IpAddr::V4(network_addr), 24)
        .with_gateway(IpAddr::V4(gateway_ip));

    match handle.add(&route).await {
        Ok(_) => println!("Successfully added route to network {}/24 via {}", network_addr, gateway),
        Err(e) => {
            if e.to_string().contains("network unreachable") || e.to_string().contains("101") {
                log::warn!("Cannot add route to {}/24 via {} - gateway unreachable", network_addr, gateway);
                return Ok(()); // Ne pas considérer comme une erreur fatale
            } else if e.to_string().contains("file exists") || e.to_string().contains("17") {
                log::debug!("Route to {}/24 via {} already exists", network_addr, gateway);
                return Ok(());
            } else {
                log::warn!("Failed to add route to network {}/24 via {}: {}", network_addr, gateway, e);
                // Essayer de supprimer et re-ajouter
                let _ = handle.delete(&route).await;
                match handle.add(&route).await {
                    Ok(_) => println!("Successfully updated route to network {}/24 via {}", network_addr, gateway),
                    Err(e2) => {
                        log::error!("Failed to update route after deletion: {}", e2);
                        return Err(e2.into());
                    }
                }
            }
        }
    }

    Ok(())
}

async fn is_gateway_reachable(gateway_ip: &Ipv4Addr) -> bool {
    let local_ips = get_local_ips();
    let broadcast_addrs = get_broadcast_addresses_with_interface_ips(5000);
    
    // Vérifier si le gateway est dans le même réseau qu'une de nos interfaces
    for (_, interface_ip) in broadcast_addrs {
        if let Ok(local_ip) = interface_ip.parse::<Ipv4Addr>() {
            // Vérifier avec différents masques de réseau communs
            for prefix in [24, 16, 8] {
                let mask = u32::MAX << (32 - prefix);
                let local_network = u32::from(local_ip) & mask;
                let gateway_network = u32::from(*gateway_ip) & mask;
                
                if local_network == gateway_network {
                    return true;
                }
            }
        }
    }
    
    false
}

async fn compute_shortest_paths(state: Arc<AppState>, source_ip: &str) -> Result<(), Box<dyn std::error::Error>> {
    let topology = state.topology.lock().await;
    let mut nodes: HashMap<String, (u32, Option<String>)> = topology
        .keys()
        .map(|id| (id.clone(), (if id == source_ip { 0 } else { u32::MAX }, None)))
        .collect();

    let mut visited = Vec::new();
    while visited.len() < topology.len() {
        let current_node = nodes
            .iter()
            .filter(|(id, _)| !visited.contains(*id))
            .min_by_key(|(_, (cost, _))| *cost)
            .map(|(id, _)| id.clone());

        if let Some(current) = current_node {
            visited.push(current.clone());
            if let Some(router) = topology.get(&current) {
                for neighbor in &router.neighbors {
                    if !neighbor.link_up {
                        continue;
                    }

                    let weight = 1000 / neighbor.capacity;
                    let new_cost = nodes[&current].0 + weight;

                    if !nodes.contains_key(&neighbor.neighbor_ip) {
                        nodes.insert(neighbor.neighbor_ip.clone(), (new_cost, Some(current.clone())));
                    } else if new_cost < nodes[&neighbor.neighbor_ip].0 {
                        nodes.insert(neighbor.neighbor_ip.clone(), (new_cost, Some(current.clone())));
                    }
                }
            }
        } else {
            break;
        }
    }

    println!("\n=== Routing Table ({}) ===", source_ip);
    let mut routing_table = state.routing_table.lock().await;
    
    for (ip, (cost, prev)) in nodes {
        if ip != source_ip {
            if let Some(gateway) = prev {
                let neighbors = state.neighbors.lock().await;
                if neighbors.contains_key(&gateway) {
                    println!("To {} via {} (cost: {})", ip, gateway, cost);
                    
                    // Mettre à jour notre table de routage locale
                    routing_table.insert(ip.clone(), RoutingEntry {
                        destination: ip.clone(),
                        next_hop: gateway.clone(),
                        metric: cost,
                        hop_count: 1,
                    });
                    
                    if let Err(e) = update_routing_table(&ip, &gateway).await {
                        log::error!("Failed to update routing table for {}: {}", ip, e);
                    }
                } else {
                    println!("To {} - no direct route available (cost: {})", ip, cost);
                }
            } else {
                println!("To {} - unreachable (cost: {})", ip, cost);
            }
        }
    }
    println!("\n==========================");
    Ok(())
}
