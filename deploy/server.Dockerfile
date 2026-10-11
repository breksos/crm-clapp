# The Breksos CRM server: one static-ish binary, no shell to speak of, not root.
#
# Build from the REPOSITORY ROOT — the server depends on ../crates/identity, which the app's
# core shares, so the build context has to contain both:
#
#   docker build -f deploy/server.Dockerfile -t crm-server .
#
# (deploy/compose.yaml does this for you.) Nothing in this file reaches out to anything but
# the base images and crates.io; there is no key, token or secret at build time.

# ---- build ---------------------------------------------------------------------------
FROM rust:1-bookworm AS build
WORKDIR /src
COPY crates ./crates
COPY server ./server
# --locked: the committed Cargo.lock is the build, not a suggestion.
RUN cargo build --release --locked --manifest-path server/Cargo.toml --bin crm-server

# ---- run -----------------------------------------------------------------------------
# Same Debian release as the builder, so the glibc it was linked against is the glibc it runs on.
FROM debian:bookworm-slim
# curl is here for the HEALTHCHECK and nothing else.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/* \
 && useradd --system --uid 10001 --no-create-home --shell /usr/sbin/nologin crm
COPY --from=build /src/server/target/release/crm-server /usr/local/bin/crm-server
USER 10001
# Inside the container it must listen on every interface; the compose file decides who on the
# host can reach it (loopback only, by default).
ENV BIND=0.0.0.0:8787
EXPOSE 8787
HEALTHCHECK --interval=15s --timeout=3s --start-period=20s --retries=3 \
  CMD curl -fsS http://127.0.0.1:8787/healthz >/dev/null || exit 1
ENTRYPOINT ["/usr/local/bin/crm-server"]
