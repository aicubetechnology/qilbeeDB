# syntax=docker/dockerfile:1
FROM rust:1.93.1-bookworm@sha256:7c4ae649a84014c467d79319bbf17ce2632ae8b8be123ac2fb2ea5be46823f31 AS build
RUN apt-get update && apt-get install -y --no-install-recommends clang libclang-dev cmake pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*
WORKDIR /src
ENV CARGO_BUILD_JOBS=2 CARGO_PROFILE_RELEASE_LTO=thin CARGO_PROFILE_RELEASE_CODEGEN_UNITS=8
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY crates ./crates
COPY docs/api/openapi.json ./docs/api/openapi.json
RUN --mount=type=cache,id=qilbeedb-cargo-registry,target=/usr/local/cargo/registry \
    --mount=type=cache,id=qilbeedb-cargo-git,target=/usr/local/cargo/git \
    --mount=type=cache,id=qilbeedb-release-target,target=/src/target \
    cargo build --locked --release -p qilbee-server --bin qilbeedb && \
    install -m 0755 target/release/qilbeedb /usr/local/bin/qilbeedb

FROM debian:bookworm-slim@sha256:3783cc01769c7b2b1b83a5c5ad96c815348e28ed7da68e2e3687004faa906251 AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl libssl3 libstdc++6 && rm -rf /var/lib/apt/lists/* && \
    groupadd --gid 10001 qilbee && useradd --uid 10001 --gid 10001 --no-create-home --shell /usr/sbin/nologin qilbee && \
    install -d -o qilbee -g qilbee -m 0700 /data
COPY --from=build /usr/local/bin/qilbeedb /usr/local/bin/qilbeedb
COPY LICENSE /usr/share/doc/qilbeedb/LICENSE
ARG VCS_REF=unknown
LABEL org.opencontainers.image.title="QilbeeDB" \
    org.opencontainers.image.version="0.2.0" \
    org.opencontainers.image.revision=$VCS_REF \
    org.opencontainers.image.source="https://github.com/aicubetechnology/qilbeeDB"
USER 10001:10001
WORKDIR /data
EXPOSE 7474
HEALTHCHECK --interval=15s --timeout=3s --start-period=10s --retries=3 CMD curl --fail --silent http://127.0.0.1:7474/health > /dev/null || exit 1
ENTRYPOINT ["/usr/local/bin/qilbeedb"]
CMD ["/data"]
