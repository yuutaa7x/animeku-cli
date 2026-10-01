#!/bin/bash
set -e

echo ""
echo "  ====================================================="
echo "    animeku-cli Auto Uninstaller for Linux / macOS"
echo "  ====================================================="
echo ""

if ! command -v cargo &> /dev/null; then
    echo "[ERROR] Rust/cargo not found. Cannot uninstall."
    exit 1
fi

echo "[1/1] Uninstalling animeku-cli via cargo..."
cargo uninstall animeku-cli

if [ $? -eq 0 ]; then
    echo ""
    echo "  ====================================================="
    echo "    SUCCESS! animeku-cli uninstalled!"
    echo "  ====================================================="
else
    echo ""
    echo "  [ERROR] Uninstallation failed. Check errors above."
fi
