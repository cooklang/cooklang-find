#!/bin/bash
# Build Kotlin/Android bindings for cooklang-find
# This script builds the library for Android and generates Kotlin bindings

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
OUTPUT_DIR="${PROJECT_ROOT}/bindings/kotlin"

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

# Android targets
ANDROID_TARGETS=(
    "aarch64-linux-android"
    "armv7-linux-androideabi"
    "x86_64-linux-android"
    "i686-linux-android"
)

# Get ABI name for a target
get_abi_for_target() {
    local target=$1
    case "$target" in
        aarch64-linux-android) echo "arm64-v8a" ;;
        armv7-linux-androideabi) echo "armeabi-v7a" ;;
        x86_64-linux-android) echo "x86_64" ;;
        i686-linux-android) echo "x86" ;;
        *) echo "" ;;
    esac
}

MIN_API=21

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
}

# Check and install cargo-ndk if needed
check_cargo_ndk() {
    if ! command -v cargo-ndk &> /dev/null; then
        log_info "Installing cargo-ndk..."
        cargo install cargo-ndk --locked
    else
        log_info "cargo-ndk is already installed"
    fi
}

# Check Android NDK
check_android_ndk() {
    if [[ -z "$ANDROID_NDK_HOME" ]] && [[ -z "$NDK_HOME" ]]; then
        log_warn "ANDROID_NDK_HOME or NDK_HOME not set."
        log_warn "Android cross-compilation will be skipped."
        return 1
    fi

    local ndk_home="${ANDROID_NDK_HOME:-$NDK_HOME}"
    if [[ ! -d "$ndk_home" ]]; then
        log_warn "Android NDK directory not found: $ndk_home"
        return 1
    fi

    log_info "Using Android NDK: $ndk_home"
    return 0
}

# Install required Rust targets
install_targets() {
    log_info "Installing Rust targets for Android..."

    for target in "${ANDROID_TARGETS[@]}"; do
        rustup target add "$target" 2>/dev/null || true
    done
}

# Build uniffi-bindgen for the host platform first
build_uniffi_bindgen() {
    log_info "Building uniffi-bindgen for host platform..."
    cd "$PROJECT_ROOT"
    cargo build --features cli --bin uniffi-bindgen --release
}

# Build for a specific Android target using cargo-ndk
build_android_target() {
    local target=$1
    log_info "Building for target: $target"

    cd "$PROJECT_ROOT"
    cargo ndk --target "$target" --platform "$MIN_API" build --release
}

# Generate Kotlin bindings using pre-built uniffi-bindgen
generate_bindings() {
    log_info "Generating Kotlin bindings..."

    mkdir -p "$OUTPUT_DIR"

    cd "$PROJECT_ROOT"

    # Find a built library to generate bindings from
    local lib_path=""

    # Try Android targets first
    for target in "${ANDROID_TARGETS[@]}"; do
        local candidate="${PROJECT_ROOT}/target/${target}/release/libcooklang_find.so"
        if [[ -f "$candidate" ]]; then
            lib_path="$candidate"
            log_info "Using library: $lib_path"
            break
        fi
    done

    # Fall back to host target
    if [[ -z "$lib_path" ]]; then
        log_info "No Android library found, building for host..."
        cargo build --release
        lib_path="${PROJECT_ROOT}/target/release/libcooklang_find.so"
        if [[ ! -f "$lib_path" ]]; then
            lib_path="${PROJECT_ROOT}/target/release/libcooklang_find.dylib"
        fi
        if [[ ! -f "$lib_path" ]]; then
            log_error "Could not find built library"
            exit 1
        fi
    fi

    # Use the pre-built uniffi-bindgen binary
    local bindgen="${PROJECT_ROOT}/target/release/uniffi-bindgen"
    if [[ ! -f "$bindgen" ]]; then
        log_info "uniffi-bindgen not found, building it..."
        build_uniffi_bindgen
    fi

    log_info "Running uniffi-bindgen..."
    "$bindgen" generate \
        --library "$lib_path" \
        --language kotlin \
        --config "${PROJECT_ROOT}/uniffi.toml" \
        --out-dir "$OUTPUT_DIR"

    # Verify and show what was generated
    log_info "Kotlin bindings generated at: $OUTPUT_DIR"
    log_info "Generated files:"
    find "$OUTPUT_DIR" -name "*.kt" -type f

    # Verify the expected structure exists
    if [[ ! -d "$OUTPUT_DIR/org" ]]; then
        log_error "Expected directory structure not created: $OUTPUT_DIR/org"
        log_error "Contents of $OUTPUT_DIR:"
        ls -la "$OUTPUT_DIR"
        exit 1
    fi
}

# Create Android library structure
create_android_lib() {
    log_info "Creating Android library structure..."

    local android_dir="${OUTPUT_DIR}/android"
    local jni_dir="${android_dir}/src/main/jniLibs"
    local kotlin_dir="${android_dir}/src/main/kotlin"

    mkdir -p "$jni_dir"/{arm64-v8a,armeabi-v7a,x86_64,x86}
    mkdir -p "$kotlin_dir"

    # Copy native libraries
    local copied=0

    for target in "${ANDROID_TARGETS[@]}"; do
        local abi
        abi=$(get_abi_for_target "$target")
        local so_file="${PROJECT_ROOT}/target/${target}/release/libcooklang_find.so"
        if [[ -f "$so_file" ]]; then
            cp "$so_file" "$jni_dir/$abi/"
            copied=$((copied + 1))
        fi
    done

    if [[ $copied -eq 0 ]]; then
        log_warn "No Android native libraries found to copy"
    else
        log_info "Copied $copied native libraries"
    fi

    # Copy Kotlin sources (org/cooklang/find/ structure from uniffi.toml package_name)
    cp -r "$OUTPUT_DIR/org" "$kotlin_dir/"

    # Create build.gradle.kts
    cat > "${android_dir}/build.gradle.kts" << 'EOF'
plugins {
    id("com.android.library")
    id("org.jetbrains.kotlin.android")
    id("maven-publish")
}

android {
    namespace = "org.cooklang.find"
    compileSdk = 34

    defaultConfig {
        minSdk = 21

        consumerProguardFiles("consumer-rules.pro")
    }

    buildTypes {
        release {
            isMinifyEnabled = false
            proguardFiles(
                getDefaultProguardFile("proguard-android-optimize.txt"),
                "proguard-rules.pro"
            )
        }
    }

    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_1_8
        targetCompatibility = JavaVersion.VERSION_1_8
    }

    kotlinOptions {
        jvmTarget = "1.8"
    }

    sourceSets {
        getByName("main") {
            jniLibs.srcDirs("src/main/jniLibs")
        }
    }
}

dependencies {
    implementation("net.java.dev.jna:jna:5.13.0@aar")
    implementation("org.jetbrains.kotlinx:kotlinx-coroutines-core:1.7.3")
}

publishing {
    publications {
        register<MavenPublication>("release") {
            groupId = "org.cooklang"
            artifactId = "cooklang-find"
            version = project.findProperty("version")?.toString() ?: "0.5.0"

            afterEvaluate {
                from(components["release"])
            }

            pom {
                name.set("CooklangFind")
                description.set("Library for finding and managing Cooklang recipes")
                url.set("https://github.com/cooklang/cooklang-find")

                licenses {
                    license {
                        name.set("MIT License")
                        url.set("https://opensource.org/licenses/MIT")
                    }
                }

                developers {
                    developer {
                        id.set("cooklang")
                        name.set("Cooklang")
                    }
                }

                scm {
                    connection.set("scm:git:git://github.com/cooklang/cooklang-find.git")
                    developerConnection.set("scm:git:ssh://github.com/cooklang/cooklang-find.git")
                    url.set("https://github.com/cooklang/cooklang-find")
                }
            }
        }
    }
}
EOF

    # Create proguard rules
    cat > "${android_dir}/proguard-rules.pro" << 'EOF'
# Keep JNA classes
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }

# Keep generated classes
-keep class org.cooklang.find.** { *; }
EOF

    cat > "${android_dir}/consumer-rules.pro" << 'EOF'
# Keep JNA classes for consumers
-keep class com.sun.jna.** { *; }
-keep class * implements com.sun.jna.** { *; }
-keep class org.cooklang.find.** { *; }
EOF

    # Create gradle.properties
    cat > "${android_dir}/gradle.properties" << 'EOF'
android.useAndroidX=true
kotlin.code.style=official
EOF

    # Create settings.gradle.kts
    cat > "${android_dir}/settings.gradle.kts" << 'EOF'
pluginManagement {
    repositories {
        google()
        mavenCentral()
        gradlePluginPortal()
    }
}

dependencyResolutionManagement {
    repositoriesMode.set(RepositoriesMode.FAIL_ON_PROJECT_REPOS)
    repositories {
        google()
        mavenCentral()
    }
}

rootProject.name = "cooklang-find"
EOF

    log_info "Android library structure created at: $android_dir"
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
                echo "  --all            Build for all Android architectures (requires cargo-ndk)"
                echo "  --generate-only  Only generate Kotlin bindings (no compilation)"
                echo "  --help, -h       Show this help message"
                echo ""
                echo "Environment Variables:"
                echo "  ANDROID_NDK_HOME  Path to Android NDK (required for --all)"
                echo "  NDK_HOME          Alternative to ANDROID_NDK_HOME"
                exit 0
                ;;
            *)
                log_error "Unknown option: $1"
                exit 1
                ;;
        esac
    done

    check_requirements

    # Always build uniffi-bindgen first (before any cross-compilation)
    build_uniffi_bindgen

    if [[ "$generate_only" == true ]]; then
        generate_bindings
        exit 0
    fi

    if [[ "$build_all" == true ]]; then
        if check_android_ndk; then
            check_cargo_ndk
            install_targets

            for target in "${ANDROID_TARGETS[@]}"; do
                build_android_target "$target"
            done
        else
            log_info "Building for host platform only..."
            cargo build --release
        fi
    else
        log_info "Building for host platform..."
        cargo build --release
    fi

    generate_bindings

    if [[ "$build_all" == true ]]; then
        create_android_lib
    fi

    log_info "Build complete!"
    log_info "Kotlin bindings are in: $OUTPUT_DIR"
}

main "$@"
