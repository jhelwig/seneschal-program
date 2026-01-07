# Seneschal Program build tasks

# PDFium configuration
pdfium_version := "7616"
pdfium_dir := "vendor/pdfium"

# Detect platform
arch := arch()
os := os()

# Map to pdfium naming convention
pdfium_platform := if os == "linux" {
    if arch == "x86_64" { "linux-x64" } else if arch == "aarch64" { "linux-arm64" } else { error("Unsupported architecture") }
} else if os == "macos" {
    if arch == "x86_64" { "mac-x64" } else if arch == "aarch64" { "mac-arm64" } else { error("Unsupported architecture") }
} else if os == "windows" {
    if arch == "x86_64" { "win-x64" } else if arch == "aarch64" { "win-arm64" } else { error("Unsupported architecture") }
} else {
    error("Unsupported OS")
}

# Library name varies by platform
pdfium_lib := if os == "linux" {
    "libpdfium.so"
} else if os == "macos" {
    "libpdfium.dylib"
} else if os == "windows" {
    "pdfium.dll"
} else {
    error("Unsupported OS")
}

# Download PDFium dynamic library
download-pdfium:
    #!/usr/bin/env bash
    set -euo pipefail

    PDFIUM_URL="https://github.com/bblanchon/pdfium-binaries/releases/download/chromium/{{pdfium_version}}/pdfium-{{pdfium_platform}}.tgz"
    DEST_DIR="{{pdfium_dir}}"

    if [ -f "$DEST_DIR/lib/{{pdfium_lib}}" ]; then
        echo "PDFium already downloaded at $DEST_DIR"
        exit 0
    fi

    echo "Downloading PDFium from $PDFIUM_URL..."
    mkdir -p "$DEST_DIR"
    curl -fsSL "$PDFIUM_URL" | tar -xzf - -C "$DEST_DIR"
    echo "PDFium extracted to $DEST_DIR"

# Build the project (downloads PDFium if needed)
build: download-pdfium
    cargo build

# Build in release mode
build-release: download-pdfium
    cargo build --release

# Run tests
test: download-pdfium
    cargo test

# Run the service (with library path set)
run: download-pdfium
    LD_LIBRARY_PATH="$(pwd)/{{pdfium_dir}}/lib:${LD_LIBRARY_PATH:-}" cargo run

# Run the service in release mode
run-release: download-pdfium
    LD_LIBRARY_PATH="$(pwd)/{{pdfium_dir}}/lib:${LD_LIBRARY_PATH:-}" cargo run --release

# Clean build artifacts (keeps downloaded PDFium)
clean:
    cargo clean

# Clean everything including downloaded PDFium
clean-all: clean
    rm -rf {{pdfium_dir}}

# Check code without building
check:
    cargo check

# Format code
fmt:
    cargo fmt

# Run clippy
clippy:
    cargo clippy
