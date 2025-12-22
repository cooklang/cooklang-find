# Makefile for cooklang-find
# Supports building for iOS and Android platforms

.PHONY: all build test clean ios android help install-deps

# Default target
all: build

# Build the library (native)
build:
	cargo build --release

# Run tests
test:
	cargo test

# Run lints
lint:
	cargo fmt --check
	cargo clippy -- -D warnings

# Format code
fmt:
	cargo fmt

# Clean build artifacts
clean:
	cargo clean
	rm -rf bindings/

# === iOS Builds ===

# Build for iOS
ios:
	@chmod +x scripts/build-swift.sh
	@scripts/build-swift.sh --all

# === Android Builds ===

# Build for Android
android:
	@chmod +x scripts/build-kotlin.sh
	@scripts/build-kotlin.sh --all

# === All Mobile Platforms ===

# Build for all mobile platforms
mobile: ios android
	@echo "All mobile builds complete!"

# === Bindings Generation ===

# Generate Swift bindings only
bindings-swift:
	@mkdir -p bindings/swift
	cargo build --release
	cargo run --release --features cli --bin uniffi-bindgen -- generate \
		--library target/release/libcooklang_find.dylib \
		--language swift \
		--config uniffi.toml \
		--out-dir bindings/swift

# Generate Kotlin bindings only
bindings-kotlin:
	@mkdir -p bindings/kotlin
	cargo build --release
	cargo run --release --features cli --bin uniffi-bindgen -- generate \
		--library target/release/libcooklang_find.dylib \
		--language kotlin \
		--config uniffi.toml \
		--out-dir bindings/kotlin

# Generate all bindings
bindings: bindings-swift bindings-kotlin
	@echo "All bindings generated in bindings/"

# === Dependencies ===

# Install development dependencies
install-deps:
	rustup target add aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios || true
	rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android i686-linux-android || true
	cargo install cargo-ndk || true

# === Release ===

# Create XCFramework zip for release
xcframework-zip: ios
	@echo "Creating XCFramework zip..."
	@cd bindings/swift && zip -r ../../CooklangFindFFI.xcframework.zip CooklangFind.xcframework
	@echo "Created CooklangFindFFI.xcframework.zip"
	@shasum -a 256 CooklangFindFFI.xcframework.zip

# Update Swift sources from generated bindings
update-swift-sources: bindings-swift
	@cp bindings/swift/Sources/CooklangFind/CooklangFind.swift Sources/CooklangFind/
	@echo "Swift sources updated"

# === Documentation ===

# Generate documentation
docs:
	cargo doc --no-deps
	@echo "Documentation generated at target/doc/cooklang_find/"

# === Help ===

help:
	@echo "cooklang-find build system"
	@echo ""
	@echo "Usage: make [target]"
	@echo ""
	@echo "Native targets:"
	@echo "  build         - Build native library (release)"
	@echo "  test          - Run tests"
	@echo "  lint          - Run lints (fmt check + clippy)"
	@echo "  fmt           - Format code"
	@echo "  docs          - Generate documentation"
	@echo "  clean         - Clean all build artifacts"
	@echo ""
	@echo "Mobile targets:"
	@echo "  ios           - Build iOS XCFramework + Swift bindings"
	@echo "  android       - Build Android library + Kotlin bindings"
	@echo "  mobile        - Build for both iOS and Android"
	@echo ""
	@echo "Bindings targets:"
	@echo "  bindings-swift   - Generate Swift bindings only"
	@echo "  bindings-kotlin  - Generate Kotlin bindings only"
	@echo "  bindings         - Generate all bindings"
	@echo ""
	@echo "Release targets:"
	@echo "  xcframework-zip     - Create XCFramework zip for GitHub release"
	@echo "  update-swift-sources - Update Sources/ from generated bindings"
	@echo ""
	@echo "Setup:"
	@echo "  install-deps  - Install required tools and targets"
