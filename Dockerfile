FROM rust:slim-buster as build

# create a new empty shell project
RUN USER=root cargo new --bin app
WORKDIR /app

# copy over your manifests
COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml

# this build step will cache your dependencies
RUN cargo build --release
RUN rm src/*.rs

# copy your source tree
COPY ./src ./src

# build for release
RUN rm ./target/release/deps/nostr_tx_broadcast*
RUN cargo build --release

# our final base
FROM debian:buster-slim

# copy the build artifact from the build stage
COPY --from=build /app/target/release/submitpackage .
COPY --from=build /app/target/release/nostr-tx-broadcast .

# set the startup command to run your binary
CMD ["./nostr-tx-broadcast"]


