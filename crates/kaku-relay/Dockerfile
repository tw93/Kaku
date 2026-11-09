# Built from the repository root (see crates/AGENTS.md), so the relay can
# carry the web terminal's page and fonts from crates/kaku-remote.
FROM rust:1.95-slim AS builder
WORKDIR /app

# The relay crate is standalone (no workspace deps); the lockfile pins the
# same dependency versions CI checks. Only the page files it embeds come
# from elsewhere, at the same relative paths.
COPY crates/kaku-relay/Cargo.toml crates/kaku-relay/Cargo.lock crates/kaku-relay/
COPY crates/kaku-relay/src crates/kaku-relay/src
COPY crates/kaku-remote/assets/web crates/kaku-remote/assets/web

WORKDIR /app/crates/kaku-relay
RUN cargo build --release --locked

FROM debian:bookworm-slim
# The relay needs no privileges: it listens on 8080 and writes nothing to
# disk, so a flaw in it should not run as root in the machine.
RUN useradd --system --uid 10001 --no-create-home --shell /usr/sbin/nologin kaku-relay
COPY --from=builder /app/crates/kaku-relay/target/release/kaku-relay /usr/local/bin/kaku-relay
ENV RUST_LOG=kaku_relay=info
EXPOSE 8080
USER kaku-relay
CMD ["kaku-relay"]
