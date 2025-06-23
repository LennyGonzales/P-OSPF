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
                
                routes.insert(dest.clone(), RouteInfo {
                    destination: dest.clone(),
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
        return u32::MAX;
    }
    
    if capacity_mbps == 0 {
        return u32::MAX;
    }
    
    // Formule améliorée avec échelle logarithmique pour mieux différencier les capacités
    let reference_bandwidth = 100_000_000; // 100 Mbps
    let bandwidth_bps = capacity_mbps * 1_000_000;
    
    // Utiliser une échelle logarithmique pour amplifier les différences entre liens
    let log_factor = (reference_bandwidth as f64 / bandwidth_bps as f64).log10() * 10.0;
    
    // Assurer que le coût reste dans une plage raisonnable
    let cost = (log_factor.max(0.0) * 10.0) as u32;
    cost.max(1).min(65535)
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