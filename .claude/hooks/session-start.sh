#!/bin/bash
set -euo pipefail

# Only run in remote Claude Code environment
if [ "${CLAUDE_CODE_REMOTE:-}" != "true" ]; then
  exit 0
fi

# Run asynchronously (5 minute timeout)
echo '{"async": true, "asyncTimeout": 300000}'

echo "Installing project dependencies..."

# Helper to run commands with sudo only if not root
run_privileged() {
  if [ "$(id -u)" -eq 0 ]; then
    "$@"
  else
    sudo "$@"
  fi
}

# Fix /tmp permissions if needed (required for apt in some environments)
if [ ! -w /tmp ]; then
  run_privileged chmod 1777 /tmp
fi

# Install system dependencies for PDF processing
run_privileged apt-get update
run_privileged apt-get install -y libpoppler-glib-dev libqpdf-dev

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

# Node.js 20+ is required - install if missing or outdated
node_major=$(node --version 2>/dev/null | sed 's/v\([0-9]*\).*/\1/' || echo "0")
if [ "$node_major" -lt 20 ]; then
  curl -fsSL https://deb.nodesource.com/setup_20.x | run_privileged bash -
  run_privileged apt-get install -y nodejs
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
