// Exemple d'utilisation des fonctionnalités d'état des liens

use std::sync::Arc;
use log::info;

/// Fonction d'exemple pour tester les fonctionnalités d'état des liens
pub async fn test_interface_states(state: Arc<crate::AppState>) {
    info!("=== TEST DES ÉTATS D'INTERFACES ===");
    
    // Afficher le rapport d'état des interfaces
    crate::neighbor::display_interface_report(&state).await;
    
    // Accéder directement aux interfaces configurées
    info!("Nombre d'interfaces configurées: {}", state.config.interfaces.len());
    
    // Simuler des changements d'état (dans un vrai système, cela viendrait du matériel)
    for interface in &state.config.interfaces {
        if interface.link_active {
            info!("Interface {} opérationnelle - calcul du coût OSPF", interface.name);
            let cost = calculate_ospf_cost(interface.capacity_mbps);
            info!("Coût OSPF pour {} ({} Mbps): {}", interface.name, interface.capacity_mbps, cost);
        } else {
            info!("Interface {} hors service - routage indisponible", interface.name);
        }
    }
    
    // Afficher les voisins et leur état en relation avec les interfaces
    let neighbors = state.neighbors.lock().await;
    info!("=== ÉTAT DES VOISINS PAR RAPPORT AUX INTERFACES ===");
    for (neighbor_ip, neighbor) in neighbors.iter() {
        let status = if neighbor.link_up { "UP" } else { "DOWN" };
        info!("Voisin {}: {} (capacité: {} Mbps)", 
              neighbor_ip, status, neighbor.capacity);
    }
    drop(neighbors);
}

/// Calcule le coût OSPF basé sur la capacité
fn calculate_ospf_cost(capacity_mbps: u32) -> u32 {
    if capacity_mbps == 0 {
        return u32::MAX;
    }
    
    let reference_bandwidth = 100_000_000; // 100 Mbps en bps
    let bandwidth_bps = capacity_mbps * 1_000_000;
    let cost = reference_bandwidth / bandwidth_bps;
    cost.max(1)
}

/// Exemple d'affichage des capacités et coûts pour toutes les configurations
pub fn display_all_interface_costs() {
    info!("=== TABLEAU DES COÛTS OSPF PAR CAPACITÉ ===");
    info!("{:<12} {:<10}", "Capacité", "Coût OSPF");
    info!("{}", "-".repeat(25));
    
    let capacities = vec![10, 100, 500, 1000, 10000];
    for capacity in capacities {
        let cost = calculate_ospf_cost(capacity);
        info!("{:<12} {:<10}", format!("{} Mbps", capacity), cost);
    }
}

/// Fonction pour simuler des changements d'état d'interface
pub async fn simulate_interface_state_changes(state: Arc<crate::AppState>) {
    info!("=== SIMULATION DE CHANGEMENTS D'ÉTAT ===");
    
    // Dans un vrai système, ces changements viendraient des drivers réseau
    // Ici, on utilise la configuration statique pour déterminer l'état initial
    
    let mut active_count = 0;
    let mut total_capacity = 0;
    
    for interface in &state.config.interfaces {
        if interface.link_active {
            active_count += 1;
            total_capacity += interface.capacity_mbps;
            info!("Interface {} active - ajout de {} Mbps à la capacité totale", 
                  interface.name, interface.capacity_mbps);
        } else {
            info!("Interface {} inactive - pas de contribution à la capacité", 
                  interface.name);
        }
    }
    
    info!("Résumé: {} interfaces actives, capacité totale: {} Mbps", 
          active_count, total_capacity);
    
    // Calculer l'impact sur le routage
    if active_count == 0 {
        info!("ALERTE: Aucune interface active - routeur isolé!");
    } else if active_count == 1 {
        info!("ATTENTION: Une seule interface active - pas de redondance");
    } else {
        info!("OK: Plusieurs interfaces actives - redondance disponible");
    }
}
