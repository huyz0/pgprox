# The proxy and the mock sidecar, from one build.
#
# Two binaries in one image on purpose: the e2e stack runs the sidecar beside
# each proxy the way a real deployment does, and a second image would mean a
# second build of the same workspace for one small binary.
FROM rust:1-slim-bookworm AS build

RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config libssl-dev protobuf-compiler cmake clang \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /src
COPY . .
# The mock sidecar is behind the `integration` feature, which is what keeps it
# out of a release build.
#
# Cache mounts on the registry and the target directory, because without them
# every e2e run is a cold build of the whole workspace, and a check that takes
# ten minutes to tell you about a typo is a check nobody runs. The binaries are
# copied out of the cache in the same layer, since a cache mount does not
# survive into the image.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release -p pgprox \
 && cargo build --release -p pgprox-auth --features integration --bin mock_sidecar \
 && mkdir -p /out \
 && cp target/release/pgprox target/release/mock_sidecar /out/

FROM debian:bookworm-slim
# curl is here for the container healthcheck and for an operator inside the
# container asking the admin API a question. The runtime shell is dash, which
# has no /dev/tcp, so without it a health check would have to be a second
# binary.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*

COPY --from=build /out/pgprox /usr/local/bin/pgprox
COPY --from=build /out/mock_sidecar /usr/local/bin/mock_sidecar

# Nothing in the image decides anything: the configuration is mounted and the
# node's identity is an argument.
ENTRYPOINT ["/usr/local/bin/pgprox"]
