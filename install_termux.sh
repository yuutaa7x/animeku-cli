#!/bin/bash
# Script Instalasi Animeku-CLI untuk Termux (Android)
# Auto setup mpv + termux-x11 untuk nonton anime

echo "================================================="
echo "   Animeku-CLI Termux Setup (X11 Video Output)   "
echo "================================================="
echo ""

echo "[1/4] Update paket Termux..."
pkg update -y
pkg upgrade -y

echo "[2/4] Menginstall Dependensi (Rust)..."
# Termux di-setting untuk memakai Native Android Video Player (VLC / MX Player)
# pkg-config dan openssl dibutuhkan untuk mengompilasi library reqwest
pkg install rust binutils pkg-config openssl -y

echo "[3/4] Build & Install Animeku-CLI..."
cargo install --git https://github.com/yuutaa7x/animeku-cli

echo "Mengatur PATH agar animeku-cli bisa langsung dijalankan..."
if [[ ":$PATH:" != *":$HOME/.cargo/bin:"* ]]; then
    echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.bashrc
    echo 'export PATH="$HOME/.cargo/bin:$PATH"' >> ~/.zshrc
    export PATH="$HOME/.cargo/bin:$PATH"
fi

echo ""
echo "================================================="
echo "             INSTALASI SELESAI                   "
echo "================================================="
echo ""
echo "Cara memutar video di Android:"
echo "1. Pastikan kamu sudah menginstall VLC for Android dari Play Store"
echo "2. Ketik: animeku-cli"
echo "   Saat anime diputar, pilih 'Putar di Pemutar Android (VLC/MX)'"
echo ""
