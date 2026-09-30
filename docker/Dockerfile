# syntax=docker/dockerfile:1
FROM --platform=$BUILDPLATFORM node:22-bookworm-slim AS frontend
WORKDIR /app/frontend
RUN corepack enable && corepack prepare pnpm@9.15.9 --activate
COPY frontend/package.json frontend/pnpm-lock.yaml frontend/pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY openapi.json /app/openapi.json
COPY frontend/ ./
RUN pnpm build

FROM rust:1-bookworm AS backend
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ ./src/
COPY --from=frontend /app/frontend/dist ./frontend/dist
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates libgcc-s1 \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir /data && chown 65532:65532 /data
COPY --from=backend /app/target/release/wirehub /usr/local/bin/wirehub
USER 65532:65532
WORKDIR /data
ENV WIREHUB_PORT=51820 \
    WIREHUB_HTTP_BIND=0.0.0.0 \
    WIREHUB_DB=/data/wirehub.sqlite3 \
    WIREHUB_HUB_KEY=/data/wirehub.key
VOLUME /data
EXPOSE 51820/tcp 51820/udp
ENTRYPOINT ["wirehub"]
