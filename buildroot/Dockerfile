# SomaOS Buildroot Docker Build Environment
# Produces a bootable x86_64 Linux ISO + disk image

FROM --platform=linux/amd64 ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive

# Install Buildroot dependencies
RUN apt-get update && apt-get install -y \
    build-essential \
    gcc g++ \
    make cmake \
    libncurses5-dev \
    cpio \
    unzip \
    rsync \
    bc \
    git \
    wget curl \
    file \
    python3 python3-pip python3-setuptools \
    perl \
    patch \
    bzip2 xz-utils \
    libssl-dev \
    device-tree-compiler \
    dosfstools mtools \
    genext2fs \
    xorriso \
    && rm -rf /var/lib/apt/lists/*

# Install Rust (for cross-compiling soma binaries)
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
    && . "$HOME/.cargo/env" \
    && rustup target add x86_64-unknown-linux-gnu

ENV PATH="/root/.cargo/bin:${PATH}"

# Get Buildroot 2024.02 LTS
WORKDIR /opt
RUN wget -q https://buildroot.org/downloads/buildroot-2024.02.2.tar.gz \
    && tar xf buildroot-2024.02.2.tar.gz \
    && rm buildroot-2024.02.2.tar.gz \
    && mv buildroot-2024.02.2 buildroot

# Copy build configs
COPY soma_defconfig /opt/buildroot/configs/soma_defconfig
COPY post-build.sh /opt/post-build.sh
RUN chmod +x /opt/post-build.sh

# Copy overlay files (systemd units, etc.)
COPY overlay/ /opt/overlay/

# Working directory for the build
WORKDIR /opt/buildroot

# Default: build the full image
CMD ["bash", "-c", "make soma_defconfig && make -j$(nproc)"]
