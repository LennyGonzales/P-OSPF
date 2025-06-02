
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <arpa/inet.h>
#include <sys/socket.h>
#include <sys/types.h>
#include <netinet/in.h>
#include "neighborshow.h"

/* A simple fixed‐size cache for recently seen request IDs (to avoid re‐broadcast loops) */
#define MAX_REQUESTS 100

typedef struct {
    int id;
} request_entry;

request_entry seen_requests[MAX_REQUESTS];
int seen_count = 0;

/* Check if a request id has already been processed */
int already_seen(int id) {
    for (int i = 0; i < seen_count; i++) {
        if (seen_requests[i].id == id)
            return 1;
    }
    return 0;
}

/* Add a request id to the cache */
void add_request(int id) {
    if (seen_count < MAX_REQUESTS) {
        seen_requests[seen_count].id = id;
        seen_count++;
    }
}

int main() {
    int sockfd;
    struct sockaddr_in addr;
    char buffer[MAX_BUFFER];
    socklen_t addr_len;

    /* Create a UDP socket */
    if ((sockfd = socket(AF_INET, SOCK_DGRAM, 0)) < 0) {
        perror("socket");
        exit(EXIT_FAILURE);
    }

    /* Bind the socket to all interfaces on NEIGHBOR_PORT */
    memset(&addr, 0, sizeof(addr));
    addr.sin_family = AF_INET;
    addr.sin_port = htons(NEIGHBOR_PORT);
    addr.sin_addr.s_addr = INADDR_ANY;
    if (bind(sockfd, (struct sockaddr*)&addr, sizeof(addr)) < 0) {
        perror("bind");
        exit(EXIT_FAILURE);
    }

    printf("neighborshow_agent listening on UDP port %d...\n", NEIGHBOR_PORT);

    while (1) {
        struct sockaddr_in sender_addr;
        addr_len = sizeof(sender_addr);
        int n = recvfrom(sockfd, buffer, MAX_BUFFER - 1, 0,
                         (struct sockaddr *)&sender_addr, &addr_len);
        if (n < 0) {
            perror("recvfrom");
            continue;
        }
        buffer[n] = '\0';

        /* Expected message format:
         *   "NEIGHBOR_REQUEST <id>"
         */
        char prefix[32];
        int req_id;
        if (sscanf(buffer, "%31s %d", prefix, &req_id) != 3) {
            /* Invalid message format; ignore */
            continue;
        }
        if (strcmp(prefix, REQUEST_PREFIX) != 0) {
            /* Not a neighbor request; ignore */
            continue;
        }

        /* If we already processed this request, ignore it */
        if (already_seen(req_id))
            continue;
        add_request(req_id);

        /* Get the local hostname */
        char hostname[256];
        if (gethostname(hostname, sizeof(hostname)) != 0) {
            perror("gethostname");
            strcpy(hostname, "unknown");
        }

        /* Prepare response message:
         *   "NEIGHBOR_RESPONSE <id> <hostname>"
         */
        char response[MAX_BUFFER];
        snprintf(response, sizeof(response), "%s %d %s", RESPONSE_PREFIX, req_id, hostname);

        /* Send the response directly to the sender */
        if (sendto(sockfd, response, strlen(response), 0,
                   (struct sockaddr*)&sender_addr, addr_len) < 0) {
            perror("sendto");
        }
    }

    close(sockfd);
    return 0;
}