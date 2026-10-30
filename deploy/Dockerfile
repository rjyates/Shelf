FROM rust:1.99.1-bookworm AS build
WORKDIR /src
COPY server/ server/
COPY web/ web/
ARG VERSION=dev
ENV SHELF_VERSION=${VERSION}
RUN cargo build --manifest-path server/Cargo.toml --locked --release

FROM debian:bookworm-slim
LABEL org.opencontainers.image.source="https://github.com/rjyates/Shelf"
LABEL org.opencontainers.image.licenses="AGPL-3.0-only"
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --create-home shelf \
    && mkdir -p /data /books && chown shelf:shelf /data /books
COPY --from=build /src/server/target/release/shelf-server /usr/local/bin/shelf-server
COPY LICENSE /usr/share/licenses/shelf/LICENSE
COPY web/fonts/*LICENSE.txt /usr/share/licenses/shelf/fonts/
ENV SHELF_DATA=/data SHELF_LISTEN=0.0.0.0:8080
USER 10001:10001
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s --start-period=10s \
  CMD curl --fail --silent http://127.0.0.1:8080/api/v1/health || exit 1
ENTRYPOINT ["shelf-server"]
