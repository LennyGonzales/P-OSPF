mod routing;
mod types;
mod ospf;

use tokio::net::TcpListener;
use std::sync::Arc;
use routing::RouterManager;
use types::RouterConfig;
use std::net::Ipv4Addr;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::init();
    
    // Configuration du routeur
    let config = RouterConfig {
        router_id: "192.168.1.1".parse::<Ipv4Addr>().unwrap(),
        name: "Router1".to_string(),
        hello_interval: 10, // secondes
        dead_interval: 40,  // secondes
        lsa_refresh_interval: 1800, // secondes
    };
    
    // Initialisation du gestionnaire de routeur
    let router_manager = Arc::new(RouterManager::new(config).await);
    
    // Démarrage des tâches périodiques
    let rm_hello = Arc::clone(&router_manager);
    tokio::spawn(async move {
        rm_hello.start_hello_protocol().await;
    });
    
    let rm_lsa = Arc::clone(&router_manager);
    tokio::spawn(async move {
        rm_lsa.start_lsa_protocol().await;
    });
    
    // Création du listener TCP
    let listener = TcpListener::bind("127.0.0.1:8080").await?;
    println!("Routeur OSPF écoute sur 127.0.0.1:8080");
    
    loop {
        let (socket, addr) = listener.accept().await?;
        let router_manager = Arc::clone(&router_manager);
        
        tokio::spawn(async move {
            if let Err(e) = router_manager.handle_connection(socket, addr).await {
                log::error!("Erreur lors du traitement de la connexion: {}", e);
            }
        });
    }
}
