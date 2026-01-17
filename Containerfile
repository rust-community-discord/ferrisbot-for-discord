FROM rust:latest as builder

ENV SQLX_OFFLINE=true
ENV DATABASE_URL=sqlite://database/ferris.sqlite3

WORKDIR /usr/src/ferrisbot

COPY Cargo.toml ./
COPY Cargo.lock ./

RUN mkdir -p src/bin \
 && printf "fn main() {}\n" > src/bin/main.rs \
 && printf "" > src/lib.rs
RUN cargo build --release
RUN rm -rf src

COPY . .
RUN cargo build --release


FROM cgr.dev/chainguard/glibc-dynamic:latest-dev

ARG APP=/usr/src/app

ENV TZ=Etc/UTC

WORKDIR ${APP}

COPY --from=builder /usr/src/ferrisbot/target/release/main ./ferrisbot

ENTRYPOINT ["./ferrisbot"]
