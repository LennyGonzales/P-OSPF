# P-OSPF

## Présentation

P-OSPF est une implémentation simplifiée du protocole de routage OSPF (Open Shortest Path First) en Rust. Ce projet permet à des routeurs de découvrir dynamiquement la topologie du réseau, d’échanger des informations d’état de liens (LSA), de calculer les plus courts chemins (Dijkstra), et de mettre à jour leurs tables de routage. Il inclut une interface CLI et des mécanismes de sécurité (chiffrement AES).

## Fonctionnalités principales
- Découverte automatique de la topologie réseau
- Calcul des routes optimales (algorithme de Dijkstra)
- Gestion dynamique de la table de routage
- Interface CLI pour l’administration et la supervision
- Chiffrement des échanges (AES/CBC)
- Gestion de l’état des interfaces (actif/inactif, capacité)
- Déploiement multi-routeurs via Docker Compose

## Structure du projet
- `src/` : code source principal
  - `main.rs` : point d’entrée du routeur
  - `cli.rs` : interface en ligne de commande
  - `dijkstra.rs` : calcul des plus courts chemins
  - `lsa.rs`, `hello.rs`, `neighbor.rs` : gestion des paquets OSPF
  - `read_config.rs` : lecture des fichiers de configuration TOML
  - `net_utils.rs` : utilitaires réseau
  - `packet_loop.rs` : boucle principale de traitement des paquets
  - `types.rs`, `error.rs` : types et gestion d’erreurs
- `src/conf/` : exemples de fichiers de configuration TOML pour chaque routeur
- `compose.yaml` : déploiement multi-conteneurs Docker
- `rapport.md` : documentation technique détaillée
- `rapport_performance.md` : analyse des performances

## Lancement rapide

### Prérequis
- Rust (edition 2021)
- Docker & Docker Compose

### Exécution
```sh
cargo run --bin routing
```
```sh
cargo run --bin cli
```
### Déploiement multi-routeurs
```sh
docker compose up --build
```

## Configuration
Chaque routeur lit un fichier TOML dans `src/conf/` décrivant ses interfaces, capacités, et voisins attendus. Exemple :
```toml
[[interfaces]]
name = "eth0"
capacity_mbps = 1000
link_active = true
```

## Auteurs
- Lenny Gonzales <lenny.gonzales@etu.mines-ales.fr>
- Nils Saadi <nils.saadi@etu.mines-ales.fr>

Voir `rapport.md` pour plus de détails techniques.