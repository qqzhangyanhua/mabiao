# 构建上下文是仓库根（要带上 crates/push-protocol）：
#   docker build -f server/Dockerfile .
FROM rust:1-bookworm AS build
WORKDIR /src
COPY crates ./crates
COPY server ./server
WORKDIR /src/server
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 mabiao
COPY --from=build /src/server/target/release/mabiao-server /usr/local/bin/mabiao-server
USER mabiao
EXPOSE 8080
ENTRYPOINT ["mabiao-server"]
CMD ["serve"]
