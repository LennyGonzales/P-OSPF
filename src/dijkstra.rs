// Module d'implémentation de l'algorithme de Dijkstra pour OSPF
// Calcul des meilleurs chemins basé sur les coûts, nombre de sauts et capacités

use std::collections::{HashMap, BinaryHeap, HashSet};
use std::cmp::Ordering;
use std::sync::Arc;
use log::{info, debug, warn, error};
use crate::types::{RouteState, Neighbor};
use crate::error::{AppError, Result};
use crate::AppState;

/// Représente un nœud dans le graphe du réseau
#[derive(Debug, Clone)]
pub struct NetworkNode {
    pub router_id: String,
    pub interfaces: Vec<InterfaceInfo>,
    pub is_reachable: bool,
}

/// Informations sur une interface réseau
#[derive(Debug, Clone)]
pub struct InterfaceInfo {
    pub name: String,
    pub network: String,
    pub capacity_mbps: u32,
    pub is_active: bool,
    pub connected_to: Option<String>, // IP du routeur voisin
}

/// Représente une arête (lien) dans le graphe
#[derive(Debug, Clone)]
pub struct NetworkLink {
    pub from: String,
    pub to: String,
    pub cost: u32,
    pub capacity_mbps: u32,
    pub is_active: bool,
    pub hop_count: u32,
}

/// Nœud utilisé dans l'algorithme de Dijkstra
#[derive(Debug, Clone, Eq, PartialEq)]
struct DijkstraNode {
    router_id: String,
    total_cost: u32,
    hop_count: u32,
    bottleneck_capacity: u32, // Capacité du goulot d'étranglement (lien le plus lent)
    path: Vec<String>,
}

impl Ord for DijkstraNode {
    fn cmp(&self, other: &Self) -> Ordering {
        // Priorité simplifiée : 1) Nombre de sauts minimum (plus court chemin), 2) Router ID pour consistance
        other.hop_count.cmp(&self.hop_count)
            .then_with(|| self.router_id.cmp(&other.router_id))
    }
}

impl PartialOrd for DijkstraNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Structure représentant la topologie complète du réseau
#[derive(Debug, Clone)]
pub struct NetworkTopology {
    pub nodes: HashMap<String, NetworkNode>,
    pub links: Vec<NetworkLink>,
}

impl NetworkTopology {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            links: Vec::new(),
        }
    }

    /// Ajoute un routeur à la topologie
    pub fn add_router(&mut self, router_id: String, interfaces: Vec<InterfaceInfo>) {
        let node = NetworkNode {
            router_id: router_id.clone(),
            interfaces,
            is_reachable: true,
        };
        self.nodes.insert(router_id, node);
    }

    /// Ajoute un lien bidirectionnel entre deux routeurs
    pub fn add_link(&mut self, from: String, to: String, is_active: bool) {
        // Lien direct
        self.links.push(NetworkLink {
            from: from.clone(),
            to: to.clone(),
            cost: if is_active { 1 } else { u32::MAX }, // Coût simplifié : 1 si actif, infini si inactif
            capacity_mbps: 100, // Valeur par défaut, ignorée pour l'instant
            is_active,
            hop_count: 1,
        });
        
        // Lien de retour (bidirectionnel)
        self.links.push(NetworkLink {
            from: to,
            to: from,
            cost: if is_active { 1 } else { u32::MAX },
            capacity_mbps: 100,
            is_active,
            hop_count: 1,
        });
    }

    /// Trouve les voisins actifs d'un routeur
    pub fn get_active_neighbors(&self, router_id: &str) -> Vec<&NetworkLink> {
        self.links.iter()
            .filter(|link| link.from == router_id && link.is_active)
            .collect()
    }

    /// Calcule les meilleurs chemins depuis un routeur source
    /// Basé sur le nombre de sauts minimum et l'état des liens
    pub fn calculate_shortest_paths(&self, source: &str) -> HashMap<String, RouteInfo> {
        let mut distances: HashMap<String, u32> = HashMap::new();
        let mut paths: HashMap<String, Vec<String>> = HashMap::new();
        let mut visited = HashSet::new();
        let mut heap = BinaryHeap::new();

        // Initialisation - tous les nœuds à distance infinie
        for node_id in self.nodes.keys() {
            distances.insert(node_id.clone(), u32::MAX);
            paths.insert(node_id.clone(), Vec::new());
        }

        // Nœud source à distance 0
        distances.insert(source.to_string(), 0);
        paths.insert(source.to_string(), vec![source.to_string()]);

        heap.push(DijkstraNode {
            router_id: source.to_string(),
            total_cost: 0,
            hop_count: 0,
            bottleneck_capacity: u32::MAX, // Ignoré pour l'instant
            path: vec![source.to_string()],
        });

        // Algorithme de Dijkstra modifié pour le nombre de sauts
        while let Some(current) = heap.pop() {
            if visited.contains(&current.router_id) {
                continue;
            }
            visited.insert(current.router_id.clone());

            // Explorer les voisins actifs
            for link in self.get_active_neighbors(&current.router_id) {
                if visited.contains(&link.to) || !link.is_active {
                    continue;
                }

                let new_distance = current.hop_count + 1;
                let current_best_distance = *distances.get(&link.to).unwrap_or(&u32::MAX);

                // Mise à jour si on trouve un chemin plus court
                if new_distance < current_best_distance {
                    distances.insert(link.to.clone(), new_distance);
                    
                    let mut new_path = current.path.clone();
                    new_path.push(link.to.clone());
                    paths.insert(link.to.clone(), new_path.clone());

                    heap.push(DijkstraNode {
                        router_id: link.to.clone(),
                        total_cost: new_distance,
                        hop_count: new_distance,
                        bottleneck_capacity: 0, // Ignoré pour l'instant
                        path: new_path,
                    });
                }
            }
        }

        // Construire les résultats
        let mut routes = HashMap::new();
        for (dest, distance) in distances {
            if dest != source && distance != u32::MAX {
                let path = paths.get(&dest).unwrap_or(&Vec::new()).clone();
                let next_hop = if path.len() > 1 { path[1].clone() } else { dest.clone() };
                
                routes.insert(dest.clone(), RouteInfo {
                    destination: dest.clone(),
                    next_hop,
                    total_cost: distance,
                    hop_count: distance,
                    bottleneck_capacity: 0, // Ignoré pour l'instant
                    path,
                    is_reachable: true,
                });
            }
        }

        debug!("Chemins calculés depuis {} : {} destinations atteignables", source, routes.len());
        for (dest, route) in &routes {
            debug!("Route vers {} : {} sauts via {} (chemin: {:?})", 
                dest, route.hop_count, route.next_hop, route.path);
        }

        routes
    }
}

/// Informations sur une route calculée
#[derive(Debug, Clone)]
pub struct RouteInfo {
    pub destination: String,
    pub next_hop: String,
    pub total_cost: u32,
    pub hop_count: u32,
    pub bottleneck_capacity: u32,
    pub path: Vec<String>,
    pub is_reachable: bool,
}

/// Calcule le coût OSPF basé sur la capacité et l'état
pub fn calculate_ospf_cost(capacity_mbps: u32, is_active: bool) -> u32 {
    if !is_active {
        return u32::MAX; // Coût infini pour les liens inactifs
    }
    
    if capacity_mbps == 0 {
        return u32::MAX;
    }
    
    // Formule OSPF standard : 100 Mbps de référence
    let reference_bandwidth = 100_000_000; // 100 Mbps en bps
    let bandwidth_bps = capacity_mbps * 1_000_000;
    let cost = reference_bandwidth / bandwidth_bps;
    cost.max(1) // Coût minimum de 1
}

/// Construit la topologie réseau à partir de l'état OSPF
pub async fn build_network_topology(state: Arc<AppState>) -> NetworkTopology {
    let mut topology = NetworkTopology::new();
    
    // Ajouter le routeur local
    let local_interfaces = state.config.interfaces.iter().map(|iface| {
        InterfaceInfo {
            name: iface.name.clone(),
            network: format!("network_{}", iface.name), // Simplification
            capacity_mbps: iface.capacity_mbps,
            is_active: iface.link_active,
            connected_to: None,
        }
    }).collect();
    
    topology.add_router(state.local_ip.clone(), local_interfaces);
    
    // Ajouter les voisins et leurs liens
    let neighbors = state.neighbors.lock().await;
    for (neighbor_ip, neighbor) in neighbors.iter() {
        // Ajouter le voisin s'il n'existe pas
        if !topology.nodes.contains_key(neighbor_ip) {
            topology.add_router(neighbor_ip.clone(), Vec::new());
        }
        
        // Ajouter le lien si le voisin est actif
        if neighbor.link_up {
            topology.add_link(
                state.local_ip.clone(),
                neighbor_ip.clone(),
                true, // Lien actif
            );
        }
    }
    drop(neighbors);
    
    topology
}

/// Calcule et met à jour les routes optimales
pub async fn calculate_and_update_optimal_routes(state: Arc<AppState>) -> Result<()> {
    debug!("Calcul des routes optimales en cours...");
    
    // Construire la topologie
    let topology = build_network_topology(Arc::clone(&state)).await;
    
    // Calculer les meilleurs chemins
    let routes = topology.calculate_shortest_paths(&state.local_ip);
    
    if routes.is_empty() {
        debug!("Aucune route calculée - routeur probablement isolé");
        return Ok(());
    }
    
    // Mettre à jour la table de routage locale
    let mut routing_table = state.routing_table.lock().await;
    routing_table.clear();
    
    for (destination, route) in &routes {
        routing_table.insert(
            destination.clone(),
            (route.next_hop.clone(), RouteState::Active(route.total_cost)),
        );
    }
    drop(routing_table);
    
    // Mettre à jour la table de routage système (optionnel)
    for (destination, route) in &routes {
        if let Err(e) = update_system_route(destination, &route.next_hop).await {
            warn!("Échec de la mise à jour de la route système vers {}: {}", destination, e);
        }
    }
    
    debug!("Calcul des routes terminé. {} routes optimales calculées.", routes.len());
    Ok(())
}

/// Met à jour une route dans la table de routage système
async fn update_system_route(destination: &str, gateway: &str) -> Result<()> {
    // Implémentation simplifiée - dans un vrai système, utiliser netlink ou ip route
    debug!("Route système: {} via {}", destination, gateway);
    Ok(())
}

/// Calcule les meilleurs chemins vers tous les réseaux IP connus
pub async fn calculate_best_paths_to_networks(state: Arc<AppState>) -> Result<HashMap<String, NetworkPathInfo>> {
    debug!("Calcul des meilleurs chemins vers tous les réseaux IP...");
    
    // Construire la topologie réseau
    let topology = build_network_topology(Arc::clone(&state)).await;
    
    // Obtenir les réseaux locaux
    let local_networks = crate::net_utils::get_local_networks()?;
    
    // Calculer les chemins depuis le routeur local vers tous les autres routeurs
    let routes = topology.calculate_shortest_paths(&state.local_ip);
    
    let mut network_paths = HashMap::new();
    
    // Ajouter les réseaux directement connectés (distance 0)
    for (network_cidr, (interface_name, ip_network)) in &local_networks {
        network_paths.insert(network_cidr.clone(), NetworkPathInfo {
            network_cidr: network_cidr.clone(),
            next_hop: None, // Directement connecté
            hop_count: 0,
            path: vec![state.local_ip.clone()],
            is_reachable: true,
            interface_name: Some(interface_name.clone()),
            route_type: NetworkRouteType::DirectlyConnected,
        });
    }
    
    // Pour chaque routeur atteignable, récupérer ses réseaux annoncés
    let neighbors = state.neighbors.lock().await;
    for (router_ip, route) in &routes {
        // Simuler les réseaux annoncés par chaque routeur
        // Dans un vrai OSPF, ces informations viendraient des LSA
        let announced_networks = get_networks_announced_by_router(router_ip);
        
        for network_cidr in announced_networks {
            // Ne pas écraser les réseaux directement connectés
            if !network_paths.contains_key(&network_cidr) {
                network_paths.insert(network_cidr.clone(), NetworkPathInfo {
                    network_cidr: network_cidr.clone(),
                    next_hop: Some(route.next_hop.clone()),
                    hop_count: route.hop_count,
                    path: route.path.clone(),
                    is_reachable: route.is_reachable,
                    interface_name: None,
                    route_type: NetworkRouteType::Remote,
                });
            }
        }
    }
    drop(neighbors);
    
    info!("Calcul terminé : {} réseaux trouvés", network_paths.len());
    for (network, path_info) in &network_paths {
        match path_info.route_type {
            NetworkRouteType::DirectlyConnected => {
                info!("Réseau {} : directement connecté via {}", 
                    network, path_info.interface_name.as_ref().unwrap_or(&"?".to_string()));
            },
            NetworkRouteType::Remote => {
                info!("Réseau {} : {} sauts via {} (chemin: {:?})", 
                    network, path_info.hop_count, 
                    path_info.next_hop.as_ref().unwrap_or(&"?".to_string()), 
                    path_info.path);
            }
        }
    }
    
    Ok(network_paths)
}

/// Informations sur le chemin vers un réseau
#[derive(Debug, Clone)]
pub struct NetworkPathInfo {
    pub network_cidr: String,
    pub next_hop: Option<String>, // None si directement connecté
    pub hop_count: u32,
    pub path: Vec<String>,
    pub is_reachable: bool,
    pub interface_name: Option<String>, // Pour les réseaux directement connectés
    pub route_type: NetworkRouteType,
}

#[derive(Debug, Clone)]
pub enum NetworkRouteType {
    DirectlyConnected,
    Remote,
}

/// Simule la récupération des réseaux annoncés par un routeur
/// Dans un vrai OSPF, cela viendrait des LSA Router et Network
fn get_networks_announced_by_router(router_ip: &str) -> Vec<String> {
    // Simulation simple - dans la réalité, cela viendrait de la base de données OSPF
    match router_ip {
        "192.168.1.1" => vec!["10.1.0.0/24".to_string(), "172.16.1.0/24".to_string()],
        "192.168.1.2" => vec!["10.2.0.0/24".to_string(), "172.16.2.0/24".to_string()],
        "192.168.1.3" => vec!["10.3.0.0/24".to_string(), "172.16.3.0/24".to_string()],
        _ => vec![format!("10.{}.0.0/24", router_ip.split('.').last().unwrap_or("0"))],
    }
}

/// Affiche un résumé complet de la topologie et des chemins optimaux
pub async fn print_network_topology_summary(state: Arc<AppState>) -> Result<()> {
    println!("\n=== ANALYSE DE LA TOPOLOGIE RÉSEAU ===");
    
    // Construire la topologie
    let topology = build_network_topology(Arc::clone(&state)).await;
    
    println!("\n1. ROUTEURS DANS LA TOPOLOGIE :");
    for (router_id, node) in &topology.nodes {
        let status = if node.is_reachable { "ACTIF" } else { "INACTIF" };
        println!("   - {} ({})", router_id, status);
        for interface in &node.interfaces {
            let link_status = if interface.is_active { "UP" } else { "DOWN" };
            println!("     └─ Interface {}: {} [{}]", 
                interface.name, interface.network, link_status);
        }
    }
    
    println!("\n2. LIENS RÉSEAU :");
    let mut displayed_links = HashSet::new();
    for link in &topology.links {
        let link_key = if link.from < link.to {
            format!("{}↔{}", link.from, link.to)
        } else {
            format!("{}↔{}", link.to, link.from)
        };
        
        if !displayed_links.contains(&link_key) {
            displayed_links.insert(link_key);
            let status = if link.is_active { "ACTIF" } else { "INACTIF" };
            println!("   - {} ↔ {} [{}] - Coût: {}", 
                link.from, link.to, status, link.cost);
        }
    }
    
    println!("\n3. MEILLEURS CHEMINS DEPUIS {} :", state.local_ip);
    let routes = topology.calculate_shortest_paths(&state.local_ip);
    
    if routes.is_empty() {
        println!("   Aucune route trouvée - Routeur isolé");
    } else {
        for (dest, route) in &routes {
            println!("   Vers {} : {} sauts via {} (chemin: {})", 
                dest, route.hop_count, route.next_hop, 
                route.path.join(" → "));
        }
    }
    
    println!("\n4. RÉSEAUX IP ATTEIGNABLES :");
    match calculate_best_paths_to_networks(Arc::clone(&state)).await {
        Ok(network_paths) => {
            for (network, path_info) in &network_paths {
                match path_info.route_type {
                    NetworkRouteType::DirectlyConnected => {
                        println!("   {} : Directement connecté via {}", 
                            network, path_info.interface_name.as_ref().unwrap_or(&"?".to_string()));
                    },
                    NetworkRouteType::Remote => {
                        println!("   {} : {} sauts via {} ({})", 
                            network, path_info.hop_count, 
                            path_info.next_hop.as_ref().unwrap_or(&"?".to_string()),
                            path_info.path.join(" → "));
                    }
                }
            }
        }
        Err(e) => println!("   Erreur lors du calcul des chemins réseau: {}", e),
    }
    
    println!("\n5. STATISTIQUES :");
    println!("   - Nombre de routeurs: {}", topology.nodes.len());
    println!("   - Nombre de liens: {}", topology.links.len() / 2); // Division par 2 car bidirectionnels
    println!("   - Routeurs atteignables: {}", routes.len());
    
    let active_links = topology.links.iter().filter(|l| l.is_active).count() / 2;
    println!("   - Liens actifs: {}", active_links);
    
    println!("\n==========================================\n");
    
    Ok(())
}