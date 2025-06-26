# Gestion de l'État des Liens dans P-OSPF

## Vue d'ensemble

Ce projet OSPF inclut maintenant la gestion de l'état des liens pour chaque interface, permettant de calculer les meilleurs chemins en tenant compte de :

- **Nombre de sauts** (métrique de base OSPF)
- **État des liens** (actifs ou inactifs)
- **Capacités nominales** (débits maximaux en Mbps)

## Configuration des Interfaces

### Format des fichiers de configuration

Chaque routeur a un fichier de configuration nommé `config_<hostname>.toml` dans le dossier `src/conf/`. 

Exemple de configuration :

```toml
[[interfaces]]
name = "eth0"
capacity_mbps = 1000
link_active = true

[[interfaces]]
name = "eth1"
capacity_mbps = 100
link_active = false
```

### Paramètres des interfaces

- **name** : Nom de l'interface (ex: "eth0", "eth1")
- **capacity_mbps** : Capacité en Mbps (utilisée pour calculer le coût OSPF)
- **link_active** : État du lien (true = actif, false = inactif)

## Calcul du Coût OSPF

Le coût OSPF est calculé selon la formule standard :

```
Coût = 100 000 000 / (capacité_en_bps)
```

### Exemples de coûts :

| Capacité | Coût OSPF |
|----------|-----------|
| 10 Mbps  | 10        |
| 100 Mbps | 1         |
| 500 Mbps | 1         |
| 1000 Mbps| 1         |
| 10 Gbps  | 1         |

*Note: Le coût minimum est 1*

## Fonctionnalités Implémentées

### 1. Configuration des Interfaces

- Lecture automatique de la configuration basée sur le hostname
- Prise en compte de l'état des liens (`link_active`) au démarrage
- Calcul des coûts OSPF basé sur les capacités configurées

### 2. Gestion des Voisins

- Prise en compte de l'état des interfaces pour déterminer la disponibilité des voisins
- Calcul dynamique de la capacité des liens depuis la configuration
- Mise à jour automatique basée sur les timeouts OSPF standard

## Utilisation

### 1. Configuration

1. Créer un fichier de configuration pour votre routeur :
   ```bash
   cp src/conf/config_R_1.toml src/conf/config_$(hostname).toml
   ```

2. Modifier la configuration selon vos interfaces :
   ```toml
   [[interfaces]]
   name = "eth0"
   capacity_mbps = 1000
   link_active = true
   ```

### 2. Lancement

```bash
cargo run
```

Le programme va :
- Charger automatiquement la configuration basée sur le hostname
- Surveiller l'état des interfaces
- Calculer les routes optimales
- Afficher les rapports d'état

### 3. Surveillance

Le programme affiche périodiquement :
- L'état des interfaces configurées
- Les coûts OSPF calculés
- Les voisins découverts et leur état
- Les statistics de routage

## Exemples de Configurations

### Routeur avec liens redondants

```toml
[[interfaces]]
name = "eth0"
capacity_mbps = 1000
link_active = true

[[interfaces]]
name = "eth1"
capacity_mbps = 1000
link_active = true
```

### Routeur avec lien principal et backup

```toml
[[interfaces]]
name = "eth0"
capacity_mbps = 1000
link_active = true

[[interfaces]]
name = "eth1"
capacity_mbps = 100
link_active = false  # Backup inactif
```

### Routeur avec liens de capacités différentes

```toml
[[interfaces]]
name = "eth0"
capacity_mbps = 1000
link_active = true

[[interfaces]]
name = "eth1"
capacity_mbps = 100
link_active = true

[[interfaces]]
name = "eth2"
capacity_mbps = 10
link_active = false
```

## Impact sur le Routage

### Liens Actifs
- Inclus dans le calcul des routes
- Coût basé sur la capacité
- Participent à la redondance

### Liens Inactifs
- Exclus du calcul des routes
- Coût infini (∞)
- Voisins marqués comme DOWN

### Sélection des Routes
- Préférence aux liens de plus forte capacité (coût plus faible)
- Évitement automatique des liens inactifs
- Recalcul automatique lors des changements d'état

## Surveillance et Debug

### Logs
Le programme génère des logs détaillés sur :
- L'état des interfaces
- Les changements de liens
- Les calculs de coûts
- Les mises à jour de routage

### Commandes de Debug
```bash
# Voir les logs en temps réel
RUST_LOG=info cargo run

# Debug complet
RUST_LOG=debug cargo run
```
