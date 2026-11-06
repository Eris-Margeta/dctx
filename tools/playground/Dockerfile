# Self-contained build of the dctx playground image. Context: the repository root.
#   docker build -f tools/playground/Dockerfile --build-arg DCTX_GIT_SHA=$(git rev-parse --short HEAD) -t dctx-play .
# The deploy path uses dist/ and Dockerfile.runtime instead, so the deploy host needs no Rust.
FROM rust:1.99.0-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends musl-tools && rm -rf /var/lib/apt/lists/* \
 && rustup target add x86_64-unknown-linux-musl
WORKDIR /src
COPY . .
ARG DCTX_GIT_SHA=unknown
ENV DCTX_GIT_SHA=${DCTX_GIT_SHA} CC_x86_64_unknown_linux_musl=musl-gcc
RUN cargo build --locked --release --target x86_64-unknown-linux-musl -p dctx-cli \
 && cargo build --locked --release --target x86_64-unknown-linux-musl --manifest-path tools/playground/Cargo.toml \
 && mkdir -p /out/samples \
 && cp -R tools/website/sample /out/samples/kiosk \
 && cp -R crates/dctx-lex-go/tests/fixtures /out/samples/lexer-go \
 && cp -R crates/dctx-map-parse/tests/fixtures /out/samples/parser-mixed \
 && cp tools/playground/samples.tsv /out/samples/index.tsv \
 && chmod -R a+rX /out/samples

FROM gcr.io/distroless/static-debian12:nonroot
COPY --from=build /src/target/x86_64-unknown-linux-musl/release/dctx-cli /usr/local/bin/dctx
COPY --from=build /src/tools/playground/target/x86_64-unknown-linux-musl/release/dctx-play /usr/local/bin/dctx-play
COPY --from=build /out/samples/ /samples/
COPY tools/playground/openapi.json /openapi.json
USER 65532:65532
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=3s --retries=3 CMD ["/usr/local/bin/dctx-play", "--health"]
ENTRYPOINT ["/usr/local/bin/dctx-play", "--listen", "0.0.0.0:8080", "--samples", "/samples", "--dctx", "/usr/local/bin/dctx", "--work", "/run/play"]
