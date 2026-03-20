#!/bin/sh
# Tina4 CLI installer — https://tina4.com
# Usage: curl -fsSL https://tina4.com/install.sh | sh
set -e

REPO="tina4stack/tina4"
INSTALL_DIR="/usr/local/bin"

# Detect OS and architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Darwin)  PLATFORM="darwin" ;;
  Linux)   PLATFORM="linux" ;;
  MINGW*|MSYS*|CYGWIN*) PLATFORM="windows" ;;
  *) echo "Unsupported OS: $OS"; exit 1 ;;
esac

case "$ARCH" in
  x86_64|amd64)  ARCH="amd64" ;;
  arm64|aarch64)  ARCH="arm64" ;;
  *) echo "Unsupported architecture: $ARCH"; exit 1 ;;
esac

if [ "$PLATFORM" = "windows" ]; then
  BINARY="tina4-windows-amd64.exe"
else
  BINARY="tina4-${PLATFORM}-${ARCH}"
fi

# Get latest release tag
LATEST=$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" | grep '"tag_name"' | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/')

if [ -z "$LATEST" ]; then
  echo "Error: Could not determine latest release"
  exit 1
fi

URL="https://github.com/${REPO}/releases/download/${LATEST}/${BINARY}"

echo "Installing tina4 ${LATEST} (${PLATFORM}/${ARCH})..."
echo "Downloading ${URL}"

TMP=$(mktemp)
curl -fsSL "$URL" -o "$TMP"
chmod +x "$TMP"

# Install — try without sudo first
if [ -w "$INSTALL_DIR" ]; then
  mv "$TMP" "${INSTALL_DIR}/tina4"
else
  echo "Need sudo to install to ${INSTALL_DIR}"
  sudo mv "$TMP" "${INSTALL_DIR}/tina4"
fi

echo "✓ tina4 installed to ${INSTALL_DIR}/tina4"
tina4 --version
