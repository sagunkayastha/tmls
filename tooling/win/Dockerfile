# Probe image: the tmls-rs build image plus what a Windows cross build needs.
FROM tmls-rs-build:latest
USER root
RUN apt-get update && apt-get install -y --no-install-recommends mingw-w64 \
    && rm -rf /var/lib/apt/lists/*
RUN rustup target add x86_64-pc-windows-gnu
USER 1000:1000
