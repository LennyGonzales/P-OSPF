# Rapport Global de Performance — P-OSPF

Lenny Gonzales - INFRES 17 DL

Nils Saadi - INFRES 17 DL

## 1. Couverture des logs et niveaux

Le projet utilise la crate `log` avec `env_logger` pour la gestion des logs. Les niveaux utilisés sont :
- **info** : événements majeurs (démarrage, envoi/réception de messages, chargement de configuration)
- **debug** : détails sur la topologie, paquets reçus, calculs intermédiaires
- **warn** : situations anormales mais non bloquantes
- **error** : erreurs critiques (échec d'envoi, déchiffrement, etc.)

Les logs sont présents dans tous les modules critiques : découverte de voisins, gestion des paquets, calcul de routes, lecture de configuration, etc.

### 1.1. Type de log

- **[FORWARD]** : Relais d’un paquet LSA à d’autres routeurs. Permet de suivre la propagation des informations de topologie dans le réseau.
- **[SEND]** : Envoi d’un message (HELLO, LSA, ou commande) vers un voisin ou un autre routeur. Utile pour tracer l’activité sortante du routeur.
- **[RECV]** : Réception d’un message (HELLO, LSA, etc.) depuis un voisin. Permet de suivre l’activité entrante et la découverte de nouveaux événements réseau.
- **[CLI]** : Interactions via l’interface en ligne de commande (commandes utilisateur, affichage d’état, etc.). Permet de distinguer les actions manuelles ou de debug.


## 2. Performence

### 2.1. Temps de découverte du réseau

**Paramètres utilisés dans le code :**
- `HELLO_INTERVAL_SEC = 5s` (intervalle d’envoi des paquets HELLO)
- `LSA_INTERVAL_SEC = 10s` (intervalle d’envoi des paquets LSA)
- Timeout de détection de perte de voisin : 8s (soit 4 × HELLO_INTERVAL_SEC)

**Formule de convergence :**
`T_convergence ≈ 2 × HELLO_INTERVAL_SEC + LSA_INTERVAL_SEC + temps de propagation`

**Exemple concret (distance max = 3 sauts entre R1 et R3) :**
- 3 × 5s (HELLO) + 10s (LSA) + ~1s (propagation réseau) ≈ **26 secondes**

En pratique, la découverte complète du réseau et la mise à jour de toutes les tables de routage prennent environ 25 à 27 secondes dans ce scénario (topologie où la distance maximale entre deux routeurs est 3).

La découverte du réseau dans P-OSPF repose sur l’échange périodique de paquets HELLO entre routeurs voisins. Lorsqu’un routeur démarre :
- Il envoie des paquets HELLO sur toutes ses interfaces actives.
- Dès qu’un voisin répond, il est ajouté à la liste des voisins actifs.
- Une fois la bidirectionnalité confirmée, les routeurs échangent des LSA (Link State Advertisements) pour diffuser leur vue locale de la topologie.
- Après réception et propagation des LSA, chaque routeur exécute l’algorithme de Dijkstra pour calculer les routes optimales et met à jour sa table de routage.

**Estimation du temps de convergence :**
- Le temps de découverte dépend de l’intervalle d’envoi des HELLO (`HELLO_INTERVAL_SEC`), de la rapidité de réponse des voisins et de la diffusion des LSA.
- En pratique, la convergence (tous les réseaux présents dans la table de routage) se fait en quelques secondes après le démarrage, typiquement : `T_convergence ≈ 2 × HELLO_INTERVAL_SEC + LSA_INTERVAL_SEC + temps de propagation`.
- Le code logue chaque étape clé (découverte de voisin, réception/forward de LSA, mise à jour de la table de routage), ce qui permet de mesurer précisément ce temps en analysant les timestamps des logs.

### 2.2. Tolérance au panne

P-OSPF intègre une tolérance aux pannes basée sur :
- **Détection de la perte de voisin :** chaque voisin est surveillé via un timeout. Si aucun HELLO n’est reçu dans un délai donné (par défaut : 8 secondes, soit 4 × HELLO_INTERVAL_SEC), le voisin est marqué comme DOWN.
- **Mise à jour automatique :** la perte d’un voisin ou d’un lien déclenche immédiatement la diffusion d’un nouveau LSA, qui propage l’information de panne à tout le réseau.
- **Recalcul dynamique des routes :** à chaque changement de topologie (ajout/perte de lien ou voisin), l’algorithme de Dijkstra est relancé pour recalculer les routes optimales, en évitant les liens ou routeurs défaillants.
- **Redondance :** si plusieurs chemins existent, le protocole sélectionne automatiquement le chemin alternatif le plus performant.

**Résumé :**
- Le protocole assure une adaptation rapide à la perte de liens ou de routeurs, avec une convergence automatique de la table de routage après chaque panne détectée.
- Les logs permettent de tracer la détection de panne, la diffusion des LSA et la re-convergence du routage.


## 3. Utilisation des ressources

Le protocole P-OSPF est conçu pour être léger et efficace :
- **CPU** : la charge processeur reste faible en fonctionnement normal, l’algorithme de Dijkstra n’étant relancé qu’en cas de changement de topologie.
- **Mémoire** : l’empreinte mémoire dépend du nombre de voisins et de la taille de la topologie, mais reste modérée (stockage des LSA, voisins, table de routage).
- **Réseau** : le trafic généré est principalement constitué de paquets HELLO et LSA, dont la taille est réduite. Le flooding des LSA est limité par la détection des doublons et la propagation sélective. Un message HELLO fait typiquement 40 à 80 octets (JSON + chiffrement), un message LSA entre 400 et 900 octets selon la topologie (nombre de voisins/routes). Ces tailles restent très faibles par rapport à la MTU Ethernet (1500 octets), donc aucun risque de fragmentation.

## 4. Robustesse et fiabilité

- **Détection rapide des pannes** grâce au timeout sur les HELLO.
- **Reconfiguration automatique** de la table de routage après chaque changement.
- **Redondance** : si plusieurs chemins existent, le protocole choisit automatiquement le meilleur.
- **Logs détaillés** pour le diagnostic et l’audit.

## 5. Limites et axes d’amélioration

- **Scalabilité** : adapté aux topologies de taille petite à moyenne. Pour de très grands réseaux, des optimisations (LSA agrégés, hiérarchisation) seraient nécessaires.
- **Sécurité** : le chiffrement des paquets est supporté, mais une gestion avancée des clés et une authentification forte pourraient être ajoutées.
- **Métriques avancées** : pour une analyse fine, il serait possible d’ajouter des compteurs de paquets, des mesures de latence et des exports de logs structurés.
