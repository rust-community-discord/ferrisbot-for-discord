FROM lukemathwalker/cargo-chef:latest-rust-1.93-slim-bookworm AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder 
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN SQLX_OFFLINE=true cargo build --release --bin main

FROM debian:bookworm-slim
ARG APP=/usr/src/app
WORKDIR ${APP}

ENV TZ=Etc/UTC
ENV DATABASE_URL=sqlite://database/ferris.sqlite3

COPY assets migrations ./
COPY --from=builder /app/target/release/main ./ferrisbot

ENTRYPOINT ["./ferrisbot"]
