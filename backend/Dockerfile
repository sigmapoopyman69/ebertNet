# ebertNet Epoxy/Wisp backend container
# Builds the official Epoxy server from MercuryWorkshop/epoxy-tls.
FROM rustlang/rust:nightly-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends     build-essential pkg-config libssl-dev ca-certificates git     && rm -rf /var/lib/apt/lists/*
WORKDIR /src
RUN git clone --depth 1 --branch multiplexed https://github.com/MercuryWorkshop/epoxy-tls.git .
RUN cargo build --release --manifest-path server/Cargo.toml

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends     ca-certificates sed     && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=build /src/target/release/epoxy-server /usr/local/bin/epoxy-server
COPY backend/epoxy.toml /app/epoxy.toml
COPY backend/start.sh /app/start.sh
RUN chmod 755 /app/start.sh
ENV PORT=8080
EXPOSE 8080
CMD ["/app/start.sh"]
