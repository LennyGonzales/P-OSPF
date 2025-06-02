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
#include "ospf_common.h"

#define RESPONSE_TIMEOUT 3

typedef struct {
    char hostname[256];
    struct sockaddr_in addr;
    int bandwidth;
    int status;
} Neighbor;

int create_broadcast_socket() {
    int sockfd;
    if ((sockfd = socket(AF_INET, SOCK_DGRAM, 0)) < 0) {
        perror("socket");
        exit(EXIT_FAILURE);
    }

    int broadcastEnable = 1;
    if (setsockopt(sockfd, SOL_SOCKET, SO_BROADCAST, &broadcastEnable, sizeof(broadcastEnable)) < 0) {
        perror("setsockopt (SO_BROADCAST)");
        exit(EXIT_FAILURE);
    }

    struct sockaddr_in local_addr;
    memset(&local_addr, 0, sizeof(local_addr));
    local_addr.sin_family = AF_INET;
    local_addr.sin_addr.s_addr = INADDR_ANY;
    local_addr.sin_port = 0;

    if (bind(sockfd, (struct sockaddr *)&local_addr, sizeof(local_addr)) < 0) {
        perror("bind");
        exit(EXIT_FAILURE);
    }

    return sockfd;
}

void get_network_info(int *bandwidth, int *status) {
    *bandwidth = 1000;
    *status = 1;
}

void send_hello(int sockfd, int bandwidth, int status) {
    char hostname[256];
    if (gethostname(hostname, sizeof(hostname)) != 0) {
        perror("gethostname");
        strcpy(hostname, "unknown");
    }
    
    char hello_msg[1024];
    snprintf(hello_msg, sizeof(hello_msg), "OSPF_HELLO %s %d %d", hostname, bandwidth, status);

    struct sockaddr_in broadcast_addr;
    memset(&broadcast_addr, 0, sizeof(broadcast_addr));
    broadcast_addr.sin_family = AF_INET;
    broadcast_addr.sin_port = htons(NEIGHBOR_PORT);
    broadcast_addr.sin_addr.s_addr = inet_addr("255.255.255.255");

    sendto(sockfd, hello_msg, strlen(hello_msg), 0, (struct sockaddr *)&broadcast_addr, sizeof(broadcast_addr));
}

int collect_neighbor_responses(int sockfd, Neighbor *neighbors) {
    fd_set read_fds;
    struct timeval timeout;
    timeout.tv_sec = RESPONSE_TIMEOUT;
    timeout.tv_usec = 0;

    int neighbor_count = 0;

    while (1) {
        FD_ZERO(&read_fds);
        FD_SET(sockfd, &read_fds);
        int ret = select(sockfd + 1, &read_fds, NULL, NULL, &timeout);
        if (ret < 0) {
            perror("select");
            break;
        } else if (ret == 0) {
            break;
        }

        char buffer[1024];
        struct sockaddr_in sender_addr;
        socklen_t sender_len = sizeof(sender_addr);
        int n = recvfrom(sockfd, buffer, sizeof(buffer) - 1, 0, (struct sockaddr *)&sender_addr, &sender_len);
        if (n < 0) {
            perror("recvfrom");
            break;
        }
        buffer[n] = '\0';

        if (strncmp(buffer, "OSPF_HELLO", 10) == 0) {
            char remote_hostname[256];
            int received_bandwidth, received_status;
            if (sscanf(buffer, "OSPF_HELLO %255s %d %d", remote_hostname, &received_bandwidth, &received_status) != 3) {
                continue;
            }
            
            neighbors[neighbor_count].addr = sender_addr;
            strncpy(neighbors[neighbor_count].hostname, remote_hostname, sizeof(neighbors[neighbor_count].hostname) - 1);
            neighbors[neighbor_count].hostname[sizeof(neighbors[neighbor_count].hostname) - 1] = '\0';
            neighbors[neighbor_count].bandwidth = received_bandwidth;
            neighbors[neighbor_count].status = received_status;
            neighbor_count++;
        }
    }

    return neighbor_count;
}

void print_neighbors(Neighbor *neighbors, int neighbor_count) {
    printf("Neighboring machines:\n");
    for (int i = 0; i < neighbor_count; i++) {
        printf("  Hostname: %s, Bandwidth: %d, Status: %d\n", neighbors[i].hostname, neighbors[i].bandwidth, neighbors[i].status);
    }
}

int main() {
    int sockfd = create_broadcast_socket();
    int bandwidth, status;

    get_network_info(&bandwidth, &status);

    send_hello(sockfd, bandwidth, status);

    Neighbor neighbors[MAX_NEIGHBORS];
    int neighbor_count = collect_neighbor_responses(sockfd, neighbors);

    print_neighbors(neighbors, neighbor_count);

    close(sockfd);
    return 0;
}