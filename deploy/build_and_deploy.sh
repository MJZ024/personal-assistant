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

# Ensure a dedicated unprivileged system account + data dir exist. The service
# runs as this user (see personal-assistant.service), never as root.
ssh "$TARGET_HOST" "set -e; \
    sudo useradd --system --no-create-home --shell /usr/sbin/nologin personal-assistant 2>/dev/null || true; \
    sudo mkdir -p ${TARGET_DIR}/{audit,backups,custom_tools,data}; \
    sudo chown -R personal-assistant:personal-assistant ${TARGET_DIR}"

scp "$TARGET_BIN" "${TARGET_HOST}:/tmp/${BINARY_NAME}"
ssh "$TARGET_HOST" "sudo install -m 0755 -o root -g root /tmp/${BINARY_NAME} /usr/local/bin/${BINARY_NAME}"

# Ship the *example* config and create a real config.yaml ONLY if one does not
# already exist — never overwrite a config that holds live secrets.
scp deploy/config.example.yaml "${TARGET_HOST}:/tmp/config.example.yaml"
ssh "$TARGET_HOST" "set -e; \
    sudo install -m 0644 -o personal-assistant -g personal-assistant /tmp/config.example.yaml ${TARGET_DIR}/config.example.yaml; \
    if [ ! -f ${TARGET_DIR}/config.yaml ]; then \
        echo 'No config.yaml found — seeding from example. EDIT IT before relying on it.'; \
        sudo install -m 0600 -o personal-assistant -g personal-assistant /tmp/config.example.yaml ${TARGET_DIR}/config.yaml; \
    else \
        sudo chown personal-assistant:personal-assistant ${TARGET_DIR}/config.yaml; \
        sudo chmod 600 ${TARGET_DIR}/config.yaml; \
    fi"

# Deploy and enable systemd service
scp deploy/personal-assistant.service "${TARGET_HOST}:/tmp/"
ssh "$TARGET_HOST" "sudo mv /tmp/personal-assistant.service /etc/systemd/system/ && sudo systemctl daemon-reload && sudo systemctl enable personal-assistant && sudo systemctl restart personal-assistant"

echo "=== Deployment complete ==="
ssh "$TARGET_HOST" "sudo systemctl status personal-assistant --no-pager"
