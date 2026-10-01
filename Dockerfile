# SPDX-License-Identifier: Apache-2.0
ARG RUST_IMAGE=rust:1.98.1-trixie
FROM scratch AS cargo_cache
WORKDIR /cache

FROM ${RUST_IMAGE} AS builder
ENV RUSTUP_TOOLCHAIN=1.98.1
ARG CARGO_NET_OFFLINE=false
WORKDIR /build
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY src ./src
COPY scripts/query-patches.ps1 ./scripts/query-patches.ps1
COPY scripts/build-runtime-root.sh ./scripts/build-runtime-root.sh
RUN --mount=type=bind,from=cargo_cache,source=/,target=/dependency-cache,readonly \
    if [ -d /dependency-cache/registry ]; then cp -a /dependency-cache/. /usr/local/cargo/; fi && \
    cargo build --release --locked
RUN sh scripts/build-runtime-root.sh

FROM scratch AS runtime
LABEL org.opencontainers.image.title="PatchPulse" \
      org.opencontainers.image.description="Read-only Windows patch health API; Linux backends report unsupported" \
      org.opencontainers.image.licenses="Apache-2.0"
WORKDIR /app
COPY --from=builder /runtime-root/ /
COPY config/patchpulse.toml /etc/patchpulse/patchpulse.toml
COPY LICENSE THIRD_PARTY_NOTICES.md /usr/share/doc/patchpulse/
USER 65532:65532
EXPOSE 9100
HEALTHCHECK --interval=30s --timeout=6s --start-period=5s --retries=3 \
  CMD ["patchpulse", "--config", "/etc/patchpulse/patchpulse.toml", "--healthcheck"]
ENTRYPOINT ["patchpulse"]
CMD ["--foreground", "--config", "/etc/patchpulse/patchpulse.toml", "--bind", "0.0.0.0:9100"]
