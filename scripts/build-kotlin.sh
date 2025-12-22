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

# Setup cargo config for Android NDK
setup_cargo_config() {
    local ndk_home="${ANDROID_NDK_HOME:-$NDK_HOME}"
    local config_dir="${PROJECT_ROOT}/.cargo"
    local config_file="${config_dir}/config.toml"

    mkdir -p "$config_dir"

    # Detect host OS
    local host_os
    case "$OSTYPE" in
        linux*)   host_os="linux" ;;
        darwin*)  host_os="darwin" ;;
        *)        host_os="linux" ;;
    esac

    # Find the NDK toolchain
    local toolchain_dir="${ndk_home}/toolchains/llvm/prebuilt/${host_os}-x86_64"
    if [[ ! -d "$toolchain_dir" ]]; then
        log_error "NDK toolchain not found at: $toolchain_dir"
        return 1
    fi

    local min_api=21

    cat > "$config_file" << EOF
# Auto-generated cargo config for Android NDK
# NDK Path: $ndk_home

[target.aarch64-linux-android]
ar = "${toolchain_dir}/bin/llvm-ar"
linker = "${toolchain_dir}/bin/aarch64-linux-android${min_api}-clang"

[target.armv7-linux-androideabi]
ar = "${toolchain_dir}/bin/llvm-ar"
linker = "${toolchain_dir}/bin/armv7a-linux-androideabi${min_api}-clang"

[target.x86_64-linux-android]
ar = "${toolchain_dir}/bin/llvm-ar"
linker = "${toolchain_dir}/bin/x86_64-linux-android${min_api}-clang"

[target.i686-linux-android]
ar = "${toolchain_dir}/bin/llvm-ar"
linker = "${toolchain_dir}/bin/i686-linux-android${min_api}-clang"
EOF

    log_info "Cargo config written to: $config_file"
}

# Build for a specific target
build_target() {
    local target=$1
    log_info "Building for target: $target"

    cd "$PROJECT_ROOT"
    cargo build --release --target "$target"
}

# Generate Kotlin bindings
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
            break
        fi
    done

    # Fall back to host target
    if [[ -z "$lib_path" ]]; then
        cargo build --release
        lib_path="${PROJECT_ROOT}/target/release/libcooklang_find.so"
        if [[ ! -f "$lib_path" ]]; then
            lib_path="${PROJECT_ROOT}/target/release/libcooklang_find.dylib"
        fi
    fi

    cargo run --release --features cli --bin uniffi-bindgen -- generate \
        --library "$lib_path" \
        --language kotlin \
        --config "${PROJECT_ROOT}/uniffi.toml" \
        --out-dir "$OUTPUT_DIR"

    log_info "Kotlin bindings generated at: $OUTPUT_DIR"
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

    if [[ -f "${PROJECT_ROOT}/target/aarch64-linux-android/release/libcooklang_find.so" ]]; then
        cp "${PROJECT_ROOT}/target/aarch64-linux-android/release/libcooklang_find.so" "$jni_dir/arm64-v8a/"
        copied=$((copied + 1))
    fi

    if [[ -f "${PROJECT_ROOT}/target/armv7-linux-androideabi/release/libcooklang_find.so" ]]; then
        cp "${PROJECT_ROOT}/target/armv7-linux-androideabi/release/libcooklang_find.so" "$jni_dir/armeabi-v7a/"
        copied=$((copied + 1))
    fi

    if [[ -f "${PROJECT_ROOT}/target/x86_64-linux-android/release/libcooklang_find.so" ]]; then
        cp "${PROJECT_ROOT}/target/x86_64-linux-android/release/libcooklang_find.so" "$jni_dir/x86_64/"
        copied=$((copied + 1))
    fi

    if [[ -f "${PROJECT_ROOT}/target/i686-linux-android/release/libcooklang_find.so" ]]; then
        cp "${PROJECT_ROOT}/target/i686-linux-android/release/libcooklang_find.so" "$jni_dir/x86/"
        copied=$((copied + 1))
    fi

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
                echo "  --all            Build for all Android architectures"
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

    if [[ "$generate_only" == true ]]; then
        generate_bindings
        exit 0
    fi

    if [[ "$build_all" == true ]]; then
        if check_android_ndk; then
            install_targets
            setup_cargo_config

            for target in "${ANDROID_TARGETS[@]}"; do
                build_target "$target"
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
