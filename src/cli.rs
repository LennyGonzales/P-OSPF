use std::net::{UdpSocket, ToSocketAddrs, SocketAddr};
use std::env;
use serde::Serialize;
use std::io::{self, Write, Read};

mod net_utils;
//use net_utils;
// Update the import path to match your actual module structure.
// For example, if read_config.rs is in the same directory:
mod read_config;
use read_config::read_router_config;
// Or, if it's in a submodule, adjust accordingly:
// use crate::some_submodule::read_config::read_router_config;

#[derive(Serialize)]
struct ControlMessage {
    message_type: u8,
    command: String,
}

fn help() {
    println!("Commandes disponibles:");
    println!("  enable   - Active le protocole OSPF");
    println!("  disable  - Désactive le protocole OSPF");
    println!("  routing-table  - Affiche la table de routage");
    println!("  neighbors - Affiche les voisins OSPF (adresse IP et nom système des routeurs voisins)");
    println!("  exit     - Quitte le CLI");
}

fn main() -> io::Result<()> {

    print!("Entrez l'adresse IP du serveur [127.0.0.1]: ");
    io::stdout().flush()?;
    let mut ip = String::new();
    io::stdin().read_line(&mut ip)?;
    let ip = ip.trim();
    let ip = if ip.is_empty() { "127.0.0.1" } else { ip };

    print!("Entrez le port du serveur [5000]: ");
    io::stdout().flush()?;
    let mut port = String::new();
    io::stdin().read_line(&mut port)?;
    let port: u16 = port.trim().parse().unwrap_or(5000);
    // Charger la configuration pour obtenir la clé
    let config = match read_router_config() {
        Ok(config) => config,
        Err(e) => {
            eprintln!("Erreur lors de la lecture de la configuration: {}", e);
            return Ok(());
        }
    };


    // Décoder la clé depuis la configuration
    let key = config.key
        .as_ref()
        .map(|k| base64::decode(k).unwrap_or_else(|_| k.as_bytes().to_vec()))
        .unwrap_or_else(|| vec![0x42; 32]); // Fallback vers une clé par défaut si pas de clé configurée

    let socket = UdpSocket::bind("0.0.0.0:0")?;
    let server_addr: SocketAddr = format!("{}:{}", ip, port).parse()
        .expect("Adresse invalide");
    println!("Connexion au serveur {}...", server_addr);

    let init_message = ControlMessage {
        message_type: 3,
        command: String::from("connexion"),
    };

    // Conversion en UdpSocket tokio pour utiliser send_message
    let socket = tokio::net::UdpSocket::from_std(socket)
        .expect("Échec de la conversion du socket");
    
    tokio::runtime::Runtime::new()?.block_on(async {
        net_utils::send_message(&socket, &server_addr, &init_message, &key, "[CLI]").await
            .expect("Échec de l'envoi du message");
        
        let mut buffer = [0; 1024];
        let (size, _) = socket.recv_from(&mut buffer).await?;
        let response = String::from_utf8_lossy(&buffer[..size]);
        println!("Réponse du serveur: {}", response);
        
        println!("\nBienvenue dans le CLI OSPF");
        help();
        
        loop {
            print!("\n> ");
            io::stdout().flush()?;
            
            let mut input = String::new();
            io::stdin().read_line(&mut input)?;
            let command = input.trim();
            
            if command == "exit" {
                println!("Au revoir!");
                break;
            } else if command == "help" {
                help();
                continue;
            }
            
            let message = ControlMessage {
                message_type: 3,
                command: String::from(command),
            };
            
            if let Err(e) = crate::net_utils::send_message(&socket, &server_addr, &message, &key, "[CLI]").await {
                println!("Erreur d'envoi: {}", e);
                continue;
            }
            
            // Réception de la réponse
            let mut buffer = [0; 4096];
            match socket.recv_from(&mut buffer).await {
                Ok((size, _)) => {
                    // Déchiffrer la réponse
                    match crate::net_utils::decrypt(&buffer[..size], &key) {
                        Ok(decrypted) => {
                            let response = String::from_utf8_lossy(&decrypted);
                            println!("Réponse:");
                            println!("{}", response);
                        },
                        Err(e) => println!("Erreur de déchiffrement: {}", e)
                    }
                },
                Err(e) => println!("Erreur lors de la réception: {}", e)
            }
        }
        Ok(())
    })
}