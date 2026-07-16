# syntax=docker/dockerfile:1.7
FROM rust:1.88.0-bookworm AS builder

RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates musl-tools \
    && rm -rf /var/lib/apt/lists/* \
    && rustup target add wasm32-unknown-unknown x86_64-unknown-linux-musl

WORKDIR /src
COPY . .

ARG SOURCE_REVISION
RUN test -n "${SOURCE_REVISION}"

RUN cargo build --locked --release -p boon_app_cli
RUN cargo build --locked --release --target x86_64-unknown-linux-musl -p boon_app_server
RUN cargo build --locked --release --target wasm32-unknown-unknown -p boon_web_host
RUN ./target/release/boon-app build apps/fjordpulse/app.toml \
      --out /opt/boon/app \
      --mode live \
      --namespace production \
      --browser-wasm target/wasm32-unknown-unknown/release/boon_web_host.wasm \
      --source-revision "${SOURCE_REVISION}"
RUN mkdir -p /image-data-dir

FROM scratch

ARG SOURCE_REVISION
LABEL org.opencontainers.image.title="FjordPulse on Boon" \
      org.opencontainers.image.revision="${SOURCE_REVISION}" \
      org.opencontainers.image.source="https://github.com/BoonLang/boon-circuit" \
      io.boon.package.manifest="/opt/boon/app/bundle.json" \
      io.boon.protocol.version="1"

COPY --from=builder /etc/ssl/certs/ca-certificates.crt /etc/ssl/certs/ca-certificates.crt
COPY --from=builder /src/target/x86_64-unknown-linux-musl/release/boon-app-server /usr/local/bin/boon-app-server
COPY --from=builder --chown=65532:65532 /opt/boon/app /opt/boon/app
COPY --from=builder --chown=65532:65532 /image-data-dir /var/lib/boon/fjordpulse

ENV BOON_PACKAGE_DIR=/opt/boon/app \
    SSL_CERT_FILE=/etc/ssl/certs/ca-certificates.crt

USER 65532:65532
VOLUME ["/var/lib/boon/fjordpulse"]
EXPOSE 8080
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=10s --timeout=3s --start-period=30s --retries=6 \
  CMD ["/usr/local/bin/boon-app-server", "healthcheck"]
ENTRYPOINT ["/usr/local/bin/boon-app-server", "serve"]

