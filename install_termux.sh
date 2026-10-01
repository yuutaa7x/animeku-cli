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

echo "[2/4] Menambahkan X11 dan TUR Repository..."
pkg install x11-repo tur-repo -y

echo "[3/4] Menginstall Dependensi (Rust, MPV, Termux-X11, PulseAudio)..."
# termux-x11-nightly diperlukan sebagai server X11 untuk Termux
# mpv untuk video player
# pulseaudio untuk output suara
pkg install rust binutils mpv termux-x11-nightly pulseaudio -y

echo "[4/4] Build & Install Animeku-CLI..."
cargo install --path .

echo ""
echo "================================================="
echo "             INSTALASI SELESAI                   "
echo "================================================="
echo ""
echo "Cara memutar video di Android tanpa Desktop Environment:"
echo "1. Download dan Install aplikasi 'Termux-X11' dari Github jika belum (https://github.com/termux/termux-x11/releases/tag/nightly)"
echo "2. Buka aplikasi Termux-X11 di HP kamu (akan muncul layar kosong)"
echo "3. Kembali ke aplikasi Termux"
echo "4. Ketik perintah ini sebelum menjalankan animeku-cli (agar server X11 jalan):"
echo "   termux-x11 :0 &"
echo "   (atau cukup jalankan sekali saja per sesi termux)"
echo "5. Ketik: animeku-cli"
echo "   Saat anime diputar, MPV akan langsung muncul di jendela Termux-X11!"
echo ""
