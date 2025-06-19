use std::net::UdpSocket;
use std::env;
use serde::Serialize;

#[derive(Serialize)]
struct ControlMessage {
    message_type: u8, // 3 pour contrôle
    command: String,  // "disable" ou "enable"
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: {} <enable|disable> <ip:port>", args[0]);
        std::process::exit(1);
    }
    let command = &args[1];
    let addr = &args[2];
    if command != "enable" && command != "disable" {
        eprintln!("Commande inconnue: {} (utilisez 'enable' ou 'disable')", command);
        std::process::exit(1);
    }
    let msg = ControlMessage {
        message_type: 3,
        command: command.clone(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let socket = UdpSocket::bind("0.0.0.0:0").expect("bind");
    socket.send_to(json.as_bytes(), addr).expect("send");
    println!("Message '{}' envoyé à {}", command, addr);
}
