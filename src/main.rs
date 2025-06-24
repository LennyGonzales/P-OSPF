use routing_project::*;

mod types;
mod neighbor;
mod lsa;
mod init;
mod tasks;
mod packet_loop;
mod hello;
mod dijkstra;

use error::*;
use lsa::*;
use net_utils::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::net::{IpAddr, SocketAddr};
use tokio::net::UdpSocket;
use tokio::sync::Mutex;
use tokio::time::{self, Duration};
use std::sync::Arc;
use log::{debug, error, info, warn};
use pnet::ipnetwork::IpNetwork;
use std::error::Error as StdError;
use std::fmt;
use crate::types::{Neighbor, Router, LSAMessage, RouteState, HelloMessage};
use crate::neighbor::{update_neighbor, check_neighbor_timeouts};
use init::{init_logging_and_env, init_socket, init_state};
use tasks::{spawn_hello_and_lsa_tasks, spawn_neighbor_timeout_task};
use packet_loop::main_loop;

pub use hello::send_hello;

pub struct AppState {
    pub topology: Mutex<HashMap<String, Router>>,
    pub neighbors: Mutex<HashMap<String, Neighbor>>,
    pub routing_table: Mutex<HashMap<String, (String, RouteState)>>,
    pub processed_lsa: Mutex<HashSet<(String, u32)>>,
    pub local_ip: String,
    pub enabled: Mutex<bool>,
    pub config: read_config::RouterConfig,
    pub key: Vec<u8>,
}

impl AppState {
    /// Active le protocole OSPF
    pub async fn enable(&self) {
        let mut enabled = self.enabled.lock().await;
        *enabled = true;
    }
    
    /// Désactive le protocole OSPF
    pub async fn disable(&self) {
        let mut enabled = self.enabled.lock().await;
        *enabled = false;
    }
    
    /// Vérifie si le protocole OSPF est activé
    pub async fn is_enabled(&self) -> bool {
        *self.enabled.lock().await
    }
}

const PORT: u16 = 5000;
const HELLO_INTERVAL_SEC: u64 = 20;
const LSA_INTERVAL_SEC: u64 = 30;
const NEIGHBOR_TIMEOUT_SEC: u64 = 60;
const INITIAL_TTL: u8 = 64;

#[tokio::main]
async fn main() -> std::result::Result<(), Box<dyn std::error::Error>> {
    init_logging_and_env();
    
    // Charger la configuration basée sur le hostname
    let config = read_config::read_router_config()?;
    info!("Configuration chargée pour le routeur avec {} interfaces", config.interfaces.len());
    
    let router_ip = get_local_ip()?;
    info!("Hostname: {}", hostname::get()?.to_string_lossy());
    let socket = init_socket(PORT).await?;
    let key = config.key
        .as_ref()
        .map(|k| base64::decode(k).unwrap_or_else(|_| k.as_bytes().to_vec()))
        .unwrap_or_else(|| vec![0u8; 32]); // fallback si pas de clé
    let state = init_state(router_ip.clone(), config, key);
    
    // Calculer les routes initiales
    if let Err(e) = dijkstra::calculate_and_update_optimal_routes(Arc::clone(&state)).await {
        warn!("Échec du calcul initial des routes: {}", e);
    }
    
    spawn_hello_and_lsa_tasks(Arc::clone(&socket), Arc::clone(&state));
    spawn_neighbor_timeout_task(Arc::clone(&state));
    
    main_loop(socket, state).await?;
    Ok(())
}