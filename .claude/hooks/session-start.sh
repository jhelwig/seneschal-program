#!/bin/bash
set -euo pipefail

# Only run in remote Claude Code environment
if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

echo "Installing project dependencies..."

# Install system dependencies for PDF processing
sudo apt-get update
sudo apt-get install -y libpoppler-glib-dev libqpdf-dev

# Install Rust 1.92.0 with required components
if ! command -v rustup &> /dev/null; then
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain 1.92.0
  source "$HOME/.cargo/env"
else
  rustup default 1.92.0
fi
rustup component add rustfmt clippy

# Install Just task runner
if ! command -v just &> /dev/null; then
  cargo install just
fi

# Install Node.js 20 if not present or wrong version
if ! command -v node &> /dev/null || [[ "$(node --version)" != v20* ]]; then
  curl -fsSL https://deb.nodesource.com/setup_20.x | sudo -E bash -
  sudo apt-get install -y nodejs
fi

# Install npm dependencies for fvtt-seneschal
cd "$CLAUDE_PROJECT_DIR/fvtt-seneschal"
npm install

# Download PDFium library
cd "$CLAUDE_PROJECT_DIR"
just download-pdfium

# Persist cargo environment for the session
echo 'source "$HOME/.cargo/env"' >> "$CLAUDE_ENV_FILE"

echo "Environment setup complete!"
