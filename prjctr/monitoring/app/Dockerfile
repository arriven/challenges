FROM rust:latest as builder

COPY . .

RUN cargo install --path . --locked

FROM debian:bullseye-slim

COPY --from=builder /usr/local/cargo/bin/app /usr/local/bin/app

RUN chmod +x /usr/local/bin/app

ENTRYPOINT app