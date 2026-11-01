# Build: docker build -t quill .
# Run:   docker run -d --name quill -p 127.0.0.1:3000:3000 -v quill-data:/data quill
FROM rust:1-slim-bookworm AS build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY crates crates
COPY src src
COPY migrations migrations
COPY templates templates
COPY static static
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN useradd --system --uid 10001 --home-dir /data --shell /usr/sbin/nologin quill \
    && mkdir -p /data && chown quill:quill /data
COPY --from=build /src/target/release/quill /usr/local/bin/quill
USER quill
ENV QUILL_DATA_DIR=/data \
    QUILL_LISTEN=0.0.0.0:3000
VOLUME ["/data"]
EXPOSE 3000
STOPSIGNAL SIGTERM
ENTRYPOINT ["quill"]
CMD ["serve"]
