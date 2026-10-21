#!/usr/bin/env bash
set -u

REPO="yuutaa7x/animeku-cli"
RELEASE_BASE="https://github.com/${REPO}/releases/latest/download"
TMP_DIR="$(mktemp -d 2>/dev/null || true)"
if [ -z "${TMP_DIR:-}" ]; then
    TMP_DIR="${TMPDIR:-/tmp}/animeku-install-$$"
    mkdir -p "$TMP_DIR"
fi
PREBUILT=0

cleanup() {
    if [ -n "${TMP_DIR:-}" ] && [ -d "$TMP_DIR" ]; then
        rm -rf "$TMP_DIR"
    fi
}
trap cleanup EXIT

echo ""
echo "  ====================================================="
echo "    animeku-cli Auto Installer for Linux / macOS"
echo "  ====================================================="
echo ""

if [ -n "${ANIMEKU_INSTALL_DIR:-}" ]; then
    INSTALL_DIR="$ANIMEKU_INSTALL_DIR"
elif case ":$PATH:" in *":$HOME/.local/bin:"*) true ;; *) false ;; esac; then
    INSTALL_DIR="$HOME/.local/bin"
else
    INSTALL_DIR="$HOME/.cargo/bin"
fi

asset_name() {
    if [ -n "${TERMUX_VERSION:-}" ]; then
        return 1
    fi

    case "$(uname -s)" in
        Linux) os="linux" ;;
        Darwin) os="macos" ;;
        *) return 1 ;;
    esac

    case "$(uname -m)" in
        x86_64|amd64) arch="x86_64" ;;
        arm64|aarch64) arch="aarch64" ;;
        *) return 1 ;;
    esac

    printf 'animeku-cli-%s-%s' "$os" "$arch"
}

verify_sha256() {
    file="$1"
    checksum_file="$2"
    expected="$(awk 'NR == 1 { print tolower($1) }' "$checksum_file")"
    if [ -z "$expected" ]; then
        return 1
    fi

    if command -v sha256sum >/dev/null 2>&1; then
        actual="$(sha256sum "$file" | awk '{ print tolower($1) }')"
    elif command -v shasum >/dev/null 2>&1; then
        actual="$(shasum -a 256 "$file" | awk '{ print tolower($1) }')"
    else
        return 1
    fi

    [ "$expected" = "$actual" ]
}

install_prebuilt() {
    if ! command -v curl >/dev/null 2>&1; then
        return 1
    fi

    asset="$(asset_name)" || return 1
    echo "[1/4] Trying prebuilt binary: $asset"

    if ! curl -fL --connect-timeout 15 -# -o "$TMP_DIR/$asset" "$RELEASE_BASE/$asset"; then
        echo "[INFO] Prebuilt binary is not available for this platform."
        return 1
    fi

    if ! curl -fsSL --connect-timeout 15 -o "$TMP_DIR/$asset.sha256" "$RELEASE_BASE/$asset.sha256"; then
        echo "[WARN] Checksum is not available; falling back to source build."
        return 1
    fi

    if ! verify_sha256 "$TMP_DIR/$asset" "$TMP_DIR/$asset.sha256"; then
        echo "[WARN] Checksum mismatch; falling back to source build."
        return 1
    fi

    chmod +x "$TMP_DIR/$asset"
    if ! "$TMP_DIR/$asset" --version >/dev/null 2>&1; then
        echo "[WARN] Prebuilt binary cannot run on this system; falling back to source build."
        return 1
    fi

    mkdir -p "$INSTALL_DIR" || return 1
    if ! install -m 755 "$TMP_DIR/$asset" "$INSTALL_DIR/animeku-cli"; then
        return 1
    fi
    cp -f "$INSTALL_DIR/animeku-cli" "$INSTALL_DIR/animeku" 2>/dev/null || true
    echo "[OK] Prebuilt binary installed to $INSTALL_DIR/animeku-cli"
    return 0
}

install_dependencies() {
    if ! command -v mpv >/dev/null 2>&1; then
        echo "[2/4] mpv not found. Attempting to install..."
        if command -v apt >/dev/null 2>&1; then
            sudo apt update && sudo apt install -y mpv
        elif command -v pacman >/dev/null 2>&1; then
            sudo pacman -S --noconfirm mpv
        elif command -v dnf >/dev/null 2>&1; then
            sudo dnf install -y mpv
        elif command -v zypper >/dev/null 2>&1; then
            sudo zypper install -y mpv
        elif command -v brew >/dev/null 2>&1; then
            brew install mpv
        elif command -v pkg >/dev/null 2>&1 && [ -n "${TERMUX_VERSION:-}" ]; then
            pkg install -y mpv
        else
            echo "[WARNING] Could not automatically install mpv. Please install it manually."
        fi
    else
        echo "[2/4] mpv already installed. Skipping."
    fi
    echo ""

    if ! command -v ffmpeg >/dev/null 2>&1; then
        echo "[3/4] ffmpeg not found. Attempting to install..."
        if command -v apt >/dev/null 2>&1; then
            sudo apt update && sudo apt install -y ffmpeg
        elif command -v pacman >/dev/null 2>&1; then
            sudo pacman -S --noconfirm ffmpeg
        elif command -v dnf >/dev/null 2>&1; then
            sudo dnf install -y ffmpeg
        elif command -v zypper >/dev/null 2>&1; then
            sudo zypper install -y ffmpeg
        elif command -v brew >/dev/null 2>&1; then
            brew install ffmpeg
        elif command -v pkg >/dev/null 2>&1 && [ -n "${TERMUX_VERSION:-}" ]; then
            pkg install -y ffmpeg
        else
            echo "[WARNING] Could not automatically install ffmpeg. Download will stay disabled."
        fi
    else
        echo "[3/4] ffmpeg already installed. Skipping."
    fi
    echo ""
}

ensure_rust() {
    if command -v cargo >/dev/null 2>&1; then
        echo "[1/4] Rust already installed. Skipping."
        return 0
    fi

    if [ -n "${TERMUX_VERSION:-}" ] && command -v pkg >/dev/null 2>&1; then
        echo "[1/4] Installing Rust toolchain via Termux pkg..."
        pkg install -y rust binutils pkg-config clang make
        return $?
    fi

    echo "[1/4] Rust not found. Installing Rust..."
    if ! command -v curl >/dev/null 2>&1; then
        echo "[ERROR] curl is required to install Rust."
        return 1
    fi
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    # shellcheck disable=SC1091
    . "$HOME/.cargo/env"
    echo "[OK] Rust installed."
}

add_to_path() {
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) return 0 ;;
    esac

    for rc in "$HOME/.profile" "$HOME/.bashrc" "$HOME/.zshrc"; do
        [ -f "$rc" ] || continue
        if ! grep -qF "$INSTALL_DIR" "$rc"; then
            printf '\n# Added by animeku-cli installer\nexport PATH="%s:$PATH"\n' "$INSTALL_DIR" >> "$rc"
        fi
    done
    export PATH="$INSTALL_DIR:$PATH"
}

if install_prebuilt; then
    PREBUILT=1
else
    echo "[INFO] Falling back to building from source."
    echo ""
    if ! ensure_rust; then
        echo "[ERROR] Rust installation failed."
        exit 1
    fi
fi

install_dependencies

if [ "$PREBUILT" -eq 0 ]; then
    echo "[4/4] Installing animeku-cli from source..."
    if ! cargo install --git "https://github.com/${REPO}" --locked --force; then
        echo ""
        echo "  [ERROR] Installation failed. Check errors above."
        exit 1
    fi
    INSTALL_DIR="$HOME/.cargo/bin"
    cp -f "$INSTALL_DIR/animeku-cli" "$INSTALL_DIR/animeku" 2>/dev/null || true
fi

add_to_path

echo ""
echo "  ====================================================="
echo "    SUCCESS - animeku-cli installed"
echo "    Location: $INSTALL_DIR/animeku-cli"
echo "    Run it with: animeku or animeku-cli"
echo "  ====================================================="
echo ""
if ! command -v animeku-cli >/dev/null 2>&1; then
    echo "[INFO] Restart your shell or run: export PATH=\"$INSTALL_DIR:\$PATH\""
fi
