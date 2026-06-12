#!/bin/bash
# Build script for Personal Assistant
# Run on MacBook to cross-compile for Linux x86_64 (1037U)

set -euo pipefail

TARGET_HOST="${TARGET_HOST:-root@1037u}"  # Change to your 1037U's address
TARGET_DIR="/opt/personal-assistant"
BINARY_NAME="personal-assistant"

echo "=== Building Personal Assistant ==="

# Install cross-compilation tool if needed
if ! command -v cross &> /dev/null; then
    echo "Installing cross..."
    cargo install cross
fi

# Build for Linux x86_64
echo "Building for x86_64-unknown-linux-gnu..."
cross build --release --target x86_64-unknown-linux-gnu --package autoagents-server

# Verify binary
TARGET_BIN="target/x86_64-unknown-linux-gnu/release/${BINARY_NAME}"
if [ ! -f "$TARGET_BIN" ]; then
    echo "Error: Binary not found at $TARGET_BIN"
    exit 1
fi

echo "Binary size: $(du -h "$TARGET_BIN" | cut -f1)"

# Deploy to 1037U
echo "=== Deploying to ${TARGET_HOST} ==="

ssh "$TARGET_HOST" "sudo mkdir -p ${TARGET_DIR}/{audit,backups,custom_tools,data}"

scp "$TARGET_BIN" "${TARGET_HOST}:/tmp/${BINARY_NAME}"
ssh "$TARGET_HOST" "sudo mv /tmp/${BINARY_NAME} /usr/local/bin/${BINARY_NAME} && sudo chmod +x /usr/local/bin/${BINARY_NAME}"

# Deploy config if not exists
ssh "$TARGET_HOST" "if [ ! -f ${TARGET_DIR}/config.yaml ]; then sudo cp ${TARGET_DIR}/config.yaml 2>/dev/null || true; fi" || true
scp deploy/config.yaml "${TARGET_HOST}:/tmp/config.yaml"
ssh "$TARGET_HOST" "sudo cp /tmp/config.yaml ${TARGET_DIR}/config.yaml"

# Deploy and enable systemd service
scp deploy/personal-assistant.service "${TARGET_HOST}:/tmp/"
ssh "$TARGET_HOST" "sudo mv /tmp/personal-assistant.service /etc/systemd/system/ && sudo systemctl daemon-reload && sudo systemctl enable personal-assistant && sudo systemctl restart personal-assistant"

echo "=== Deployment complete ==="
ssh "$TARGET_HOST" "sudo systemctl status personal-assistant --no-pager"
