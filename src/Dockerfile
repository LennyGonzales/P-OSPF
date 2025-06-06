# Utilise une image de base avec gcc et make
FROM gcc:latest

# Dépendances nécessaires (ex : libpcap-dev si nécessaire)
RUN apt-get update && apt-get install -y \
    build-essential \
    libpcap-dev \
    && rm -rf /var/lib/apt/lists/*

# Copie le code dans le conteneur
COPY . /app

# Définir le répertoire de travail
WORKDIR /app

# Compiler ton programme
RUN gcc -o ospf_router ospf_router.c

# Expose le port UDP pour la communication (par exemple, port 5000)
EXPOSE 5000/udp

# Commande pour démarrer le programme
# CMD ["./ospf_router"]
CMD ["sleep", "infinity"]