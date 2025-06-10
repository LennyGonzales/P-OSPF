# Utilise une image de base avec gcc
FROM gcc:latest

# Installer les dépendances
RUN apt-get update && apt-get install -y \
    build-essential \
    libpcap-dev \
    tcpdump \
    && rm -rf /var/lib/apt/lists/*

# Copier le code dans le conteneur
COPY . /app

# Définir le répertoire de travail
WORKDIR /app

# Compiler le programme (remplace par `make` si tu as un Makefile)
RUN gcc -o ospf_router ospf_router.c

# Expose un port si nécessaire (facultatif pour OSPF qui utilise IP direct)
# EXPOSE 5000/udp

# Lancer ton programme (ou sleep si tu veux garder le conteneur ouvert)
CMD ["./ospf_router"]
