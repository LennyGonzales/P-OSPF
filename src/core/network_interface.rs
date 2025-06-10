// This file manages the network interfaces, specifying which interfaces should be included in the routing calculations.

use std::net::{IpAddr, SocketAddr};
use crate::error::ProtocolError;
use std::collections::HashMap;

#[derive(Debug)]
pub struct NetworkInterface {
    pub name: String,
    pub ip_address: IpAddr,
    pub is_active: bool,
    pub metric: u32,
}

impl NetworkInterface {
    pub fn new(name: String, ip_address: IpAddr, metric: u32) -> Self {
        Self {
            name,
            ip_address,
            is_active: true,
            metric,
        }
    }
    
    pub fn get_broadcast_address(&self, port: u16) -> Result<SocketAddr, ProtocolError> {
        match self.ip_address {
            IpAddr::V4(ipv4) => {
                let broadcast_ip = std::net::Ipv4Addr::new(255, 255, 255, 255);
                Ok(SocketAddr::new(IpAddr::V4(broadcast_ip), port))
            }
            IpAddr::V6(_) => {
                Err(ProtocolError::NetworkInterface(
                    "IPv6 broadcast not supported".to_string()
                ))
            }
        }
    }
    
    pub fn is_same_network(&self, other_ip: &IpAddr) -> bool {
        // Simplified network comparison - in real implementation,
        // you would use subnet masks
        match (self.ip_address, other_ip) {
            (IpAddr::V4(self_ip), IpAddr::V4(other_ip)) => {
                let self_octets = self_ip.octets();
                let other_octets = other_ip.octets();
                
                // Assuming /24 network
                self_octets[0] == other_octets[0] &&
                self_octets[1] == other_octets[1] &&
                self_octets[2] == other_octets[2]
            }
            _ => false,
        }
    }
}

pub struct NetworkInterfaceManager {
    interfaces: HashMap<String, NetworkInterface>,
}

impl NetworkInterfaceManager {
    pub fn new() -> Self {
        NetworkInterfaceManager {
            interfaces: HashMap::new(),
        }
    }

    pub fn add_interface(&mut self, name: String, ip_address: IpAddr, metric: u32) {
        let interface = NetworkInterface {
            name: name.clone(),
            ip_address,
            is_active: true,
            metric,
        };
        self.interfaces.insert(name, interface);
    }

    pub fn remove_interface(&mut self, name: &str) {
        self.interfaces.remove(name);
    }

    pub fn set_interface_status(&mut self, name: &str, status: bool) {
        if let Some(interface) = self.interfaces.get_mut(name) {
            interface.is_active = status;
        }
    }

    pub fn get_active_interfaces(&self) -> Vec<&NetworkInterface> {
        self.interfaces.values().filter(|&iface| iface.is_active).collect()
    }

    pub fn get_interface(&self, name: &str) -> Option<&NetworkInterface> {
        self.interfaces.get(name)
    }
}