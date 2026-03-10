# Stage 1: Build everything (SPA + binaries) in one stage to share cache invalidation
FROM rust:1.89-bookworm AS builder

ARG GIT_SHA=dev
ENV GIT_SHA=${GIT_SHA}

# Install WASM toolchain for SPA build
RUN rustup target add wasm32-unknown-unknown
RUN cargo install trunk

# Pre-download wasm-bindgen binary (trunk needs the musl static build;
# downloading it here with retries avoids transient failures during trunk build)
ARG WASM_BINDGEN_VERSION=0.2.108
RUN curl -sSL --retry 5 --retry-delay 5 \
    "https://github.com/rustwasm/wasm-bindgen/releases/download/${WASM_BINDGEN_VERSION}/wasm-bindgen-${WASM_BINDGEN_VERSION}-x86_64-unknown-linux-musl.tar.gz" \
    -o /tmp/wb.tar.gz && \
    mkdir -p /root/.trunk/tools/wasm-bindgen-${WASM_BINDGEN_VERSION} && \
    tar xzf /tmp/wb.tar.gz --strip-components=1 \
    -C /root/.trunk/tools/wasm-bindgen-${WASM_BINDGEN_VERSION}/ && \
    rm /tmp/wb.tar.gz

WORKDIR /app
COPY . .

# Build the SPA (Trunk compiles Leptos to WASM)
RUN cd crates/tempo-x402-app && trunk build --release

# Build the gateway and node binaries
RUN cargo build --release --package tempo-x402-gateway --package tempo-x402-node

# Stage 2: Runtime
FROM rust:1.89-bookworm

RUN apt-get update && apt-get install -y \
    ca-certificates gosu git curl \
    jq python3 bc \
    && rm -rf /var/lib/apt/lists/*

# Install GitHub CLI for soul PR/issue creation
RUN curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg \
    -o /usr/share/keyrings/githubcli-archive-keyring.gpg && \
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" \
    > /etc/apt/sources.list.d/github-cli.list && \
    apt-get update && apt-get install -y gh && \
    rm -rf /var/lib/apt/lists/*

RUN groupadd -r app && useradd -r -g app -d /app app

COPY --from=builder /app/target/release/x402-gateway /usr/local/bin/x402-gateway
COPY --from=builder /app/target/release/x402-node /usr/local/bin/x402-node
COPY --from=builder /app/crates/tempo-x402-app/dist /app/spa

RUN chown -R app:app /app

# Entrypoint: fix volume permissions then drop to non-root
# Use X402_BINARY env var to select binary (default: x402-node)
RUN printf '#!/bin/sh\nchown -R app:app /data 2>/dev/null || true\nBIN=${X402_BINARY:-x402-node}\nexec gosu app "$BIN" "$@"\n' > /entrypoint.sh && chmod +x /entrypoint.sh

ENV SPA_DIR=/app/spa
ENV PORT=4023
ENV DB_PATH=/data/gateway.db
ENV NONCE_DB_PATH=/data/x402-nonces.db

EXPOSE 4023

# Health check: verify the gateway is responsive
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD wget -q -O /dev/null http://localhost:4023/health || exit 1

ENTRYPOINT ["/entrypoint.sh"]
