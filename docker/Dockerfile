# syntax=docker/dockerfile:1
#
# The `energyml` command in a container: everything the CLI does, on files
# mounted at /work.
#
#   docker run --rm -v "$PWD:/work" ghcr.io/geosiris-technologies/energyml check model.epc
#   docker run --rm -v "$PWD:/work" ghcr.io/geosiris-technologies/energyml export model.epc --format obj -o out
#
# Built from this checkout by `.github/workflows/docker.yml`, never by hand: the
# image is the code of the commit that built it, and carries that commit in its
# labels and in /opt/energyml/REF.
#
# Programs of other repositories (`retp` for ETP) are not in this image:
# `energyml etp ...` says which one to install. Their own images live beside
# this one (`ghcr.io/geosiris-technologies/retp`).

FROM rust:1-bookworm AS build
RUN apt-get update \
 && apt-get install -y --no-install-recommends libhdf5-dev pkg-config \
 && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
# Extra features of `energyml-cli`, comma-separated (`hdf5` reads the arrays and
# is always on; `png` rasterises the section view).
ARG FEATURES=png
# `fat` is the release profile's own; `thin` when a runner lacks time or memory.
ARG RELEASE_LTO=fat
ENV CARGO_PROFILE_RELEASE_LTO=${RELEASE_LTO}
# Only the downloaded archives are cached, with `sharing=locked`: two builds
# never unpack the same crate at once, and a half-unpacked crate never persists.
RUN --mount=type=cache,target=/usr/local/cargo/registry/index,sharing=locked \
    --mount=type=cache,target=/usr/local/cargo/registry/cache,sharing=locked \
    set -eu; \
    features="energyml-cli/hdf5"; \
    for f in $(echo "$FEATURES" | tr ',' ' '); do features="$features,energyml-cli/$f"; done; \
    cargo build --release --locked -p energyml-cli --features "$features"; \
    mkdir /out; \
    cp target/release/energyml target/release/enyml /out/

FROM debian:bookworm-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends libhdf5-103-1 ca-certificates python3 \
 && rm -rf /var/lib/apt/lists/*
COPY --from=build /out/ /usr/local/bin/
COPY --chmod=755 docker/entrypoint.sh /usr/local/bin/energyml-entrypoint
# The runner of the generated Galaxy tools (docs/galaxy.md).
COPY --from=build /src/scripts/galaxy /opt/energyml/scripts/galaxy
ARG REVISION=unknown
ARG VERSION=dev
RUN echo "$REVISION" > /opt/energyml/REF
LABEL org.opencontainers.image.title="energyml" \
      org.opencontainers.image.description="The energyml command: read, check, export, create and repair EnergyML packages (EPC + HDF5)" \
      org.opencontainers.image.source="https://github.com/geosiris-technologies/energyml-rust" \
      org.opencontainers.image.url="https://www.geosiris.com" \
      org.opencontainers.image.vendor="Geosiris" \
      org.opencontainers.image.licenses="Apache-2.0" \
      org.opencontainers.image.version="$VERSION" \
      org.opencontainers.image.revision="$REVISION"
ENV LANG=C.UTF-8 \
    PYTHONUTF8=1
# Not root: a file written into a mounted folder belongs to uid 1000, the usual
# first user of a Linux host.
RUN useradd --uid 1000 --create-home energyml
USER energyml
WORKDIR /work
ENTRYPOINT ["/usr/local/bin/energyml-entrypoint"]
CMD ["--help"]
