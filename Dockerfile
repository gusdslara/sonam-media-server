FROM node:24-bookworm-slim AS frontend
WORKDIR /app
COPY package.json package-lock.json ./
RUN npm ci
COPY index.html vite.config.ts tsconfig.json ./
COPY src ./src
RUN npm run build

FROM rust:1-slim-bookworm AS backend
RUN apt-get update && apt-get install -y --no-install-recommends gcc libc6-dev pkg-config && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY server ./server
COPY --from=frontend /app/dist ./dist
RUN cargo build --release --locked --manifest-path server/Cargo.toml

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ffmpeg ca-certificates && rm -rf /var/lib/apt/lists/* && useradd --uid 10001 --create-home sonam && mkdir /data && chown sonam:sonam /data
COPY --from=backend /app/server/target/release/sonam-server /usr/local/bin/sonam
USER sonam
ENV SONAM_DATA=/data SONAM_BIND=0.0.0.0:8096
VOLUME /data
EXPOSE 8096
ENTRYPOINT ["sonam"]
