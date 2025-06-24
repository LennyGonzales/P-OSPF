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
                let next_hop = if path.len() > 1 { path[1].clone() } else { dest.clone() };
                
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
    info!("Calcul des routes optimales avec Dijkstra en cours...");
    
    let mut routes_to_add = HashMap::new();
    
    // Obtenir les interfaces locales
    let local_interfaces = get_local_interface_networks().await;
    info!("Interfaces locales détectées: {:?}", local_interfaces);
    
    // Construire la topologie globale à partir des LSAs
    let topology = build_topology_from_lsas(Arc::clone(&state)).await;
    info!("Topologie construite: {} nœuds, {} liens", topology.nodes.len(), topology.links.len());
    
    // Découvrir tous les réseaux accessibles depuis nos voisins et les LSAs
    let discoverable_networks = discover_remote_networks(Arc::clone(&state), &local_interfaces).await;
    info!("Réseaux distants découverts: {:?}", discoverable_networks);
    
    // Pour chaque réseau distant, calculer la meilleure route
    for (network_cidr, gateway_routers) in discoverable_networks {
        // Vérifier que ce n'est pas un réseau local
        if is_local_network(&network_cidr, &local_interfaces) {
            info!("Réseau {} est local, ignoré", network_cidr);
            continue;
        }
        
        // Trouver le meilleur routeur pour atteindre ce réseau
        let mut best_route: Option<RouteInfo> = None;
        
        for gateway_router in gateway_routers {
            // Exécuter Dijkstra vers ce routeur passerelle
            let shortest_paths = topology.calculate_shortest_paths(&state.local_ip);
            
            if let Some(route_to_gateway) = shortest_paths.get(&gateway_router) {
                if route_to_gateway.is_reachable && route_to_gateway.hop_count > 0 {
                    // Extraire l'IP pure du next-hop (enlever le masque s'il y en a un)
                    let clean_next_hop = extract_ip_from_address(&route_to_gateway.next_hop);
                    
                    // Parser le réseau cible
                    if let Ok(target_network) = network_cidr.parse::<pnet::ipnetwork::Ipv4Network>() {
                        let network_route = RouteInfo {
                            destination: network_cidr.clone(),
                            network: IpNetwork::V4(target_network),
                            next_hop: clean_next_hop,
                            total_cost: route_to_gateway.total_cost,
                            hop_count: route_to_gateway.hop_count,
                            bottleneck_capacity: route_to_gateway.bottleneck_capacity,
                            path: route_to_gateway.path.clone(),
                            is_reachable: true,
                        };
                        
                        // Choisir la meilleure route (coût le plus faible)
                        if best_route.is_none() || network_route.total_cost < best_route.as_ref().unwrap().total_cost {
                            best_route = Some(network_route);
                        }
                    }
                }
            }
        }
        
        // Ajouter la meilleure route trouvée
        if let Some(route) = best_route {
            info!("Route vers réseau {} via {} (coût: {}, sauts: {})", 
                  route.destination, route.next_hop, route.total_cost, route.hop_count);
            routes_to_add.insert(route.destination.clone(), route);
        }
    }
    
    info!("Routes à installer: {} routes", routes_to_add.len());
    
    if routes_to_add.is_empty() {
        info!("Aucune route à ajouter");
        return Ok(());
    }
    
    // Mettre à jour la table de routage système
    update_routing_table_safe(Arc::clone(&state), &routes_to_add).await?;
    info!("Table de routage mise à jour avec {} routes Dijkstra", routes_to_add.len());
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
        // Ajouter le routeur originator s'il n'existe pas
        if !topology.nodes.contains_key(originator) {
            topology.add_router(originator.clone(), Vec::new());
            info!("Ajouté routeur depuis LSA: {}", originator);
        }
        
        // Ajouter des liens basés sur les voisins annoncés dans la LSA
        for neighbor in &lsa.neighbors {
            if neighbor.link_up {
                // Ajouter le routeur voisin s'il n'existe pas
                if !topology.nodes.contains_key(&neighbor.neighbor_ip) {
                    topology.add_router(neighbor.neighbor_ip.clone(), Vec::new());
                    info!("Ajouté routeur voisin depuis LSA: {}", neighbor.neighbor_ip);
                }
                
                // Ajouter le lien bidirectionnel
                topology.add_link(
                    originator.clone(),
                    neighbor.neighbor_ip.clone(),
                    neighbor.capacity,
                    neighbor.link_up,
                );
                info!("Ajouté lien: {} <-> {} (capacité: {} Mbps)", 
                      originator, neighbor.neighbor_ip, neighbor.capacity);
            }
        }
    }
    
    // Ajouter nos propres voisins directs
    for (neighbor_ip, neighbor) in neighbors.iter() {
        if neighbor.link_up {
            // Ajouter le voisin s'il n'existe pas
            if !topology.nodes.contains_key(neighbor_ip) {
                topology.add_router(neighbor_ip.clone(), Vec::new());
                info!("Ajouté notre voisin direct: {}", neighbor_ip);
            }
            
            // Ajouter le lien bidirectionnel avec notre routeur
            topology.add_link(
                state.local_ip.clone(),
                neighbor_ip.clone(),
                neighbor.capacity,
                neighbor.link_up,
            );
            info!("Ajouté notre lien direct: {} <-> {} (capacité: {} Mbps)", 
                  state.local_ip, neighbor_ip, neighbor.capacity);
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
    
    // NE PAS nettoyer toutes les routes - seulement les routes OSPF spécifiques
    // Les routes locales (scope link) ne doivent pas être touchées
    
    // Vider seulement la table de routage en mémoire (pas le système)
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

/// Nettoie toutes les routes OSPF de la table de routage système
async fn cleanup_system_routes(old_routes: &HashMap<String, (String, RouteState, IpNetwork)>, local_interfaces: &HashMap<String, IpNetwork>) -> Result<()> {
    info!("Nettoyage des anciennes routes OSPF (garder les routes locales)");
    
    for (destination, (gateway, _, network)) in old_routes {
        // Ne supprimer que les routes OSPF (via gateway), pas les routes locales
        if !gateway.is_empty() {
            // Vérifier que ce n'est pas une route vers un réseau local
            let mut is_local_network = false;
            if let Ok(dest_network) = destination.parse::<pnet::ipnetwork::Ipv4Network>() {
                for (_, local_net) in local_interfaces {
                    if let IpNetwork::V4(local_net_v4) = local_net {
                        if dest_network.network() == local_net_v4.network() {
                            is_local_network = true;
                            info!("Garde la route vers {} car c'est un réseau local", destination);
                            break;
                        }
                    }
                }
            }
            
            if !is_local_network {
                info!("Suppression de l'ancienne route OSPF: {} via {}", destination, gateway);
                let _ = remove_system_route(destination, *network).await; // Ignore les erreurs
            }
        }
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

/// Découvre tous les réseaux distants accessibles via nos voisins et les LSAs
async fn discover_remote_networks(state: Arc<AppState>, local_interfaces: &HashMap<String, IpNetwork>) -> HashMap<String, Vec<String>> {
    let mut networks = HashMap::new();
    
    // Examiner tous les voisins directs pour découvrir leurs réseaux
    let neighbors = state.neighbors.lock().await;
    for (neighbor_ip, neighbor) in neighbors.iter() {
        if neighbor.link_up {
            // Déduire le réseau du voisin à partir de son IP
            if let Ok(neighbor_addr) = neighbor_ip.parse::<Ipv4Addr>() {
                // Supposer un masque /24 pour simplifier
                let network_cidr = format!("{}.0/24", 
                    format!("{}.{}.{}", neighbor_addr.octets()[0], neighbor_addr.octets()[1], neighbor_addr.octets()[2]));
                
                // Ajouter ce réseau avec le voisin comme passerelle
                networks.entry(network_cidr)
                    .or_insert_with(Vec::new)
                    .push(neighbor_ip.clone());
            }
        }
    }
    
    // Examiner les LSAs pour découvrir d'autres réseaux
    let lsa_database = state.lsa_database.lock().await;
    for (originator, lsa) in lsa_database.iter() {
        info!("Processing LSA from router: {}", originator);
        
        // NOUVEAU: Parcourir la routing_table de cette LSA pour découvrir tous les réseaux annoncés
        for (network_cidr, route_state) in &lsa.routing_table {
            // Ne considérer que les routes actives
            if matches!(route_state, RouteState::Active(_)) {
                info!("Discovered remote network: {} from LSA router: {}", network_cidr, originator);
                // Ajouter ce réseau avec l'originator comme routeur qui l'annonce
                networks.entry(network_cidr.clone())
                    .or_insert_with(Vec::new)
                    .push(originator.clone());
            }
        }
        
        // Aussi parcourir les voisins pour déduire leurs réseaux potentiels (comme avant)
        for neighbor_info in &lsa.neighbors {
            if neighbor_info.link_up {
                if let Ok(neighbor_addr) = neighbor_info.neighbor_ip.parse::<Ipv4Addr>() {
                    let network_cidr = format!("{}.0/24", 
                        format!("{}.{}.{}", neighbor_addr.octets()[0], neighbor_addr.octets()[1], neighbor_addr.octets()[2]));
                    
                    // Ajouter ce réseau avec l'originator comme passerelle possible
                    networks.entry(network_cidr)
                        .or_insert_with(Vec::new)
                        .push(originator.clone());
                }
            }
        }
    }
    
    drop(neighbors);
    drop(lsa_database);
    
    // Filtrer les réseaux locaux
    networks.retain(|network_cidr, _| !is_local_network(network_cidr, local_interfaces));
    
    info!("Réseaux distants découverts: {:?}", networks);
    for (network, routers) in &networks {
        info!("Network: {} announced by routers: {:?}", network, routers);
    }
    
    networks
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

/// Extrait l'adresse IP pure d'une chaîne qui peut contenir un masque
fn extract_ip_from_address(address: &str) -> String {
    // Si l'adresse contient un '/', prendre seulement la partie avant
    if let Some(slash_pos) = address.find('/') {
        address[..slash_pos].to_string()
    } else {
        address.to_string()
    }
}