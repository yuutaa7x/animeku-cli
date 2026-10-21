#!/bin/bash
set -u

echo ""
echo "  ====================================================="
echo "    animeku-cli Auto Uninstaller for Linux / macOS"
echo "  ====================================================="
echo ""

REMOVED=0

if command -v cargo >/dev/null 2>&1; then
    if cargo uninstall animeku-cli >/dev/null 2>&1; then
        REMOVED=1
    fi
fi

for dir in "$HOME/.local/bin" "$HOME/.cargo/bin"; do
    for name in animeku-cli animeku; do
        if [ -e "$dir/$name" ]; then
            rm -f "$dir/$name" 2>/dev/null && REMOVED=1
        fi
    done
done

echo ""
if [ "$REMOVED" -eq 1 ]; then
    echo "  ====================================================="
    echo "    SUCCESS! animeku-cli uninstalled!"
    echo "  ====================================================="
else
    echo "  [WARN] animeku-cli was not found or could not be removed."
    echo "         Close any running animeku-cli and try again."
fi
