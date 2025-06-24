use tokio::net::UdpSocket;
use std::net::SocketAddr;
use std::env;
use routing_project::read_config;
use routing_project::net_utils;
use serde::Serialize;
use std::io::{self, Write, Read};

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

#[tokio::main]
async fn main() -> io::Result<()> {
    let config = read_config::read_router_config().map_err(|e| {
        io::Error::new(io::ErrorKind::Other, format!("Erreur de configuration: {}", e))
    })?;
    let key = config.key
        .as_ref()
        .map(|k| base64::decode(k).unwrap_or_else(|_| k.as_bytes().to_vec()))
        .unwrap_or_else(|| vec![0u8; 32]);
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

    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    let server_addr: SocketAddr = format!("{}:{}", ip, port).parse().expect("Adresse serveur invalide");
    println!("Connexion au serveur {}...", server_addr);

    let init_message = ControlMessage {
        message_type: 3,
        command: String::from("connexion"),
    };
    
    net_utils::send_message(&socket, &server_addr, &init_message, &key, "[CLI]").await.map_err(|e| {
        io::Error::new(io::ErrorKind::Other, format!("Erreur d'envoi: {}", e))
    })?;
    
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
        
        // Envoi de la commande
        let message = ControlMessage {
            message_type: 3,
            command: String::from(command),
        };
        
        net_utils::send_message(&socket, &server_addr, &message, &key, "[CLI]").await.map_err(|e| {
            io::Error::new(io::ErrorKind::Other, format!("Erreur d'envoi: {}", e))
        })?;
        
        // Réception de la réponse
        let mut buffer = [0; 4096];
        match socket.recv_from(&mut buffer).await {
            Ok((size, _)) => {
                let ciphertext = &buffer[..size];
                match net_utils::decrypt(ciphertext, &key) {
                    Ok(decrypted) => {
                        match String::from_utf8(decrypted) {
                            Ok(text) => {
                                println!("Réponse:");
                                println!("{}", text);
                            },
                            Err(e) => println!("Erreur de décodage UTF-8: {}", e)
                        }
                    },
                    Err(e) => println!("Erreur de déchiffrement: {}", e)
                }
            },
            Err(e) => {
                println!("Erreur lors de la réception de la réponse: {}", e);
            }
        }
    }
    
    Ok(())
}