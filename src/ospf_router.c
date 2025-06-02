#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
#include <string.h>
#include <ifaddrs.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <sys/socket.h>
#include <net/if.h>
#include <errno.h>
#include <time.h>

#define MULTICAST_ADDR "224.0.0.5"
#define PORT 5000
#define MAX_BUF 1024

// Types de messages
#define MSG_HELLO 1
#define MSG_LSA   2

typedef struct {
    int type;
    char router_id[32]; // identifiant du routeur (ex: IP)
} OSPFHello;

typedef struct {
    int type;
    char router_id[32];
    char advertised_network[32]; // exemple: "10.0.0.0/24"
} OSPFLSA;

char *get_local_ip() {
    static char ip[INET_ADDRSTRLEN] = "";
    struct ifaddrs *ifaddr, *ifa;
    void *tmp;

    if (getifaddrs(&ifaddr) == -1) {
        perror("getifaddrs");
        return NULL;
    }

    for (ifa = ifaddr; ifa != NULL; ifa = ifa->ifa_next) {
        if (!ifa->ifa_addr || ifa->ifa_addr->sa_family != AF_INET)
            continue;
        if (strcmp(ifa->ifa_name, "lo") == 0)
            continue;
        tmp = &((struct sockaddr_in *)ifa->ifa_addr)->sin_addr;
        inet_ntop(AF_INET, tmp, ip, INET_ADDRSTRLEN);
        break;
    }

    freeifaddrs(ifaddr);
    return ip;
}

void send_hello(int sock, struct sockaddr_in *addr, const char *router_id) {
    OSPFHello hello = {MSG_HELLO, ""};
    strncpy(hello.router_id, router_id, sizeof(hello.router_id));
    sendto(sock, &hello, sizeof(hello), 0, (struct sockaddr *)addr, sizeof(*addr));
    printf("[SENT] Hello from %s\n", router_id);
}

void send_lsa(int sock, struct sockaddr_in *addr, const char *router_id) {
    OSPFLSA lsa = {MSG_LSA, "", ""};
    strncpy(lsa.router_id, router_id, sizeof(lsa.router_id));
    strncpy(lsa.advertised_network, "10.0.0.0/24", sizeof(lsa.advertised_network));
    sendto(sock, &lsa, sizeof(lsa), 0, (struct sockaddr *)addr, sizeof(*addr));
    printf("[SENT] LSA from %s\n", router_id);
}

int main() {
    int sock;
    struct sockaddr_in local_addr, multicast_addr;
    struct ip_mreq mreq;
    char buf[MAX_BUF];
    ssize_t len;
    socklen_t addrlen;
    char *local_ip = get_local_ip();

    if (!local_ip) {
        fprintf(stderr, "Could not determine local IP.\n");
        exit(EXIT_FAILURE);
    }

    printf("Starting OSPF router with ID: %s\n", local_ip);

    // Create socket
    sock = socket(AF_INET, SOCK_DGRAM, 0);
    if (sock < 0) {
        perror("socket");
        exit(EXIT_FAILURE);
    }

    // Allow multiple sockets to use the same PORT
    int yes = 1;
    setsockopt(sock, SOL_SOCKET, SO_REUSEADDR, &yes, sizeof(yes));

    // Bind to port
    memset(&local_addr, 0, sizeof(local_addr));
    local_addr.sin_family = AF_INET;
    local_addr.sin_port = htons(PORT);
    local_addr.sin_addr.s_addr = htonl(INADDR_ANY);
    if (bind(sock, (struct sockaddr *)&local_addr, sizeof(local_addr)) < 0) {
        perror("bind");
        exit(EXIT_FAILURE);
    }

    // Join multicast group
    mreq.imr_multiaddr.s_addr = inet_addr(MULTICAST_ADDR);
    mreq.imr_interface.s_addr = inet_addr(local_ip);
    if (setsockopt(sock, IPPROTO_IP, IP_ADD_MEMBERSHIP, &mreq, sizeof(mreq)) < 0) {
        perror("setsockopt - multicast");
        exit(EXIT_FAILURE);
    }

    // Setup destination addr
    memset(&multicast_addr, 0, sizeof(multicast_addr));
    multicast_addr.sin_family = AF_INET;
    multicast_addr.sin_port = htons(PORT);
    multicast_addr.sin_addr.s_addr = inet_addr(MULTICAST_ADDR);

    // Main loop
    time_t last_hello = 0;
    while (1) {
        fd_set fds;
        FD_ZERO(&fds);
        FD_SET(sock, &fds);

        struct timeval timeout = {1, 0}; // check every second
        if (select(sock + 1, &fds, NULL, NULL, &timeout) > 0) {
            addrlen = sizeof(multicast_addr);
            len = recvfrom(sock, buf, MAX_BUF, 0, (struct sockaddr *)&multicast_addr, &addrlen);
            if (len > 0) {
                int type = *(int *)buf;
                if (type == MSG_HELLO) {
                    OSPFHello *hello = (OSPFHello *)buf;
                    printf("[RECV] Hello from %s\n", hello->router_id);
                    // répondre avec LSA
                    send_lsa(sock, &multicast_addr, local_ip);
                } else if (type == MSG_LSA) {
                    OSPFLSA *lsa = (OSPFLSA *)buf;
                    printf("[RECV] LSA from %s: network %s\n", lsa->router_id, lsa->advertised_network);
                }
            }
        }

        // Envoie périodique de Hello
        if (time(NULL) - last_hello >= 5) {
            send_hello(sock, &multicast_addr, local_ip);
            last_hello = time(NULL);
        }
    }

    close(sock);
    return 0;
}
