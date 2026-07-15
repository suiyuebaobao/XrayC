# syntax=docker/dockerfile:1.7

FROM rust:1-bookworm AS builder

ENV PATH="/usr/local/cargo/bin:${PATH}"
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN --mount=type=cache,target=/usr/local/cargo/registry \
  --mount=type=cache,target=/app/target,id=xrayc-target-bookworm-v1 \
  cargo build --release --workspace --locked \
  && mkdir -p /app/dist-bin \
  && cp /app/target/release/xrayc-api /app/dist-bin/xrayc-api \
  && cp /app/target/release/xrayc-worker /app/dist-bin/xrayc-worker \
  && cp /app/target/release/xrayc-access-agent /app/dist-bin/xrayc-access-agent

FROM postgres:16-bookworm AS pg-client

FROM debian:bookworm-slim AS runtime

# access-agent 运行时需自签节点全部域名证书:certbot 跑 ACME 签发/续期,
# python3-certbot-dns-cloudflare 提供 DNS-01 插件(CF token 同账号通签、免占 80 端口),
# openssl 做证书有效期 x509 -checkend 判活;清 apt 缓存保持镜像精简。
RUN apt-get update \
  && apt-get install -y --no-install-recommends bash ca-certificates curl iproute2 iptables kmod libpq5 openssh-client sshpass rsync certbot python3-certbot-dns-cloudflare openssl \
  && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY --from=pg-client /usr/lib/postgresql/16 /usr/lib/postgresql/16
COPY --from=pg-client /usr/share/postgresql/16 /usr/share/postgresql/16
COPY --from=builder /app/dist-bin/xrayc-api /usr/local/bin/xrayc-api
COPY --from=builder /app/dist-bin/xrayc-worker /usr/local/bin/xrayc-worker
COPY --from=builder /app/dist-bin/xrayc-access-agent /usr/local/bin/xrayc-access-agent
COPY migrations ./migrations
COPY scripts/deploy-access-agent.sh ./scripts/deploy-access-agent.sh
COPY scripts/lib/deploy-access-agent ./scripts/lib/deploy-access-agent
RUN chmod 0755 ./scripts/deploy-access-agent.sh

ENV API_BIND=0.0.0.0:3000
ENV PATH="/usr/lib/postgresql/16/bin:${PATH}"
EXPOSE 3000

CMD ["xrayc-api"]
