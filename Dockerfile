FROM --platform=$BUILDPLATFORM rust:1.86.0-alpine3.21 AS builder
ENV PKGCONFIG_SYSROOTDIR=/
RUN apk add --no-cache musl-dev perl build-base zig
RUN cargo install --locked cargo-zigbuild
RUN rustup target add x86_64-unknown-linux-musl aarch64-unknown-linux-musl
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY migration ./migration
RUN mkdir src \
    && echo "fn main() {}" > src/main.rs \
    && cargo fetch \
    && cargo zigbuild --release --locked --target x86_64-unknown-linux-musl --target aarch64-unknown-linux-musl \
    && rm src/main.rs
COPY static ./static
COPY templates ./templates
COPY src ./src
RUN touch src/main.rs \
    && cargo zigbuild --release --locked --target x86_64-unknown-linux-musl --target aarch64-unknown-linux-musl

FROM --platform=$BUILDPLATFORM scratch AS binary
COPY --from=builder /app/target/x86_64-unknown-linux-musl/release/samey /samey-linux-amd64
COPY --from=builder /app/target/aarch64-unknown-linux-musl/release/samey /samey-linux-arm64

FROM alpine:3.21 AS runner
ARG TARGETOS
ARG TARGETARCH
RUN apk add --no-cache ffmpeg
COPY --from=binary /samey-${TARGETOS}-${TARGETARCH} /usr/bin/samey
ENTRYPOINT [ "samey" ]
