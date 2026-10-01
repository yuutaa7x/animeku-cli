#!/bin/bash
set -e

echo ""
echo "  ====================================================="
echo "    animeku-cli Auto Installer for Linux / macOS"
echo "  ====================================================="
echo ""

# Check for Rust
if ! command -v cargo &> /dev/null; then
    echo "[1/3] Rust not found. Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
    echo "[OK] Rust installed."
else
    echo "[1/3] Rust already installed. Skipping."
fi
echo ""

# Check for MPV
if ! command -v mpv &> /dev/null; then
    echo "[2/3] mpv not found. Attempting to install..."
    if command -v apt &> /dev/null; then
        sudo apt update && sudo apt install -y mpv
    elif command -v pacman &> /dev/null; then
        sudo pacman -S --noconfirm mpv
    elif command -v dnf &> /dev/null; then
        sudo dnf install -y mpv
    elif command -v zypper &> /dev/null; then
        sudo zypper install -y mpv
    elif command -v brew &> /dev/null; then
        brew install mpv
    elif command -v pkg &> /dev/null && [ -n "$TERMUX_VERSION" ]; then
        pkg install -y mpv
    else
        echo "[WARNING] Could not automatically install mpv. Please install it manually from your package manager."
    fi
else
    echo "[2/3] mpv already installed. Skipping."
fi
echo ""

# Install animeku-cli
echo "[3/3] Installing animeku-cli..."
cargo install --git https://github.com/yuutaa7x/animeku-cli

if [ $? -eq 0 ]; then
    echo ""
    echo "  ====================================================="
    echo "    SUCCESS! animeku-cli installed!"
    echo "    Run it with: animeku-cli"
    echo "  ====================================================="
else
    echo ""
    echo "  [ERROR] Installation failed. Check errors above."
fi
