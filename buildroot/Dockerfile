# SomaOS Buildroot Docker Build Environment
# Produces a bootable x86_64 Linux ISO + disk image

FROM --platform=linux/amd64 ubuntu:22.04

ENV DEBIAN_FRONTEND=noninteractive

# Install Buildroot dependencies + Rust cross-compile deps
RUN apt-get update && apt-get install -y \
    build-essential \
    gcc g++ \
    make cmake \
    libncurses5-dev \
    libelf-dev \
    libssl-dev \
    flex bison \
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
    device-tree-compiler \
    dosfstools mtools \
    genext2fs \
    xorriso \
    musl-tools \
    libdrm-dev \
    libevdev-dev \
    && rm -rf /var/lib/apt/lists/*

# Install Rust with musl target for fully static binaries
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
    && . "$HOME/.cargo/env" \
    && rustup target add x86_64-unknown-linux-musl

ENV PATH="/root/.cargo/bin:${PATH}"

# Get Buildroot 2024.02 LTS
WORKDIR /opt
RUN wget -q https://buildroot.org/downloads/buildroot-2024.02.2.tar.gz \
    && tar xf buildroot-2024.02.2.tar.gz \
    && rm buildroot-2024.02.2.tar.gz \
    && mv buildroot-2024.02.2 buildroot

# Copy build configs (both architectures)
COPY soma_defconfig /opt/buildroot/configs/soma_defconfig
COPY soma_aarch64_defconfig /opt/buildroot/configs/soma_aarch64_defconfig
COPY post-build.sh /opt/post-build.sh
RUN chmod +x /opt/post-build.sh

# Copy overlay files (systemd units, etc.)
COPY overlay/ /opt/overlay/

# Working directory for the build
WORKDIR /opt/buildroot

# Default: build the full image.
# Use Buildroot's own BR2_JLEVEL knob instead of top-level make jobserver flags.
# This avoids nested kernel/header builds inheriting broken jobserver FDs under Docker.
ENV SOMA_BUILD_JOBS=1
ENV SOMA_DEFCONFIG=soma_defconfig
CMD ["bash", "-lc", "make ${SOMA_DEFCONFIG} && make BR2_JLEVEL=${SOMA_BUILD_JOBS}"]
