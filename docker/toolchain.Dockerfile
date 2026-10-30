# Shared toolchain image: used as the base of the agent sandbox and as the grader.
# Contains NO spreadsheet software and NO spreadsheet libraries.
FROM debian:trixie-slim
ENV DEBIAN_FRONTEND=noninteractive LANG=C.UTF-8 LC_ALL=C.UTF-8
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential clang pkg-config cmake ca-certificates git \
      python3 python3-venv golang-go rustc cargo nodejs \
      zlib1g-dev libexpat1-dev libpugixml-dev libzip-dev \
      time procps file less util-linux \
    && rm -rf /var/lib/apt/lists/*
# Vendored Rust crates (the only third-party Rust code allowed). Cargo runs offline.
ENV CARGO_HOME=/opt/cargo CARGO_NET_OFFLINE=true GOFLAGS=-mod=mod GOPROXY=off GOTOOLCHAIN=local
COPY docker/rust-vendor /opt/rust-vendor-seed
RUN mkdir -p /opt/cargo /opt/vendor && cd /opt/rust-vendor-seed \
    && CARGO_NET_OFFLINE=false cargo vendor --versioned-dirs /opt/vendor/rust > /opt/cargo/config.toml \
    && printf '\n[net]\noffline = true\n' >> /opt/cargo/config.toml \
    && ls /opt/vendor/rust > /opt/vendor/RUST_CRATES.txt \
    && cargo build --release --offline && rm -rf target
