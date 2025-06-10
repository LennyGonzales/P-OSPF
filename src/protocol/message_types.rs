// This file defines the various message types used in the protocol, including request and response formats.

#[derive(Debug, Clone)]
pub enum MessageType {
    Hello,
    RouteRequest,
    RouteResponse,
    UpdateRequest,
    UpdateResponse,
}

#[derive(Debug, Clone)]
pub struct HelloMessage {
    pub sender_ip: String,
    pub sender_name: String,
}

#[derive(Debug, Clone)]
pub struct RouteRequestMessage {
    pub source_ip: String,
    pub destination_ip: String,
}

#[derive(Debug, Clone)]
pub struct RouteResponseMessage {
    pub source_ip: String,
    pub destination_ip: String,
    pub next_hop: String,
    pub hops: u32,
    pub link_state: Vec<LinkState>,
}

#[derive(Debug, Clone)]
pub struct UpdateRequestMessage {
    pub source_ip: String,
    pub updates: Vec<RouteUpdate>,
}

#[derive(Debug, Clone)]
pub struct UpdateResponseMessage {
    pub source_ip: String,
    pub success: bool,
}

#[derive(Debug, Clone)]
pub struct LinkState {
    pub ip: String,
    pub is_active: bool,
    pub capacity: u32,
}

#[derive(Debug, Clone)]
pub struct RouteUpdate {
    pub destination_ip: String,
    pub next_hop: String,
    pub hops: u32,
}