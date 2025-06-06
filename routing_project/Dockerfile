# Utiliser la dernière version stable de Rust
FROM rust:latest as builder

# Mettre à jour Rust et Cargo à la dernière version
RUN rustup update

# Définir le répertoire de travail
WORKDIR /usr/src/myapp

# Copier les fichiers de configuration Cargo
COPY Cargo.toml Cargo.lock ./

# Copier tous les fichiers source du projet dans le container
COPY src ./src

# Télécharger les dépendances et construire le projet
RUN cargo build --release

# Créer une nouvelle image avec une version plus légère (debian) pour l'exécution
# Utiliser une version plus récente de Debian
FROM debian:bookworm-slim

# Installer les dépendances nécessaires, y compris libc
RUN apt-get update && apt-get install -y libc6

# Copier le binaire compilé de l'étape de construction précédente
COPY --from=builder /usr/src/myapp/target/release/routing_project /usr/local/bin/routing_project

# Exposer le port nécessaire
EXPOSE 8080

# Commande par défaut
CMD ["routing_project"]

