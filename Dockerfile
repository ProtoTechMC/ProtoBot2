FROM rust:latest

WORKDIR /home/container

COPY . .
#     -

RUN --mount=type=cache,target=/root/.cargo/bin \
    --mount=type=cache,target=/root/.cargo/registry/index \
    --mount=type=cache,target=/root/.cargo/registry/cache \
    --mount=type=cache,target=/root/.cargo/git/db \
    --mount=type=cache,target=target \
    cargo install --path .

CMD ["protobot"]
