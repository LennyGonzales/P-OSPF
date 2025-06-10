// This file manages the network interfaces, specifying which interfaces should be included in the routing calculations.

use std::net::IpAddr;
use std::collections::HashMap;

#[derive(Debug)]
pub struct NetworkInterface {
    pub name: String,
    pub ip_address: IpAddr,
    pub is_active: bool,
    pub capacity: u32, // in Mbps
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

    pub fn add_interface(&mut self, name: String, ip_address: IpAddr, capacity: u32) {
        let interface = NetworkInterface {
            name: name.clone(),
            ip_address,
            is_active: true,
            capacity,
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