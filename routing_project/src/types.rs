use serde::{Deserialize, Serialize};
use std::net::{Ipv4Addr};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct RouterConfig {
    pub router_id: Ipv4Addr,
    pub name: String,
    pub hello_interval: u32,
    pub dead_interval: u32,
    pub lsa_refresh_interval: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkState {
    Up,
    Down,
}

#[derive(Debug, Clone)]
pub struct Link {
    pub neighbor_id: Ipv4Addr,
    pub interface_addr: Ipv4Addr,
    pub cost: u32,
    pub capacity: u64, // en Mbps
    pub state: LinkState,
    pub last_hello: u64,
}

#[derive(Debug, Clone)]
pub struct Neighbor {
    pub router_id: Ipv4Addr,
    pub state: NeighborState,
    pub last_seen: u64,
    pub interface_addr: Ipv4Addr,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NeighborState {
    Down,
    Init,
    TwoWay,
    ExStart,
    Exchange,
    Loading,
    Full,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloPacket {
    pub router_id: Ipv4Addr,
    pub area_id: u32,
    pub hello_interval: u32,
    pub dead_interval: u32,
    pub neighbors: Vec<Ipv4Addr>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LSAHeader {
    pub lsa_type: u8,
    pub link_state_id: Ipv4Addr,
    pub advertising_router: Ipv4Addr,
    pub sequence_number: u32,
    pub age: u16,
    pub checksum: u16,
    pub length: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterLSA {
    pub header: LSAHeader,
    pub flags: u8,
    pub num_links: u16,
    pub links: Vec<RouterLSALink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouterLSALink {
    pub link_id: Ipv4Addr,
    pub link_data: Ipv4Addr,
    pub link_type: u8,
    pub num_metrics: u8,
    pub metric: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OSPFMessage {
    Hello(HelloPacket),
    LSA(RouterLSA),
    LSAck(Vec<LSAHeader>),
}

#[derive(Debug, Clone)]
pub struct RoutingEntry {
    pub destination: Ipv4Addr,
    pub next_hop: Ipv4Addr,
    pub cost: u32,
    pub interface: String,
}

pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
}