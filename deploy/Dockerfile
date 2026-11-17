# The operator's image, one for every service but the node
# (deploy/docker-compose.yml): the hub (`glichen-hub`), the wallet
# (`glichen`, for deploy/smoke.sh), and the prover `glichen-r0` (RISC Zero
# 3.1.0, in process) with the committed guest it reads. A Groth16 wrap
# runs RISC Zero's prover image, through the host's Docker (its socket
# mounted). Build context: a directory holding glichen/ (deploy/build.sh
# makes it); lichen and lichen-btc come by git.
FROM rust:1.96-bookworm AS build
WORKDIR /src
COPY glichen glichen
WORKDIR /src/glichen
# the commit the hub reports in its offer (`GET /info`)
ARG LICHEN_COMMIT=
RUN cargo build --release -p glichen-hub -p glichen-wallet
RUN mkdir -p /out && cp target/release/glichen-hub target/release/glichen /out/
# the prover: glichen's RISC Zero workspace (the guest is committed, so
# building the host needs no guest toolchain)
RUN apt-get update && apt-get install -y --no-install-recommends clang && rm -rf /var/lib/apt/lists/*
RUN cargo build --release --locked --manifest-path r0/Cargo.toml -p glichen-r0 && cp r0/target/release/glichen-r0 /out/ \
    && /out/glichen-r0 check-id

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*
# the docker CLI for the wrap (static; bookworm's docker.io speaks an API
# older than a current daemon's minimum)
COPY --from=docker:28.5.1-cli /usr/local/bin/docker /usr/local/bin/docker
COPY --from=build /out/glichen-hub /out/glichen /out/glichen-r0 /usr/local/bin/
# glichen-r0 reads the committed guest from its source tree's path,
# r0/host/../guests: r0/host has to exist for that to resolve
COPY --from=build /src/glichen/r0/guests /src/glichen/r0/guests
RUN mkdir -p /src/glichen/r0/host
COPY glichen/r0/hp-prover.sh /usr/local/bin/glichen-hp-prover
COPY glichen/deploy/entry.sh /usr/local/bin/glichen-entry
ENTRYPOINT ["/usr/local/bin/glichen-entry"]
