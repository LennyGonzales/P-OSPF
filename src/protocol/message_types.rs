// This file defines the various message types used in the protocol, including request and response formats.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProtocolMessage {
    Hello(HelloMessage),
    LinkState(LinkStateUpdate),
    RouteRequest(RouteRequest),
    RouteResponse(RouteResponse),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloMessage {
    pub router_id: String,
    pub sequence_number: u32,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkStateUpdate {
    pub router_id: String,
    pub sequence_number: u32,
    pub links: Vec<LinkInfo>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkInfo {
    pub neighbor_id: String,
    pub interface_ip: String,
    pub metric: u32,
    pub is_active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteRequest {
    pub destination: String,
    pub source: String,
    pub request_id: u32,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteResponse {
    pub destination: String,
    pub next_hop: String,
    pub metric: u32,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NeighborInfo {
    pub router_id: String,
    pub ip_address: String,
    pub last_seen: u64,
    pub is_active: bool,
}