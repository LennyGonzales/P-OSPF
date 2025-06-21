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
