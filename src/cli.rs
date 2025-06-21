use std::net::{UdpSocket, ToSocketAddrs, SocketAddr};
use std::env;
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

    let socket = UdpSocket::bind("0.0.0.0:0")?;
    let server_addr = format!("{}:{}", ip, port);
    println!("Connexion au serveur {}...", server_addr);

    let init_message = ControlMessage {
        message_type: 3,
        command: String::from("connexion"),
    };
    
    let serialized = serde_json::to_string(&init_message)
        .expect("Échec de la sérialisation du message");
    
    socket.send_to(serialized.as_bytes(), &server_addr)?;
    
    let mut buffer = [0; 1024];
    let (size, _) = socket.recv_from(&mut buffer)?;
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
        
        let serialized = serde_json::to_string(&message)
            .expect("Échec de la sérialisation du message");
        
        socket.send_to(serialized.as_bytes(), &server_addr)?;
        
        // Réception de la réponse
        let mut buffer = [0; 4096];
        match socket.recv_from(&mut buffer) {
            Ok((size, _)) => {
                let response = String::from_utf8_lossy(&buffer[..size]);
                println!("Réponse:");
                println!("{}", response);
            },
            Err(e) => {
                println!("Erreur lors de la réception de la réponse: {}", e);
            }
        }
    }
    
    Ok(())
}