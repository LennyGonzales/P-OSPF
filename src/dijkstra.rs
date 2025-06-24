// Module d'implémentation de l'algorithme de Dijkstra pour OSPF
// Calcul des meilleurs chemins basé sur les coûts, nombre de sauts et capacités

use std::collections::{HashMap, BinaryHeap, HashSet};
use std::cmp::Ordering;
use std::sync::Arc;
use std::net::{IpAddr, Ipv4Addr};
use log::{info, debug, warn, error};
use pnet::ipnetwork::{IpNetwork, Ipv4Network};
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
        // Priorité : 1) Coût total minimum, 2) Nombre de sauts minimum (plus court chemin), 3) Capacité goulot maximale
        other.total_cost.cmp(&self.total_cost)
            .then_with(|| other.hop_count.cmp(&self.hop_count))
            .then_with(|| self.bottleneck_capacity.cmp(&other.bottleneck_capacity))
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
    pub fn add_link(&mut self, from: String, to: String, capacity_mbps: u32, is_active: bool) {
        let cost = calculate_ospf_cost(capacity_mbps, is_active);
        
        // Lien direct
        self.links.push(NetworkLink {
            from: from.clone(),
            to: to.clone(),
            cost,
            capacity_mbps,
            is_active,
            hop_count: 1,
        });
        
        // Lien de retour (bidirectionnel)
        self.links.push(NetworkLink {
            from: to,
            to: from,
            cost,
            capacity_mbps,
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
    /// Basé sur : 1) Plus court chemin (nombre de sauts), 2) Capacité goulot, 3) État des liens
    pub fn calculate_shortest_paths(&self, source: &str) -> HashMap<String, RouteInfo> {
        let mut hop_counts: HashMap<String, u32> = HashMap::new();
        let mut bottleneck_capacities: HashMap<String, u32> = HashMap::new();
        let mut paths: HashMap<String, Vec<String>> = HashMap::new();
        let mut visited = HashSet::new();
        let mut heap = BinaryHeap::new();

        // Initialisation
        for node_id in self.nodes.keys() {
            hop_counts.insert(node_id.clone(), u32::MAX);
            bottleneck_capacities.insert(node_id.clone(), 0);
            paths.insert(node_id.clone(), Vec::new());
        }

        // Nœud source
        hop_counts.insert(source.to_string(), 0);
        bottleneck_capacities.insert(source.to_string(), u32::MAX);
        paths.insert(source.to_string(), vec![source.to_string()]);

        heap.push(DijkstraNode {
            router_id: source.to_string(),
            total_cost: 0,
            hop_count: 0,
            bottleneck_capacity: u32::MAX,
            path: vec![source.to_string()],
        });

        // Algorithme modifié pour la capacité goulot d'étranglement
        while let Some(current) = heap.pop() {
            if visited.contains(&current.router_id) {
                continue;
            }
            visited.insert(current.router_id.clone());

            // Explorer les voisins
            for link in self.get_active_neighbors(&current.router_id) {
                if visited.contains(&link.to) {
                    continue;
                }

                let new_hop_count = current.hop_count + 1;
                let new_bottleneck_capacity = current.bottleneck_capacity.min(link.capacity_mbps);
                
                let current_best_hops = *hop_counts.get(&link.to).unwrap_or(&u32::MAX);
                let current_best_capacity = *bottleneck_capacities.get(&link.to).unwrap_or(&0);

                // Critères de mise à jour : nombre de sauts principal, puis capacité goulot
                let should_update = new_hop_count < current_best_hops ||
                    (new_hop_count == current_best_hops && new_bottleneck_capacity > current_best_capacity);

                if should_update {
                    hop_counts.insert(link.to.clone(), new_hop_count);
                    bottleneck_capacities.insert(link.to.clone(), new_bottleneck_capacity);
                    
                    let mut new_path = current.path.clone();
                    new_path.push(link.to.clone());
                    paths.insert(link.to.clone(), new_path.clone());

                    heap.push(DijkstraNode {
                        router_id: link.to.clone(),
                        total_cost: new_hop_count,
                        hop_count: new_hop_count,
                        bottleneck_capacity: new_bottleneck_capacity,
                        path: new_path,
                    });
                }
            }
        }

        // Construire les résultats
        let mut routes = HashMap::new();
        for (dest, hops) in hop_counts {
            if dest != source && hops != u32::MAX {
                let path = paths.get(&dest).unwrap_or(&Vec::new()).clone();
                
                // AMÉLIORATION: Calculer le next-hop correct
                let next_hop = if path.len() > 1 {
                    // Utiliser le premier saut dans le chemin (voisin direct)
                    path[1].clone()
                } else {
                    // Si pas de chemin, utiliser la destination (voisin direct)
                    dest.clone()
                };
                
                // Créer un réseau /32 par défaut pour cette destination
                let network = match dest.parse::<Ipv4Addr>() {
                    Ok(ip) => IpNetwork::V4(pnet::ipnetwork::Ipv4Network::new(ip, 32).unwrap()),
                    Err(_) => continue, // Skip invalid destinations
                };
                
                routes.insert(dest.clone(), RouteInfo {
                    destination: dest.clone(),
                    network,
                    next_hop,
                    total_cost: hops,
                    hop_count: hops,
                    bottleneck_capacity: *bottleneck_capacities.get(&dest).unwrap_or(&0),
                    path,
                    is_reachable: true,
                });
            }
        }

        routes
    }
    
    /// Calcule les meilleurs chemins avec les vrais préfixes de réseau
    pub fn calculate_shortest_paths_with_networks(&self, source: &str, interface_networks: &HashMap<String, IpNetwork>) -> HashMap<String, RouteInfo> {
        let mut hop_counts: HashMap<String, u32> = HashMap::new();
        let mut bottleneck_capacities: HashMap<String, u32> = HashMap::new();
        let mut paths: HashMap<String, Vec<String>> = HashMap::new();
        let mut visited = HashSet::new();
        let mut heap = BinaryHeap::new();

        // Initialisation
        for node_id in self.nodes.keys() {
            hop_counts.insert(node_id.clone(), u32::MAX);
            bottleneck_capacities.insert(node_id.clone(), 0);
            paths.insert(node_id.clone(), Vec::new());
        }

        // Nœud source
        hop_counts.insert(source.to_string(), 0);
        bottleneck_capacities.insert(source.to_string(), u32::MAX);
        paths.insert(source.to_string(), vec![source.to_string()]);

        heap.push(DijkstraNode {
            router_id: source.to_string(),
            total_cost: 0,
            hop_count: 0,
            bottleneck_capacity: u32::MAX,
            path: vec![source.to_string()],
        });

        // Algorithme modifié pour la capacité goulot d'étranglement
        while let Some(current) = heap.pop() {
            if visited.contains(&current.router_id) {
                continue;
            }
            visited.insert(current.router_id.clone());

            // Explorer les voisins
            for link in self.get_active_neighbors(&current.router_id) {
                if visited.contains(&link.to) {
                    continue;
                }

                let new_hop_count = current.hop_count + 1;
                let new_bottleneck_capacity = current.bottleneck_capacity.min(link.capacity_mbps);
                
                let current_best_hops = *hop_counts.get(&link.to).unwrap_or(&u32::MAX);
                let current_best_capacity = *bottleneck_capacities.get(&link.to).unwrap_or(&0);

                // Critères de mise à jour : nombre de sauts principal, puis capacité goulot
                let should_update = new_hop_count < current_best_hops ||
                    (new_hop_count == current_best_hops && new_bottleneck_capacity > current_best_capacity);

                if should_update {
                    hop_counts.insert(link.to.clone(), new_hop_count);
                    bottleneck_capacities.insert(link.to.clone(), new_bottleneck_capacity);
                    
                    let mut new_path = current.path.clone();
                    new_path.push(link.to.clone());
                    paths.insert(link.to.clone(), new_path.clone());

                    heap.push(DijkstraNode {
                        router_id: link.to.clone(),
                        total_cost: new_hop_count,
                        hop_count: new_hop_count,
                        bottleneck_capacity: new_bottleneck_capacity,
                        path: new_path,
                    });
                }
            }
        }

        // Construire les résultats avec les vrais réseaux
        let mut routes = HashMap::new();
        for (dest, hops) in hop_counts {
            if dest != source && hops != u32::MAX {
                let path = paths.get(&dest).unwrap_or(&Vec::new()).clone();
                
                // Pour les voisins directs (1 saut), utiliser l'IP du voisin comme next_hop
                // Pour les destinations plus lointaines, utiliser le premier routeur du chemin
                let next_hop = if hops == 1 {
                    dest.clone() // Voisin direct
                } else if path.len() > 1 {
                    path[1].clone() // Premier routeur du chemin
                } else {
                    dest.clone()
                };
                
                // Trouver le réseau correspondant à cette destination
                let network = interface_networks.get(&dest).cloned()
                    .unwrap_or_else(|| {
                        // Fallback: créer un réseau /32 si pas trouvé
                        match dest.parse::<Ipv4Addr>() {
                            Ok(ip) => IpNetwork::V4(pnet::ipnetwork::Ipv4Network::new(ip, 32).unwrap()),
                            Err(_) => {
                                // Skip invalid destinations
                                return IpNetwork::V4(pnet::ipnetwork::Ipv4Network::new(Ipv4Addr::new(0, 0, 0, 0), 32).unwrap());
                            }
                        }
                    });
                
                routes.insert(dest.clone(), RouteInfo {
                    destination: dest.clone(),
                    network,
                    next_hop,
                    total_cost: hops,
                    hop_count: hops,
                    bottleneck_capacity: *bottleneck_capacities.get(&dest).unwrap_or(&0),
                    path,
                    is_reachable: true,
                });
            }
        }

        routes
    }
}

/// Informations sur une route calculée
#[derive(Debug, Clone)]
pub struct RouteInfo {
    pub destination: String,
    pub network: IpNetwork, // Réseau avec préfixe
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
                neighbor.capacity,
                true,
            );
        }
    }
    drop(neighbors);
    
    topology
}

/// Construit la topologie réseau globale à partir de toutes les informations de voisinage (LSAs)
pub async fn build_global_network_topology(local_state: Arc<AppState>, all_neighbors: Vec<(String, Vec<Neighbor>)>) -> NetworkTopology {
    let mut topology = NetworkTopology::new();
    // Ajouter tous les routeurs et leurs interfaces
    for (router_ip, neighbors) in &all_neighbors {
        if !topology.nodes.contains_key(router_ip) {
            topology.add_router(router_ip.clone(), Vec::new());
        }
        for neighbor in neighbors {
            if !topology.nodes.contains_key(&neighbor.neighbor_ip) {
                topology.add_router(neighbor.neighbor_ip.clone(), Vec::new());
            }
            if neighbor.link_up {
                topology.add_link(
                    router_ip.clone(),
                    neighbor.neighbor_ip.clone(),
                    neighbor.capacity,
                    true,
                );
            }
        }
    }
    // Ajouter le routeur local et ses interfaces réelles
    let local_interfaces = local_state.config.interfaces.iter().map(|iface| {
        InterfaceInfo {
            name: iface.name.clone(),
            network: format!("network_{}", iface.name),
            capacity_mbps: iface.capacity_mbps,
            is_active: iface.link_active,
            connected_to: None,
        }
    }).collect();
    topology.add_router(local_state.local_ip.clone(), local_interfaces);
    topology
}

/// Calcule et met à jour les routes optimales en utilisant l'algorithme de Dijkstra
/// basé sur les LSAs stockées dans la base de données
pub async fn calculate_and_update_optimal_routes(state: Arc<AppState>) -> Result<()> {
    info!("Recalcul Dijkstra suite à réception LSA...");
    
    // 1. Construire la topologie complète à partir des LSAs
    let topology = build_topology_from_lsas(Arc::clone(&state)).await;
    info!("Topologie: {} nœuds, {} liens", topology.nodes.len(), topology.links.len());
    
    // 2. Calculer tous les meilleurs chemins depuis ce routeur
    let shortest_paths = topology.calculate_shortest_paths(&state.local_ip);
    info!("Dijkstra calculé: {} destinations trouvées", shortest_paths.len());
    
    // 3. Découvrir tous les réseaux annoncés et calculer les meilleures routes vers eux
    let mut routes_to_add: HashMap<String, RouteInfo> = HashMap::new();
    let local_interfaces = get_local_interface_networks().await;
    let lsa_database = state.lsa_database.lock().await;
    
    // Collecter tous les réseaux annoncés avec leurs routeurs sources
    let mut network_sources: HashMap<String, Vec<String>> = HashMap::new();
    
    for (originator, lsa) in lsa_database.iter() {
        let clean_originator = extract_ip_from_address(originator);
        
        // Parcourir les réseaux annoncés par ce routeur
        for (network_cidr, route_state) in &lsa.routing_table {
            if matches!(route_state, RouteState::Active(_)) {
                // Vérifier que ce n'est pas un réseau local
                if is_local_network(network_cidr, &local_interfaces) {
                    continue;
                }
                
                network_sources.entry(network_cidr.clone())
                    .or_insert_with(Vec::new)
                    .push(clean_originator.clone());
            }
        }
    }
    
    // Pour chaque réseau, trouver la meilleure route
    for (network_cidr, announcing_routers) in network_sources {
        let mut best_route: Option<RouteInfo> = None;
        
        // Tester chaque routeur qui annonce ce réseau
        for announcing_router in &announcing_routers {
            if let Some(path_to_announcer) = shortest_paths.get(announcing_router) {
                // Le next-hop est le premier saut vers le routeur qui annonce ce réseau
                let next_hop = if path_to_announcer.path.len() > 1 {
                    path_to_announcer.path[1].clone()
                } else {
                    announcing_router.clone() // Voisin direct
                };
                
                if let Ok(target_network) = network_cidr.parse::<pnet::ipnetwork::Ipv4Network>() {
                    let route = RouteInfo {
                        destination: network_cidr.clone(),
                        network: IpNetwork::V4(target_network),
                        next_hop: next_hop.clone(),
                        total_cost: path_to_announcer.total_cost,
                        hop_count: path_to_announcer.hop_count,
                        bottleneck_capacity: path_to_announcer.bottleneck_capacity,
                        path: path_to_announcer.path.clone(),
                        is_reachable: true,
                    };
                    
                    // Garder la meilleure route (coût minimum)
                    if best_route.is_none() || route.total_cost < best_route.as_ref().unwrap().total_cost {
                        best_route = Some(route);
                    }
                }
            }
        }
        
        // Ajouter la meilleure route trouvée
        if let Some(route) = best_route {
            info!("Meilleure route pour {}: via {} (coût: {}, annoncé par: {:?})", 
                  network_cidr, route.next_hop, route.total_cost, announcing_routers);
            routes_to_add.insert(network_cidr, route);
        }
    }
    
    drop(lsa_database);
    
    // 5. Mettre à jour la table de routage système
    if !routes_to_add.is_empty() {
        update_routing_table_safe(Arc::clone(&state), &routes_to_add).await?;
        info!("Table de routage mise à jour avec {} routes", routes_to_add.len());
    } else {
        info!("Aucune nouvelle route à ajouter");
    }
    
    Ok(())
}

/// Construit la topologie réseau complète à partir des LSAs stockées
async fn build_topology_from_lsas(state: Arc<AppState>) -> NetworkTopology {
    let mut topology = NetworkTopology::new();
    
    // Ajouter notre routeur local
    let local_interfaces = state.config.interfaces.iter().map(|iface| {
        InterfaceInfo {
            name: iface.name.clone(),
            network: format!("network_{}", iface.name), // Utiliser un nom simple
            capacity_mbps: iface.capacity_mbps,
            is_active: iface.link_active,
            connected_to: None,
        }
    }).collect();
    
    topology.add_router(state.local_ip.clone(), local_interfaces);
    info!("Ajouté routeur local: {}", state.local_ip);
    
    // Ajouter les nœuds et liens à partir des LSAs stockées
    let lsa_database = state.lsa_database.lock().await;
    let neighbors = state.neighbors.lock().await;
    
    for (originator, lsa) in lsa_database.iter() {
        // Nettoyer l'ID du routeur originator (enlever /24 s'il y en a un)
        let clean_originator = extract_ip_from_address(originator);
        
        // Ajouter le routeur originator s'il n'existe pas
        if !topology.nodes.contains_key(&clean_originator) {
            topology.add_router(clean_originator.clone(), Vec::new());
            info!("Ajouté routeur depuis LSA: {}", clean_originator);
        }
        
        // Ajouter des liens basés sur les voisins annoncés dans la LSA
        for neighbor in &lsa.neighbors {
            if neighbor.link_up {
                // Nettoyer l'ID du routeur voisin (enlever /24 s'il y en a un)
                let clean_neighbor_ip = extract_ip_from_address(&neighbor.neighbor_ip);
                
                // Ajouter le routeur voisin s'il n'existe pas
                if !topology.nodes.contains_key(&clean_neighbor_ip) {
                    topology.add_router(clean_neighbor_ip.clone(), Vec::new());
                    info!("Ajouté routeur voisin depuis LSA: {}", clean_neighbor_ip);
                }
                
                // Ajouter le lien bidirectionnel
                topology.add_link(
                    clean_originator.clone(),
                    clean_neighbor_ip.clone(),
                    neighbor.capacity,
                    neighbor.link_up,
                );
                info!("Ajouté lien: {} <-> {} (capacité: {} Mbps)", 
                      clean_originator, clean_neighbor_ip, neighbor.capacity);
            }
        }
    }
    
    // Ajouter nos propres voisins directs
    for (neighbor_ip, neighbor) in neighbors.iter() {
        if neighbor.link_up {
            // Nettoyer l'ID du voisin (enlever /24 s'il y en a un)
            let clean_neighbor_ip = extract_ip_from_address(neighbor_ip);
            
            // Ajouter le voisin s'il n'existe pas
            if !topology.nodes.contains_key(&clean_neighbor_ip) {
                topology.add_router(clean_neighbor_ip.clone(), Vec::new());
                info!("Ajouté notre voisin direct: {}", clean_neighbor_ip);
            }
            
            // AMÉLIORATION: S'assurer que nous n'ajoutons que des liens vers des voisins réellement accessibles
            // Vérifier que le voisin n'est pas déjà connecté
            let already_connected = topology.links.iter().any(|link| {
                (link.from == state.local_ip && link.to == clean_neighbor_ip) ||
                (link.from == clean_neighbor_ip && link.to == state.local_ip)
            });
            
            if !already_connected {
                // Ajouter le lien bidirectionnel avec notre routeur
                topology.add_link(
                    state.local_ip.clone(),
                    clean_neighbor_ip.clone(),
                    neighbor.capacity,
                    neighbor.link_up,
                );
                info!("Ajouté notre lien direct: {} <-> {} (capacité: {} Mbps)", 
                      state.local_ip, clean_neighbor_ip, neighbor.capacity);
            } else {
                info!("Lien déjà existant: {} <-> {}", state.local_ip, clean_neighbor_ip);
            }
        }
    }
    
    drop(lsa_database);
    drop(neighbors);
    
    info!("Topologie finale: {} nœuds, {} liens", topology.nodes.len(), topology.links.len());
    topology
}

/// Met à jour la table de routage locale de façon sécurisée
pub async fn update_routing_table_safe(state: Arc<AppState>, routes: &HashMap<String, RouteInfo>) -> Result<()> {
    let mut routing_table = state.routing_table.lock().await;
    
    routing_table.clear();
    
    // Mise à jour de la table de routage en mémoire
    for (destination, route) in routes {
        routing_table.insert(
            destination.clone(),
            (route.next_hop.clone(), RouteState::Active(route.total_cost)),
        );
    }
    
    // Mise à jour de la table de routage système uniquement pour les nouvelles routes
    for (destination, route) in routes {
        if let Err(e) = add_system_route(&destination, &route.next_hop, route.network).await {
            info!("Erreur lors de l'ajout de la route système {}: {}", destination, e);
        } else {
            info!("Route système ajoutée: {} via {}", destination, route.next_hop);
        }
    }
    
    info!("Table de routage mise à jour: {} routes en mémoire et système", routes.len());
    Ok(())
}

/// Ajoute une route dans la table de routage système
async fn add_system_route(destination: &str, gateway: &str, network: IpNetwork) -> Result<()> {
    use std::str::FromStr;
    
    // Parser l'adresse IP de la passerelle
    let gateway_ip = Ipv4Addr::from_str(gateway)
        .map_err(|e| crate::error::AppError::NetworkError(format!("Adresse IP gateway invalide {}: {}", gateway, e)))?;
    
    // Vérifier que la destination n'est pas la même que la passerelle (éviter les routes circulaires)
    if destination == gateway {
        info!("Éviter d'ajouter une route circulaire vers {}", destination);
        return Ok(());
    }
    
    // Utiliser net-route pour ajouter la route
    let handle = net_route::Handle::new()
        .map_err(|e| crate::error::AppError::RouteError(format!("Cannot create routing handle (permissions?): {}", e)))?;
    
    let (ip, prefix) = match network {
        IpNetwork::V4(net) => (IpAddr::V4(net.network()), net.prefix()),
        IpNetwork::V6(_) => {
            return Err(crate::error::AppError::RouteError("IPv6 not supported".to_string()));
        }
    };
    
    let route = net_route::Route::new(ip, prefix as u8)
        .with_gateway(IpAddr::V4(gateway_ip));
    
    match handle.add(&route).await {
        Ok(_) => {
            info!("Successfully added network route to {} via {}", network, gateway_ip);
            Ok(())
        },
        Err(e) => {
            info!("Route add failed, trying to update: {}", e);
            let _ = handle.delete(&route).await;
            match handle.add(&route).await {
                Ok(_) => {
                    info!("Successfully updated network route to {} via {}", network, gateway_ip);
                    Ok(())
                },
                Err(e2) => {
                    warn!("Failed to add/update route to {} via {}: {}", network, gateway_ip, e2);
                    Err(crate::error::AppError::RouteError(format!("Routing update failed: {}", e2)))
                }
            }
        }
    }
}

/// Supprime une route de la table de routage système
async fn remove_system_route(destination: &str, network: IpNetwork) -> Result<()> {
    // Utiliser net-route pour supprimer la route
    let handle = net_route::Handle::new()
        .map_err(|e| crate::error::AppError::RouteError(format!("Cannot create routing handle (permissions?): {}", e)))?;
    
    let (ip, prefix) = match network {
        IpNetwork::V4(net) => (IpAddr::V4(net.network()), net.prefix()),
        IpNetwork::V6(_) => {
            return Err(crate::error::AppError::RouteError("IPv6 not supported".to_string()));
        }
    };
    
    // Créer la route à supprimer
    let route = net_route::Route::new(ip, prefix as u8);
    
    // Supprimer la route (on ignore les erreurs car la route peut ne pas exister)
    if let Err(e) = handle.delete(&route).await {
        info!("Impossible de supprimer la route {}: {}", destination, e);
    } else {
        info!("Route système supprimée avec net-route: {}", destination);
    }
    
    Ok(())
}

/// Récupère les réseaux des interfaces locales du système
async fn get_local_interface_networks() -> HashMap<String, IpNetwork> {
    let mut networks = HashMap::new();
    
    // Utiliser pnet pour lister les interfaces réseau
    use pnet::datalink;
    
    for interface in datalink::interfaces() {
        if interface.is_up() && !interface.is_loopback() {
            for ip in &interface.ips {
                match ip {
                    pnet::ipnetwork::IpNetwork::V4(ipv4_net) => {
                        networks.insert(interface.name.clone(), IpNetwork::V4(*ipv4_net));
                        info!("Interface locale trouvée: {} -> {}", interface.name, ipv4_net);
                    }
                    pnet::ipnetwork::IpNetwork::V6(_) => {
                        // Ignorer IPv6 pour l'instant
                    }
                }
            }
        }
    }
    
    networks
}

/// Normalise une adresse IP/CIDR en adresse réseau
/// Convertit par exemple 192.168.2.1/24 en 192.168.2.0/24
fn normalize_to_network_address(cidr: &str) -> String {
    if let Ok(network) = cidr.parse::<pnet::ipnetwork::Ipv4Network>() {
        // Utiliser l'adresse réseau réelle, pas l'adresse d'hôte
        format!("{}/{}", network.network(), network.prefix())
    } else {
        // En cas d'erreur de parsing, retourner l'original
        cidr.to_string()
    }
}

/// Vérifie si un réseau est local (directement connecté)
fn is_local_network(network_cidr: &str, local_interfaces: &HashMap<String, IpNetwork>) -> bool {
    if let Ok(test_network) = network_cidr.parse::<pnet::ipnetwork::Ipv4Network>() {
        for (_, local_net) in local_interfaces {
            if let IpNetwork::V4(local_net_v4) = local_net {
                if test_network.network() == local_net_v4.network() && test_network.prefix() == local_net_v4.prefix() {
                    return true;
                }
            }
        }
    }
    false
}

/// Extrait l'adresse IP d'une chaîne qui peut contenir un suffixe CIDR
/// Par exemple: "10.1.0.2/24" -> "10.1.0.2"
fn extract_ip_from_address(address: &str) -> String {
    if let Some(pos) = address.find('/') {
        address[..pos].to_string()
    } else {
        address.to_string()
    }
}

// Fonction supprimée - logique simplifiée dans calculate_and_update_optimal_routes

/// Découvre tous les réseaux distants à partir des LSAs (version simplifiée)
async fn discover_all_networks_from_lsas(state: Arc<AppState>) -> HashMap<String, Vec<String>> {
    let mut networks = HashMap::new();
    
    let lsa_database = state.lsa_database.lock().await;
    
    // Parcourir toutes les LSAs pour extraire les réseaux annoncés
    for (originator, lsa) in lsa_database.iter() {
        let clean_originator = extract_ip_from_address(originator);
        
        // 1. Extraire les réseaux de la routing_table du LSA
        for (network_cidr, route_state) in &lsa.routing_table {
            // Ne considérer que les routes actives
            if matches!(route_state, RouteState::Active(_)) {
                networks.entry(network_cidr.clone())
                    .or_insert_with(Vec::new)
                    .push(clean_originator.clone());
                    
                info!("Réseau {} annoncé par {}", network_cidr, clean_originator);
            }
        }
        
        // 2. Inférer les réseaux à partir des IPs des voisins
        for neighbor_info in &lsa.neighbors {
            if neighbor_info.link_up {
                if let Ok(neighbor_addr) = neighbor_info.neighbor_ip.parse::<Ipv4Addr>() {
                    // Générer le réseau /24 basé sur l'IP du voisin
                    let network_cidr = format!("{}.{}.{}.0/24",
                        neighbor_addr.octets()[0], 
                        neighbor_addr.octets()[1], 
                        neighbor_addr.octets()[2]);
                    
                    networks.entry(network_cidr.clone())
                        .or_insert_with(Vec::new)
                        .push(clean_originator.clone());
                        
                    info!("Réseau {} inféré depuis voisin {} de {}", 
                          network_cidr, neighbor_info.neighbor_ip, clean_originator);
                }
            }
        }
    }
    
    drop(lsa_database);
    networks
}