FROM rust:nightly-bookworm AS epoxy-build
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential pkg-config libssl-dev ca-certificates git \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
RUN git clone --depth 1 --branch multiplexed https://github.com/MercuryWorkshop/epoxy-tls.git .
RUN cargo build --release --manifest-path server/Cargo.toml

FROM caddy:2 AS caddy-bin

FROM node:22-bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates sed bash \
    && rm -rf /var/lib/apt/lists/*
COPY --from=epoxy-build /src/target/release/epoxy-server /usr/local/bin/epoxy-server
COPY --from=caddy-bin /usr/bin/caddy /usr/bin/caddy
WORKDIR /app
COPY backend/epoxy.toml /app/epoxy.toml
COPY backend/Caddyfile /etc/caddy/Caddyfile
COPY backend/start.sh /app/start.sh
COPY backend/http-proxy/package.json /app/http-proxy/package.json
COPY backend/http-proxy/server.mjs /app/http-proxy/server.mjs
WORKDIR /app/http-proxy
RUN npm install --omit=dev
WORKDIR /app
RUN chmod 755 /app/start.sh
ENV PORT=8080
EXPOSE 8080
CMD ["/app/start.sh"]
