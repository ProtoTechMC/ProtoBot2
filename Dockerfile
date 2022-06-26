FROM rust:latest

WORKDIR /home/container

COPY . .
#     -

RUN cargo install --path .

CMD ["protobot"]
