FROM frolvlad/alpine-glibc:alpine-3.12 as builder

RUN apk update && \
    apk add --no-cache curl bash gcc libc-dev make

RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
ENV PATH="/root/.cargo/bin:${PATH}"
RUN rustup update

WORKDIR /usr/src/myapp

COPY Cargo.toml Cargo.lock ./

COPY src ./src

RUN cargo build --release

FROM frolvlad/alpine-glibc:alpine-3.12

COPY --from=builder /usr/src/myapp/target/release/routing_project /usr/local/bin/routing_project

EXPOSE 8080

CMD ["routing_project"]
