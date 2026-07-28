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
 && cargo build --release -p pgload \
 && mkdir -p /out \
 && cp target/release/pgprox target/release/mock_sidecar target/release/pgload /out/

# The FIPS variant.
#
# A separate stage rather than a build argument, because the toolchain is
# different: `aws-lc-fips-sys` compiles AWS-LC from source, and the FIPS module
# is delocated, meaning its assembly is rewritten into one contiguous text
# section whose hash is verified at startup. That needs cmake, Go and a
# compiler the rewriter accepts.
#
# It sits before the default runtime stage on purpose. Docker builds the last
# stage when no --target is given, so putting this after would quietly make
# every compose build a FIPS build. Nothing here is built unless it is asked
# for by name.
FROM rust:1-slim-bookworm AS build-fips

# golang and make are the additions over the default stage. The delocate step
# is a Go program that ships with AWS-LC, and cmake generates Unix Makefiles
# here, so without make it configures and then fails with `CMAKE_MAKE_PROGRAM
# is not set`, which reads like a cmake problem and is a missing package.
RUN apt-get update \
 && apt-get install -y --no-install-recommends pkg-config libssl-dev protobuf-compiler cmake clang golang make \
 && rm -rf /var/lib/apt/lists/*

# gcc emits `.data.rel.ro.local` for the FIPS module's relocatable read-only
# tables once optimisation is on, and the delocate step refuses any .data
# section in the module. A release build is optimised by definition, so the
# default compiler cannot build this image at all. See scripts/fips-check.sh.
ENV CC=clang CXX=clang++

WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target \
    cargo build --release -p pgprox --features fips \
 && mkdir -p /out \
 && cp target/release/pgprox /out/

FROM debian:bookworm-slim AS fips
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && rm -rf /var/lib/apt/lists/*
# No openssl and no mock sidecar. This image is what a FIPS deployment runs, so
# it carries the proxy and nothing that exists for testing: a mock sidecar in a
# validated image is a credential resolver an auditor has to be told to ignore.
COPY --from=build-fips /out/pgprox /usr/local/bin/pgprox
ENTRYPOINT ["/usr/local/bin/pgprox"]

FROM debian:bookworm-slim
# curl is here for the container healthcheck and for an operator inside the
# container asking the admin API a question. The runtime shell is dash, which
# has no /dev/tcp, so without it a health check would have to be a second
# binary.
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl openssl \
 && rm -rf /var/lib/apt/lists/*

COPY --from=build /out/pgprox /usr/local/bin/pgprox
COPY --from=build /out/mock_sidecar /usr/local/bin/mock_sidecar
# The load client ships in the same image so a scale run can generate its load
# from inside the network. On the machine this was built on, the host has 4,096
# ephemeral ports and a container has 28,231, so ten thousand connections to
# one address is possible from in here and not from out there. It also skips
# the published-port forwarder, which is a hop that is not the proxy.
COPY --from=build /out/pgload /usr/local/bin/pgload

# Nothing in the image decides anything: the configuration is mounted and the
# node's identity is an argument.
ENTRYPOINT ["/usr/local/bin/pgprox"]
