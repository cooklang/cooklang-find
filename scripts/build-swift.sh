#!/bin/bash
# Build Swift bindings for cooklang-find
# This script builds the library for Apple platforms and generates Swift bindings

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
# Use realpath to get canonical path with correct case on macOS
PROJECT_ROOT="$(realpath "$(dirname "$SCRIPT_DIR")")"

# Ensure we're working from the canonical path
cd "$PROJECT_ROOT"

OUTPUT_DIR="${PROJECT_ROOT}/bindings/swift"
XCFRAMEWORK_DIR="${OUTPUT_DIR}/CooklangFind.xcframework"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

log_info() {
    echo -e "${GREEN}[INFO]${NC} $1"
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

log_error() {
    echo -e "${RED}[ERROR]${NC} $1"
}

# Check for required tools
check_requirements() {
    log_info "Checking requirements..."

    if ! command -v cargo &> /dev/null; then
        log_error "cargo is not installed. Please install Rust."
        exit 1
    fi

    if ! command -v rustup &> /dev/null; then
        log_error "rustup is not installed. Please install rustup."
        exit 1
    fi

    # Check if we're on macOS for Apple targets
    if [[ "$OSTYPE" == "darwin"* ]]; then
        if ! command -v xcodebuild &> /dev/null; then
            log_warn "Xcode command line tools not found. XCFramework creation may fail."
        fi
    fi
}

# Install required Rust targets
install_targets() {
    log_info "Installing Rust targets for iOS..."

    # iOS only
    rustup target add aarch64-apple-ios 2>/dev/null || true
    rustup target add aarch64-apple-ios-sim 2>/dev/null || true
    rustup target add x86_64-apple-ios 2>/dev/null || true
}

# Build for a specific target
build_target() {
    local target=$1
    log_info "Building for target: $target"

    cd "$PROJECT_ROOT"
    cargo build --release --target "$target"
}

# Generate Swift bindings
generate_bindings() {
    log_info "Generating Swift bindings..."

    mkdir -p "$OUTPUT_DIR/Sources/CooklangFind"

    cd "$PROJECT_ROOT"

    # Find a built library to generate bindings from
    local lib_path=""
    for target in aarch64-apple-ios aarch64-apple-ios-sim x86_64-apple-ios; do
        local candidate="${PROJECT_ROOT}/target/${target}/release/libcooklang_find.dylib"
        if [[ -f "$candidate" ]]; then
            lib_path="$candidate"
            break
        fi
        candidate="${PROJECT_ROOT}/target/${target}/release/libcooklang_find.a"
        if [[ -f "$candidate" ]]; then
            lib_path="$candidate"
            break
        fi
    done

    # Fall back to host target
    if [[ -z "$lib_path" ]]; then
        cargo build --release
        if [[ "$OSTYPE" == "darwin"* ]]; then
            lib_path="${PROJECT_ROOT}/target/release/libcooklang_find.dylib"
        else
            lib_path="${PROJECT_ROOT}/target/release/libcooklang_find.so"
        fi
    fi

    cargo run --release --features cli --bin uniffi-bindgen -- generate \
        --library "$lib_path" \
        --language swift \
        --config "${PROJECT_ROOT}/uniffi.toml" \
        --out-dir "$OUTPUT_DIR/Sources/CooklangFind"

    log_info "Swift bindings generated at: $OUTPUT_DIR/Sources/CooklangFind"
}

# Create XCFramework (iOS only)
create_xcframework() {
    if [[ "$OSTYPE" != "darwin"* ]]; then
        log_warn "XCFramework creation is only supported on macOS"
        return
    fi

    log_info "Creating XCFramework..."

    rm -rf "$XCFRAMEWORK_DIR"
    mkdir -p "$OUTPUT_DIR/tmp"

    local framework_args=()

    # iOS device
    if [[ -f "${PROJECT_ROOT}/target/aarch64-apple-ios/release/libcooklang_find.a" ]]; then
        log_info "Adding iOS device library..."
        mkdir -p "$OUTPUT_DIR/tmp/ios-device/Headers"
        cp "${PROJECT_ROOT}/target/aarch64-apple-ios/release/libcooklang_find.a" "$OUTPUT_DIR/tmp/ios-device/"
        cp "$OUTPUT_DIR/Sources/CooklangFind/CooklangFindFFI.h" "$OUTPUT_DIR/tmp/ios-device/Headers/"
        cp "$OUTPUT_DIR/Sources/CooklangFind/CooklangFindFFI.modulemap" "$OUTPUT_DIR/tmp/ios-device/Headers/module.modulemap"

        framework_args+=(-library "$OUTPUT_DIR/tmp/ios-device/libcooklang_find.a" -headers "$OUTPUT_DIR/tmp/ios-device/Headers")
    fi

    # iOS simulator (universal)
    local sim_libs=()
    [[ -f "${PROJECT_ROOT}/target/aarch64-apple-ios-sim/release/libcooklang_find.a" ]] && \
        sim_libs+=("${PROJECT_ROOT}/target/aarch64-apple-ios-sim/release/libcooklang_find.a")
    [[ -f "${PROJECT_ROOT}/target/x86_64-apple-ios/release/libcooklang_find.a" ]] && \
        sim_libs+=("${PROJECT_ROOT}/target/x86_64-apple-ios/release/libcooklang_find.a")

    if [[ ${#sim_libs[@]} -gt 0 ]]; then
        log_info "Creating iOS simulator library..."
        mkdir -p "$OUTPUT_DIR/tmp/ios-sim/Headers"

        if [[ ${#sim_libs[@]} -gt 1 ]]; then
            lipo -create "${sim_libs[@]}" -output "$OUTPUT_DIR/tmp/ios-sim/libcooklang_find.a"
        else
            cp "${sim_libs[0]}" "$OUTPUT_DIR/tmp/ios-sim/libcooklang_find.a"
        fi

        cp "$OUTPUT_DIR/Sources/CooklangFind/CooklangFindFFI.h" "$OUTPUT_DIR/tmp/ios-sim/Headers/"
        cp "$OUTPUT_DIR/Sources/CooklangFind/CooklangFindFFI.modulemap" "$OUTPUT_DIR/tmp/ios-sim/Headers/module.modulemap"

        framework_args+=(-library "$OUTPUT_DIR/tmp/ios-sim/libcooklang_find.a" -headers "$OUTPUT_DIR/tmp/ios-sim/Headers")
    fi

    if [[ ${#framework_args[@]} -gt 0 ]]; then
        xcodebuild -create-xcframework "${framework_args[@]}" -output "$XCFRAMEWORK_DIR"
        log_info "XCFramework created at: $XCFRAMEWORK_DIR"
    else
        log_warn "No libraries found to create XCFramework"
    fi

    # Cleanup
    rm -rf "$OUTPUT_DIR/tmp"
}

# Generate Package.swift
generate_package_swift() {
    log_info "Generating Package.swift..."

    cat > "$OUTPUT_DIR/Package.swift" << 'EOF'
// swift-tools-version:5.5
import PackageDescription

let package = Package(
    name: "CooklangFind",
    platforms: [
        .iOS(.v13)
    ],
    products: [
        .library(
            name: "CooklangFind",
            targets: ["CooklangFind", "CooklangFindFFI"]
        ),
    ],
    targets: [
        .target(
            name: "CooklangFind",
            dependencies: ["CooklangFindFFI"],
            path: "Sources/CooklangFind",
            exclude: ["CooklangFindFFI.h", "CooklangFindFFI.modulemap"]
        ),
        .binaryTarget(
            name: "CooklangFindFFI",
            path: "CooklangFind.xcframework"
        ),
    ]
)
EOF

    log_info "Package.swift generated"
}

# Main build flow
main() {
    local build_all=false
    local generate_only=false

    while [[ $# -gt 0 ]]; do
        case $1 in
            --all)
                build_all=true
                shift
                ;;
            --generate-only)
                generate_only=true
                shift
                ;;
            --help|-h)
                echo "Usage: $0 [OPTIONS]"
                echo ""
                echo "Options:"
                echo "  --all            Build for all iOS platforms"
                echo "  --generate-only  Only generate Swift bindings (no compilation)"
                echo "  --help, -h       Show this help message"
                exit 0
                ;;
            *)
                log_error "Unknown option: $1"
                exit 1
                ;;
        esac
    done

    check_requirements

    if [[ "$generate_only" == true ]]; then
        generate_bindings
        exit 0
    fi

    if [[ "$build_all" == true ]]; then
        install_targets

        if [[ "$OSTYPE" == "darwin"* ]]; then
            build_target "aarch64-apple-ios"
            build_target "aarch64-apple-ios-sim"
            build_target "x86_64-apple-ios"
        else
            log_warn "Cross-compiling for iOS requires macOS"
            log_info "Building for host platform only..."
            cargo build --release
        fi
    else
        log_info "Building for host platform..."
        cargo build --release
    fi

    generate_bindings

    if [[ "$build_all" == true ]] && [[ "$OSTYPE" == "darwin"* ]]; then
        create_xcframework
        generate_package_swift
    fi

    log_info "Build complete!"
    log_info "Swift bindings are in: $OUTPUT_DIR"
}

main "$@"
