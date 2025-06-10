# Utiliser une image Alpine déjà configurée avec glibc
FROM frolvlad/alpine-glibc:alpine-3.12 as builder

# Installer curl, bash, gcc, et les dépendances nécessaires pour la compilation Rust
RUN apk update && \
    apk add --no-cache curl bash gcc libc-dev make

# Installer rustup et mettre à jour Rust
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"
RUN rustup update

# Définir le répertoire de travail
WORKDIR /usr/src/myapp

# Copier les fichiers de configuration Cargo
COPY Cargo.toml Cargo.lock ./

# Copier tous les fichiers source du projet dans le container
COPY src ./src

# Télécharger les dépendances et construire le projet
RUN cargo build --release

# Créer une nouvelle image avec une version plus légère (Alpine)
FROM frolvlad/alpine-glibc:alpine-3.12

# Copier le binaire compilé de l'étape de construction précédente
COPY --from=builder /usr/src/myapp/target/release/routing_project /usr/local/bin/routing_project

# Exposer le port nécessaire
EXPOSE 8080

# Commande par défaut
CMD ["routing_project"]
