use crate::types::*;
use crate::ospf::OSPFProtocol;
use tokio::net::TcpStream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use std::sync::Arc;
use tokio::sync::RwLock;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::Duration;

pub struct RouterManager {
    config: RouterConfig,
    ospf: Arc<RwLock<OSPFProtocol>>,
}

impl RouterManager {
    pub async fn new(config: RouterConfig) -> Self {
        let ospf = Arc::new(RwLock::new(OSPFProtocol::new(config.clone())));
        
        Self {
            config,
            ospf,
        }
    }
    
    pub async fn handle_connection(
        &self,
        mut socket: TcpStream,
        addr: SocketAddr,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let mut buf = [0; 4096];
        
        loop {
            let n = socket.read(&mut buf).await?;
            if n == 0 {
                break;
            }
            
            // Tentative de désérialisation du message OSPF
            println!("handle_connection - Lecture de données depuis {}", addr);
            println!("handle_connection - Données lues: {:?}", &buf[..n]);
            if let Ok(message) = serde_json::from_slice::<OSPFMessage>(&buf[..n]) {
                self.process_ospf_message(message, addr.ip()).await?;
            } else {
                // Format legacy pour compatibilité
                self.handle_legacy_message(&buf[..n], &mut socket).await?;
            }
        }
        
        Ok(())
    }
    
    async fn process_ospf_message(
        &self,
        message: OSPFMessage,
        sender_ip: std::net::IpAddr,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match message {
            OSPFMessage::Hello(hello) => {
                let mut ospf = self.ospf.write().await;
                ospf.process_hello(hello, sender_ip).await;
            }
            OSPFMessage::LSA(lsa) => {
                let mut ospf = self.ospf.write().await;
                ospf.process_lsa(lsa).await;
            }
            OSPFMessage::LSAck(headers) => {
                let ospf = self.ospf.read().await;
                ospf.process_lsa_ack(headers).await;
            }
        }
        Ok(())
    }
    
    async fn handle_legacy_message(
        &self,
        data: &[u8],
        socket: &mut TcpStream,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let message = std::str::from_utf8(data)?;
        let parts: Vec<&str> = message.trim().split_whitespace().collect();
        
        if parts.len() >= 4 {
            let neighbor_ip: Ipv4Addr = parts[0].parse()?;
            let cost: u32 = parts[1].parse()?;
            let capacity: u64 = parts[2].parse()?;
            let state = match parts[3] {
                "Up" | "Active" => LinkState::Up,
                "Down" | "Inactive" => LinkState::Down,
                _ => LinkState::Down,
            };
            
            // Ajouter le lien
            let link = Link {
                neighbor_id: neighbor_ip,
                interface_addr: neighbor_ip, // Simplifié
                cost,
                capacity,
                state,
                last_hello: current_timestamp(),
            };
            
            let mut ospf = self.ospf.write().await;
            ospf.add_link(neighbor_ip, link).await;
            
            // Recalculer les routes
            ospf.calculate_routing_table().await;
            
            // Envoyer la table de routage
            let routing_table = ospf.get_routing_table().await;
            let response = self.format_routing_table(&routing_table);
            socket.write_all(response.as_bytes()).await?;
        }
        
        Ok(())
    }
    
    fn format_routing_table(&self, table: &[RoutingEntry]) -> String {
        let mut response = String::from("=== Table de Routage ===\n");
        response.push_str("Destination     | Next Hop       | Cost | Interface\n");
        response.push_str("----------------|----------------|------|----------\n");
        
        for entry in table {
            response.push_str(&format!(
                "{:<15} | {:<14} | {:<4} | {}\n",
                entry.destination,
                entry.next_hop,
                entry.cost,
                entry.interface
            ));
        }
        
        response
    }
    
    pub async fn start_hello_protocol(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(self.config.hello_interval as u64));
        
        loop {
            interval.tick().await;
            
            let ospf = self.ospf.read().await;
            println!("start_hello_protocol - Envoi paquet Hello");
            if let Err(e) = ospf.send_hello_packets().await {
                log::error!("Erreur lors de l'envoi des paquets Hello: {}", e);
            }
        }
    }
    
    pub async fn start_lsa_protocol(&self) {
        let mut interval = tokio::time::interval(Duration::from_secs(self.config.lsa_refresh_interval as u64));
        
        loop {
            interval.tick().await;
            
            let mut ospf = self.ospf.write().await;
            if let Err(e) = ospf.generate_and_flood_lsa().await {
                log::error!("Erreur lors de la génération/diffusion LSA: {}", e);
            }
        }
    }
}
