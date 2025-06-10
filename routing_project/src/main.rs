use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use std::sync::Arc;
use net_route::{Route, Handle};

#[derive(Debug, Serialize, Deserialize)]
struct HelloMessage {
    message_type: u8,
    router_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Neighbor {
    neighbor_id: String,
    link_up: bool,
    capacity: u32, // in Mbps
}

#[derive(Debug, Serialize, Deserialize)]
struct LSAMessage {
    message_type: u8,
    router_id: String,
    neighbor_count: usize,
    neighbors: Vec<Neighbor>,
}

struct Router {
    router_id: String,
    neighbors: Vec<Neighbor>,
}

struct AppState {
    topology: Mutex<HashMap<String, Router>>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();

    let router_id = get_local_ip().unwrap_or_else(|| "127.0.0.1".to_string());
    println!("Router ID: {}", router_id); // log::info!("Router ID: {}", router_id);

    let socket = Arc::new(UdpSocket::bind("0.0.0.0:5000").await?);
    socket.set_multicast_loop_v4(true)?;

    let multicast_addr: Ipv4Addr = "224.0.0.5".parse().unwrap();
    let local_ip = get_local_ip().unwrap();
    socket.join_multicast_v4(multicast_addr, local_ip.parse().unwrap())?;


    let remote_addr = SocketAddr::new(IpAddr::V4(multicast_addr), 5000);

    let state = Arc::new(AppState {
        topology: Mutex::new(HashMap::new()),
    });

    // Cloner les ressources partagées pour la tâche d'envoi de HELLO
    let socket_clone = Arc::clone(&socket);
    let router_id_clone = router_id.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(20)).await;
            if let Err(e) = send_hello(&socket_clone, &remote_addr, &router_id_clone).await {
                log::error!("Failed to send hello: {}", e);
            }
        }
    });

    let mut buf = [0; 2048];
    loop {
        let (len, _addr) = socket.recv_from(&mut buf).await?;
        println!("Received {} bytes from {}", len, _addr);
        match serde_json::from_slice::<serde_json::Value>(&buf[..len]) {
            Ok(json) => {
                if let Some(message_type) = json.get("message_type").and_then(|v| v.as_u64()) {
                    match message_type {
                        1 => {
                            println!("IN [RECV] HELLO");
                            if let Ok(hello) = serde_json::from_value::<HelloMessage>(json) {
                                println!("[RECV] HELLO from {}", hello.router_id);
                                if let Err(e) = send_lsa(&socket, &remote_addr, &router_id).await {
                                    log::error!("Failed to send LSA: {}", e);
                                }
                            }
                        }
                        2 => {
                            if let Ok(lsa) = serde_json::from_value::<LSAMessage>(json) {
                                println!("[RECV] LSA from {}", lsa.router_id);
                                if let Err(e) = update_topology(state.clone(), lsa).await {
                                    log::error!("Failed to update topology: {}", e);
                                }
                                if let Err(e) = compute_shortest_paths(state.clone(), &router_id).await {
                                    log::error!("Failed to compute shortest paths: {}", e);
                                }
                            }
                        }
                        _ => {
                            log::warn!("Unknown message type: {}", message_type);
                        }
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

async fn send_hello(socket: &UdpSocket, remote_addr: &SocketAddr, router_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let message = HelloMessage {
        message_type: 1,
        router_id: router_id.to_string(),
    };
    let serialized = serde_json::to_vec(&message)?;
    socket.send_to(&serialized, remote_addr).await?;
    println!("[SEND] HELLO from {}", router_id);
    Ok(())
}

async fn send_lsa(socket: &UdpSocket, remote_addr: &SocketAddr, router_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let message = LSAMessage {
        message_type: 2,
        router_id: router_id.to_string(),
        neighbor_count: 1,
        neighbors: vec![Neighbor {
            neighbor_id: "192.168.1.1".to_string(),
            link_up: true,
            capacity: 100,
        }],
    };
    let serialized = serde_json::to_vec(&message)?;
    socket.send_to(&serialized, remote_addr).await?;
    println!("[SEND] LSA from {}", router_id);
    Ok(())
}

fn get_local_ip() -> Option<String> {
    let socket = match std::net::UdpSocket::bind("0.0.0.0:0") {
        Ok(s) => s,
        Err(_) => return None,
    };

    match socket.connect("8.8.8.8:80") {
        Ok(()) => (),
        Err(_) => return None,
    };

    match socket.local_addr() {
        Ok(addr) => Some(addr.ip().to_string()),
        Err(_) => None,
    }
}

async fn update_topology(state: Arc<AppState>, lsa: LSAMessage) -> Result<(), Box<dyn std::error::Error>> {
    let mut topology = state.topology.lock().await;
    topology.insert(
        lsa.router_id.clone(),
        Router {
            router_id: lsa.router_id,
            neighbors: lsa.neighbors,
        },
    );
    Ok(())
}

async fn compute_shortest_paths(state: Arc<AppState>, source_id: &str) -> Result<(), Box<dyn std::error::Error>> {
    let topology = state.topology.lock().await;
    let mut nodes: HashMap<String, (u32, Option<String>)> = topology
        .keys()
        .map(|id| (id.clone(), (if id == source_id { 0 } else { u32::MAX }, None)))
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

                    if !nodes.contains_key(&neighbor.neighbor_id) { // Ajoute s'il n'existe pas
                        nodes.insert(neighbor.neighbor_id.clone(), (new_cost, Some(current.clone())));
                    } else if new_cost < nodes[&neighbor.neighbor_id].0 { // Mettre à jour le coût si le nouveau est plus bas
                        nodes.insert(neighbor.neighbor_id.clone(), (new_cost, Some(current.clone())));
                    }
                }
            }
        } else {
            break;
        }
    }

    println!("\n=== Routing Table ({}) ===", source_id);
    for (id, (cost, prev)) in nodes {
        if id != source_id {
            let gateway = prev.unwrap_or_else(|| "0.0.0.0".to_string());
            println!("To {} via {} (cost: {})", id, gateway, cost);
            if let Err(e) = update_routing_table(&id, &gateway).await {
                log::error!("Failed to update routing table: {}", e);
            }
        }
    }
    println!("\n==========================");
    Ok(())
}

async fn update_routing_table(destination: &str, gateway: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Utilisation de net-route pour une approche cross-platform
    let handle = Handle::new()?;
    
    let dest_ip: Ipv4Addr = destination.parse()?;
    let gateway_ip: Ipv4Addr = gateway.parse()?;
    
    // Créer une route vers la destination via la gateway
    let route = Route::new(IpAddr::V4(dest_ip), 32)
        .with_gateway(IpAddr::V4(gateway_ip));
    
    // Ajouter la route (supprime automatiquement l'ancienne si elle existe)
    match handle.add(&route).await {
        Ok(_) => println!("Successfully added route to {} via {}", destination, gateway),
        Err(e) => {
            log::warn!("Failed to add route to {} via {}: {}", destination, gateway, e);
            // Essayer de supprimer l'ancienne route puis ajouter la nouvelle
            let _ = handle.delete(&route).await;
            handle.add(&route).await?;
            println!("Successfully updated route to {} via {}", destination, gateway);
        }
    }
    
    Ok(())
}

// Fonction utilitaire pour lister les routes existantes (optionnel)
#[allow(dead_code)]
async fn list_routes() -> Result<(), Box<dyn std::error::Error>> {
    let handle = Handle::new()?;
    let routes = handle.list().await?;
    
    println!("Current routing table:");
    for route in routes {
        println!("  {} -> {:?}", route.destination, route.gateway);
    }
    
    Ok(())
}