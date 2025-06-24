pub fn init_logging_and_env() {
    if std::env::var("RUST_LOG").is_err() {
        std::env::set_var("RUST_LOG", "info");
    }
    env_logger::init();
}

pub async fn init_socket(port: u16) -> crate::error::Result<std::sync::Arc<tokio::net::UdpSocket>> {
    let socket = std::sync::Arc::new(tokio::net::UdpSocket::bind(format!("0.0.0.0:{}", port)).await?);
    socket.set_broadcast(true)?;
    Ok(socket)
}

pub fn init_state(router_ip: String, config: crate::read_config::RouterConfig) -> std::sync::Arc<crate::AppState> {
    std::sync::Arc::new(crate::AppState {
        topology: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        neighbors: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        routing_table: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        processed_lsa: tokio::sync::Mutex::new(std::collections::HashSet::new()),
        lsa_database: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        local_ip: router_ip,
        enabled: tokio::sync::Mutex::new(true), // OSPF activé par défaut
        config,
    })
}