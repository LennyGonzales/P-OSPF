#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <arpa/inet.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <netinet/in.h>
#include <net/if.h>
#include <sys/ioctl.h>
#include <linux/sockios.h>
#include <ifaddrs.h>
#include "neighborshow.h"

#define MAX_REQUESTS 100
#define MAX_NEIGHBORS 100
#define OSPF_HELLO 1
#define OSPF_LSA 2

typedef struct {
    int id;
} request_entry;

typedef struct {
    char hostname[256];
    struct sockaddr_in addr;
    int bandwidth;
    int status;
} Neighbor;

request_entry seen_requests[MAX_REQUESTS];
int seen_count = 0;
Neighbor neighbors[MAX_NEIGHBORS];
int neighbor_count = 0;

int already_seen(int id) {
    for (int i = 0; i < seen_count; i++) {
        if (seen_requests[i].id == id)
            return 1;
    }
    return 0;
}

void add_request(int id) {
    if (seen_count < MAX_REQUESTS) {
        seen_requests[seen_count].id = id;
        seen_count++;
    }
}

void add_neighbor(struct sockaddr_in addr, const char *hostname, int bandwidth, int status) {
    for (int i = 0; i < neighbor_count; i++) {
        if (neighbors[i].addr.sin_addr.s_addr == addr.sin_addr.s_addr) {
            return;
        }
    }
    if (neighbor_count < MAX_NEIGHBORS) {
        neighbors[neighbor_count].addr = addr;
        strncpy(neighbors[neighbor_count].hostname, hostname, sizeof(neighbors[neighbor_count].hostname) - 1);
        neighbors[neighbor_count].bandwidth = bandwidth;
        neighbors[neighbor_count].status = status;
        neighbor_count++;
    }
}

void send_hello(int sockfd, int bandwidth, int status) {
    char hello_msg[1024];
    snprintf(hello_msg, sizeof(hello_msg), "OSPF_HELLO %d %d", bandwidth, status);

    struct sockaddr_in broadcast_addr;
    memset(&broadcast_addr, 0, sizeof(broadcast_addr));
    broadcast_addr.sin_family = AF_INET;
    broadcast_addr.sin_port = htons(NEIGHBOR_PORT);
    broadcast_addr.sin_addr.s_addr = inet_addr("255.255.255.255");

    sendto(sockfd, hello_msg, strlen(hello_msg), 0, (struct sockaddr *)&broadcast_addr, sizeof(broadcast_addr));
}

void send_lsa(int sockfd) {
    char lsa_msg[1024];
    snprintf(lsa_msg, sizeof(lsa_msg), "OSPF_LSA");

    for (int i = 0; i < neighbor_count; i++) {
        sendto(sockfd, lsa_msg, strlen(lsa_msg), 0, (struct sockaddr *)&neighbors[i].addr, sizeof(neighbors[i].addr));
    }
}

int main() {
    int sockfd;
    struct sockaddr_in addr;
    char buffer[1024];
    socklen_t addr_len;

    if ((sockfd = socket(AF_INET, SOCK_DGRAM, 0)) < 0) {
        perror("socket");
        exit(EXIT_FAILURE);
    }

    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = htons(NEIGHBOR_PORT);
    addr.sin_addr.s_addr = INADDR_ANY;

    if (bind(sockfd, (struct sockaddr*)&addr, sizeof(addr)) < 0) {
        perror("bind");
        exit(EXIT_FAILURE);
    }

    printf("OSPF Agent listening on UDP port %d...\n", NEIGHBOR_PORT);

    while (1) {
        struct sockaddr_in sender_addr;
        addr_len = sizeof(sender_addr);
        int n = recvfrom(sockfd, buffer, sizeof(buffer) - 1, 0, (struct sockaddr *)&sender_addr, &addr_len);
        if (n < 0) {
            perror("recvfrom");
            continue;
        }
        buffer[n] = '\0';

        if (strncmp(buffer, "OSPF_HELLO", 10) == 0) {
            char hostname[256];
            int received_bandwidth, received_status;
            if (sscanf(buffer, "OSPF_HELLO %d %d", &received_bandwidth, &received_status) != 2) {
                continue;
            }
            if (gethostname(hostname, sizeof(hostname)) != 0) {
                perror("gethostname");
                strcpy(hostname, "unknown");
            }
            add_neighbor(sender_addr, hostname, received_bandwidth, received_status);
            send_lsa(sockfd);
        }
    }

    close(sockfd);
    return 0;
}
