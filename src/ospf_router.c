#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <string.h>
#include <ifaddrs.h>
#include <arpa/inet.h>
#include <netinet/in.h>
#include <sys/socket.h>
#include <time.h>
#include <errno.h>
#include <netdb.h>

#define MULTICAST_ADDR "224.0.0.5"
#define PORT 5000
#define MAX_ROUTERS 32
#define MAX_NEIGHBORS 8

#define MSG_HELLO 1
#define MSG_LSA 2

typedef struct {
    int type;
    char router_id[32];
} HelloMessage;

typedef struct {
    int type;
    char router_id[32];
    int neighbor_count;
    struct {
        char neighbor_id[32];
        int link_up;
        int capacity; // en Mbps
    } neighbors[MAX_NEIGHBORS];
} LSAMessage;

typedef struct {
    char router_id[32];
    int link_up;
    int capacity;
} Neighbor;

typedef struct {
    char router_id[32];
    Neighbor neighbors[MAX_NEIGHBORS];
    int neighbor_count;
} Router;

Router topology[MAX_ROUTERS];
int topology_size = 0;

char *get_local_ip() {
    char hostname[1024];
    struct hostent *host_entry;
    char *IPbuffer;

    // Obtenir le nom de l'hôte
    if (gethostname(hostname, sizeof(hostname)) == -1) {
        perror("gethostname");
        return NULL;
    }

    // Résolution du nom en adresse IP
    host_entry = gethostbyname(hostname);
    if (host_entry == NULL) {
        herror("gethostbyname");
        return NULL;
    }

    // Convertir l'adresse IP au format lisible
    IPbuffer = inet_ntoa(*((struct in_addr*) host_entry->h_addr_list[0]));
    if (IPbuffer == NULL) {
        perror("inet_ntoa");
        return NULL;
    }

    printf("IP locale : %s\n", IPbuffer);
    return IPbuffer;
}

void update_topology(LSAMessage *lsa) {
    for (int i = 0; i < topology_size; i++) {
        if (strcmp(topology[i].router_id, lsa->router_id) == 0) {
            topology[i].neighbor_count = lsa->neighbor_count;
            for (int j = 0; j < lsa->neighbor_count; j++) {
                topology[i].neighbors[j].link_up = lsa->neighbors[j].link_up;
                topology[i].neighbors[j].capacity = lsa->neighbors[j].capacity;
                strncpy(topology[i].neighbors[j].router_id, lsa->neighbors[j].neighbor_id, 32);
            }
            return;
        }
    }
    // Nouveau routeur
    strncpy(topology[topology_size].router_id, lsa->router_id, 32);
    topology[topology_size].neighbor_count = lsa->neighbor_count;
    for (int j = 0; j < lsa->neighbor_count; j++) {
        strncpy(topology[topology_size].neighbors[j].router_id, lsa->neighbors[j].neighbor_id, 32);
        topology[topology_size].neighbors[j].link_up = lsa->neighbors[j].link_up;
        topology[topology_size].neighbors[j].capacity = lsa->neighbors[j].capacity;
    }
    topology_size++;
}

void compute_shortest_paths(const char *source_id) {
    typedef struct {
        char router_id[32];
        int cost;
        char prev[32];
        int visited;
    } Node;

    Node nodes[MAX_ROUTERS];
    for (int i = 0; i < topology_size; i++) {
        strncpy(nodes[i].router_id, topology[i].router_id, 32);
        nodes[i].cost = strcmp(topology[i].router_id, source_id) == 0 ? 0 : 1e9;
        nodes[i].prev[0] = '\0';
        nodes[i].visited = 0;
    }

    while (1) {
        int min_idx = -1;
        int min_cost = 1e9;
        for (int i = 0; i < topology_size; i++) {
            if (!nodes[i].visited && nodes[i].cost < min_cost) {
                min_cost = nodes[i].cost;
                min_idx = i;
            }
        }
        if (min_idx == -1) break;

        nodes[min_idx].visited = 1;
        Router *router = &topology[min_idx];
        for (int j = 0; j < router->neighbor_count; j++) {
            if (!router->neighbors[j].link_up) continue;
            // coût inverse à la capacité : + de débit = moins cher
            int weight = 1000 / router->neighbors[j].capacity;
            for (int k = 0; k < topology_size; k++) {
                if (strcmp(topology[k].router_id, router->neighbors[j].router_id) == 0) {
                    if (nodes[min_idx].cost + weight < nodes[k].cost) {
                        nodes[k].cost = nodes[min_idx].cost + weight;
                        strncpy(nodes[k].prev, router->router_id, 32);
                    }
                }
            }
        }
    }

    printf("\n=== Routing Table (%s) ===\n", source_id);
    for (int i = 0; i < topology_size; i++) {
        if (strcmp(nodes[i].router_id, source_id) == 0) continue;
        printf("To %s via %s (cost: %d)\n", nodes[i].router_id,
               nodes[i].prev[0] ? nodes[i].prev : "-", nodes[i].cost);
    }
}

void send_hello(int sock, struct sockaddr_in *addr, const char *router_id) {
    HelloMessage msg = {MSG_HELLO, ""};
    strncpy(msg.router_id, router_id, sizeof(msg.router_id));
    sendto(sock, &msg, sizeof(msg), 0, (struct sockaddr *)addr, sizeof(*addr));
    printf("[SEND] HELLO from %s to %s\n", msg.router_id, inet_ntoa(addr->sin_addr));
}

void send_lsa(int sock, struct sockaddr_in *addr, const char *router_id) {
    LSAMessage msg = {MSG_LSA, "", 1};
    strncpy(msg.router_id, router_id, sizeof(msg.router_id));
    strncpy(msg.neighbors[0].neighbor_id, "192.168.1.1", 32); // Simulé
    msg.neighbors[0].link_up = 1;
    msg.neighbors[0].capacity = 100; // Mbps
    sendto(sock, &msg, sizeof(msg), 0, (struct sockaddr *)addr, sizeof(*addr));
}

int main() {
    char *router_id = get_local_ip();
    if (!router_id) {
        fprintf(stderr, "IP locale introuvable\n");
        exit(1);
    }

    printf("Router ID: %s\n", router_id);

    int sock = socket(AF_INET, SOCK_DGRAM, 0);
    int yes = 1;
    setsockopt(sock, SOL_SOCKET, SO_REUSEADDR, &yes, sizeof(yes));

    struct sockaddr_in local = {0}, remote = {0};
    local.sin_family = AF_INET;
    local.sin_port = htons(PORT);
    local.sin_addr.s_addr = htonl(INADDR_ANY);
    bind(sock, (struct sockaddr *)&local, sizeof(local));

    struct ip_mreq mreq;
    mreq.imr_multiaddr.s_addr = inet_addr(MULTICAST_ADDR);
    mreq.imr_interface.s_addr = inet_addr(router_id);
    setsockopt(sock, IPPROTO_IP, IP_ADD_MEMBERSHIP, &mreq, sizeof(mreq));

    remote.sin_family = AF_INET;
    remote.sin_port = htons(PORT);
    remote.sin_addr.s_addr = inet_addr(MULTICAST_ADDR);

    time_t last_hello = 0, last_lsa = 0;
    char buf[2048];
    socklen_t len;

    while (1) {
        fd_set fds;
        FD_ZERO(&fds);
        FD_SET(sock, &fds);
        struct timeval tv = {1, 0};
        select(sock + 1, &fds, NULL, NULL, &tv);

        if (FD_ISSET(sock, &fds)) {
            len = sizeof(remote);
            ssize_t r = recvfrom(sock, buf, sizeof(buf), 0, (struct sockaddr *)&remote, &len);
            if (r > 0) {
                int type = *(int *)buf;
                if (type == MSG_HELLO) {
                    HelloMessage *h = (HelloMessage *)buf;
                    printf("[RECV] HELLO from %s\n", h->router_id);
                    send_lsa(sock, &remote, router_id);
                } else if (type == MSG_LSA) {
                    LSAMessage *lsa = (LSAMessage *)buf;
                    printf("[RECV] LSA from %s\n", lsa->router_id);
                    update_topology(lsa);
                    compute_shortest_paths(router_id);
                }
            }
        }

        if (time(NULL) - last_hello >= 5) {
            send_hello(sock, &remote, router_id);
            last_hello = time(NULL);
        }
    }

    close(sock);
    return 0;
}