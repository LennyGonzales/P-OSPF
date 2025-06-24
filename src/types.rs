// Définitions des structures et enums partagées

use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RouteState {
    Active(u32),
    Unreachable,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct HelloMessage {
    pub message_type: u8,
    pub router_ip: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Neighbor {
    pub neighbor_ip: String,
    pub link_up: bool,
    pub capacity: u32,
    pub last_seen: u64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LSAMessage {
    pub message_type: u8,
    pub router_ip: String,
    pub last_hop: Option<String>,
    pub originator: String,
    pub seq_num: u32,
    pub neighbor_count: usize,
    pub neighbors: Vec<Neighbor>,
    pub routing_table: HashMap<String, RouteState>, // Clé = préfixe réseau CIDR (ex: "10.2.0.0/24")
    pub path: Vec<String>,
    pub ttl: u8,
}

#[derive(Debug, Clone)]
pub struct Router {}

#[derive(Debug, Clone)]
pub struct InterfaceState {
    pub name: String,
    pub capacity_mbps: u32,
    pub link_active: bool,
    pub ip_address: Option<String>,
    pub network: Option<String>,
    pub last_state_change: u64,
}

impl InterfaceState {
    pub fn new(name: String, capacity_mbps: u32, link_active: bool) -> Self {
        Self {
            name,
            capacity_mbps,
            link_active,
            ip_address: None,
            network: None,
            last_state_change: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_else(|_| std::time::Duration::from_secs(0))
                .as_secs(),
        }
    }
    
    /// Met à jour l'état du lien
    pub fn set_link_state(&mut self, active: bool) {
        if self.link_active != active {
            self.link_active = active;
            self.last_state_change = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_else(|_| std::time::Duration::from_secs(0))
                .as_secs();
        }
    }
    
    /// Vérifie si le lien est actif
    pub fn is_link_up(&self) -> bool {
        self.link_active
    }
    
    /// Obtient le coût OSPF basé sur la capacité
    pub fn get_ospf_cost(&self) -> u32 {
        if !self.link_active {
            return u32::MAX; // Coût infini pour les liens inactifs
        }
        
        if self.capacity_mbps == 0 {
            return u32::MAX;
        }
        
        let reference_bandwidth = 100_000_000; // 100 Mbps en bps
        let bandwidth_bps = self.capacity_mbps * 1_000;
        let cost = reference_bandwidth / bandwidth_bps;
        cost.max(1) // Le coût minimum est 1
    }
}
