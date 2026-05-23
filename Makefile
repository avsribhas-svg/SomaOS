# SomaOS build targets

.PHONY: build check test build-installer-x86 build-installer-arm64 clean

build:
	cargo build -p soma-agent -p soma-compositor -p soma-cli -p soma-updater

check:
	cargo check -p soma-agent --target x86_64-unknown-linux-musl
	cargo check -p soma-agent --target aarch64-unknown-linux-musl
	cargo check -p soma-compositor --features drm-backend --no-default-features --target x86_64-unknown-linux-musl
	cargo check -p soma-compositor --features drm-backend --no-default-features --target aarch64-unknown-linux-musl
	cargo check -p soma-updater --target x86_64-unknown-linux-musl

test:
	cargo run -p soma-cli -- --test

# USB installer image — x86_64 (requires buildroot checkout at ./buildroot)
build-installer-x86:
	cargo build --release --target x86_64-unknown-linux-musl -p soma-agent
	cargo build --release --target x86_64-unknown-linux-musl -p soma-compositor \
	    --features drm-backend --no-default-features
	cargo build --release --target x86_64-unknown-linux-musl -p soma-updater
	cp target/x86_64-unknown-linux-musl/release/soma-agent      buildroot/installer_overlay/usr/bin/
	cp target/x86_64-unknown-linux-musl/release/soma-compositor buildroot/installer_overlay/usr/bin/
	cp target/x86_64-unknown-linux-musl/release/soma-updater    buildroot/installer_overlay/usr/bin/
	chmod +x buildroot/installer_overlay/usr/bin/*
	chmod +x buildroot/post-build-installer.sh
	make -C buildroot soma_installer_defconfig && make -C buildroot
	@echo "Installer image: buildroot/output/images/sdcard.img"

# USB installer image — ARM64
build-installer-arm64:
	cargo build --release --target aarch64-unknown-linux-musl -p soma-agent
	cargo build --release --target aarch64-unknown-linux-musl -p soma-compositor \
	    --features drm-backend --no-default-features
	cargo build --release --target aarch64-unknown-linux-musl -p soma-updater
	cp target/aarch64-unknown-linux-musl/release/soma-agent      buildroot/installer_overlay/usr/bin/soma-agent-arm64
	cp target/aarch64-unknown-linux-musl/release/soma-compositor buildroot/installer_overlay/usr/bin/soma-compositor-arm64
	cp target/aarch64-unknown-linux-musl/release/soma-updater    buildroot/installer_overlay/usr/bin/soma-updater-arm64
	@echo "ARM64 binaries copied to installer_overlay/"

clean:
	cargo clean
