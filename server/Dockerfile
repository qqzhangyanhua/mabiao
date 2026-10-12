# 构建上下文是仓库根（要带上 crates/ 与桌面端内置的 LiteLLM 快照，统一价目的兜底价表）：
#   docker build -f server/Dockerfile .
FROM node:22-bookworm-slim AS web
# pnpm 版本由 server/web/package.json 的 packageManager 钉住。
RUN corepack enable
WORKDIR /web
COPY server/web/package.json server/web/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY server/web ./
RUN pnpm build

FROM rust:1-bookworm AS build
WORKDIR /src
COPY crates ./crates
COPY src-tauri/assets/litellm_prices.json ./src-tauri/assets/litellm_prices.json
COPY server ./server
WORKDIR /src/server
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 mabiao
COPY --from=build /src/server/target/release/mabiao-server /usr/local/bin/mabiao-server
COPY --from=web /web/dist /usr/share/mabiao/web
ENV MABIAO_WEB_DIR=/usr/share/mabiao/web
USER mabiao
EXPOSE 8080
ENTRYPOINT ["mabiao-server"]
CMD ["serve"]
