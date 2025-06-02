FROM alpine:latest AS builder

RUN apk update && apk add --no-cache \
    build-base \
    musl-dev \
    gcc \
    make

WORKDIR /app

ENTRYPOINT [ "make" ]