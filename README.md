<div align="center">
  
![thanks ai hehe](https://i.ibb.co/yqjnrCP/8-WOp-J0-XSca-EN5krf-NNXb-F-transformed.png)

Streaming anime dan film Asia dengan subtitle Indonesia lewat terminal.

*Fork dari [lucasbuilds/animeku-cli](https://github.com/lucasbuilds/animeku-cli)* | Vibecoded BTW (sorry for this bad things)

</div>

### Tutorial Penggunaan

<video width="100%" controls>
  <source src="assets/tutorial.mp4" type="video/mp4">
</video>

### instalasi
> [!NOTE]
> Pastikan kamu sudah menginstall Rust dan pemutar media (**mpv** sangat disarankan), jika belum silahkan klik [tautan berikut](https://rustup.rs/).
> 
> - **Windows**: Install MPV dengan mudah melalui terminal (PowerShell / CMD):
>   ```powershell
>   winget install --id shinchiro.mpv
>   ```
> - **Linux**: `sudo apt install mpv` (Ubuntu/Debian) atau `sudo pacman -S mpv` (Arch).
> - **Android (Termux)**: `pkg install rust mpv`.

#### Auto Install (Windows — satu baris)

Buka **PowerShell** atau **CMD** lalu jalankan perintah berikut:

```powershell
powershell -c "irm https://raw.githubusercontent.com/yuutaa7x/animeku-cli/refs/heads/main/install.bat -OutFile install.bat; .\install.bat"
```

Script akan otomatis menginstall Rust, GCC (via MSYS2), dan MPV jika belum ada, lalu build dan install animeku-cli.

#### Manual Install (semua platform)

*Copy paste* perintah berikut:
```bash
cargo install --git https://github.com/yuutaa7x/animeku-cli
```

Silahkan tunggu proses penginstallan selesai dan jika sudah, ketik `animeku-cli` untuk menjalankannya.

### uninstall

#### Auto Uninstall (Windows — satu baris)

Buka **PowerShell** atau **CMD** lalu jalankan perintah berikut:

```powershell
powershell -c "irm https://raw.githubusercontent.com/yuutaa7x/animeku-cli/refs/heads/main/uninstall.bat -OutFile uninstall.bat; .\uninstall.bat"
```

#### Manual Uninstall (semua platform)

```bash
cargo uninstall animeku-cli
```


### Keunggulan & Fitur
- Sangat mudah digunakan langsung dari command line.
- Menonton Anime, Live Action, Film, dan TV Series secara gratis tanpa iklan.
- Hasil pencarian lengkap dengan pilihan server & resolusi (1080p, 720p, 480p, 360p).
- Otomatis memuat Subtitle Indonesia langsung ke player.
- Terintegrasi dengan Discord Rich Presence (RPC).
- Fitur Riwayat Tontonan & Resume Otomatis (melanjutkan episode dan menit/detik terakhir yang ditonton).

### Changelog (Perbedaan dari Upstream)
Dibandingkan dengan repo upstream (`lucasbuilds/animeku-cli`), fork ini membawa sejumlah perbaikan besar:

#### 1. Menu & Riwayat Tontonan (Resume Playback)
- **Menu Utama & Submenu Terstruktur**:
  - Menu utama: `1. Update Animeku-CLI`, `2. Go Watch Anime / Movie`, dan `3. Exit`.
  - Submenu menonton: `1. Watch Any Anime / Movie` (pencarian biasa) dan `2. Watch Last Anime / Movie` (riwayat tontonan).
- **Auto-Tracking & Resume Playback**:
  - Otomatis melacak dan menyimpan posisi timestamp tontonan secara presisi melalui integrasi script MPV.
  - Memilih `Watch Last Anime / Movie` menampilkan judul, episode, dan posisi menit terakhir yang ditonton, serta opsi untuk langsung melanjutkan pemutaran (*resume*) atau memutar ulang dari awal.

#### 2. Provider Streaming
- **Idlix (Baru & Dioptimasi)**:
  - Otomatis melakukan bypass proteksi gateway sesi Idlix.
  - Multi-resolusi HLS adaptif: resolusi diurutkan dari kualitas tertinggi dan tidak duplikat.
  - Penanganan audio terpisah (`#EXT-X-MEDIA:TYPE=AUDIO`) dan video fMP4 langsung tersinkronisasi.
  - Otomatis mengunduh subtitle Bahasa Indonesia (`.vtt`) dan dipasangkan ke MPV/VLC.
  - Optimasi koneksi segmen CDN (`http_persistent=0`) untuk menghilangkan lag/stutter dan spam error.
- **Otakudesu**:
  - Penyegaran parser untuk streaming anime Sub Indo yang stabil.
- **Pembersihan Provider Usang**:
  - Menghapus provider mati (`nontonanime` dan `tenflix`) yang sudah tidak berfungsi di upstream.

#### 3. Discord Rich Presence (RPC)
- Integrasi Discord RPC real-time yang bersih:
  - Membersihkan format judul (menghapus tag `[Movie]`, `Subtitle Indonesia`, dan kurung kosong).
  - Menampilkan status dinamis (*Full Movie* untuk film atau *Season X • Episode Y* untuk serial).
  - Menampilkan durasi tonton live (*elapsed timestamp*) dan poster thumbnail anime/film.

#### 4. Pemutar Media & Tampilan CLI
- **Integrasi MPV Lebih Matang**:
  - Konfigurasi otomatis demuxer lavf, whitelist protokol HLS, dan peredam spam log demuxer (`--msg-level=ffmpeg/demuxer=error`).
  - Dukungan resume posisi memutar via flag `--start`.
  - Opsi alternatif pemutar media: VLC dan Browser.
  - Pengecekan dependensi instan (langsung masuk ke menu jika MPV sudah terpasang tanpa splash redundan).
- **Refaktor & Kualitas Kode**:
  - Bersih dari compiler warnings (0 warnings).
  - Eliminasi dead code, magic numbers/strings, dan deep nesting.
  - Ditambahkan unit test & integration test otomatis (`cargo test`).

#### 5. Kompatibilitas Windows & Cross-Platform
- **Auto-Install & Deteksi Package Manager**:
  - Memperbaiki deteksi package manager Windows (`winget`, `choco`, `scoop`) yang sebelumnya gagal karena `which` tidak tersedia di Windows.
  - Mendukung auto-install MPV resmi di Windows melalui winget (`shinchiro.mpv` dengan fallback `mpv.net`).
  - Pencarian jalur binary cerdas (mendeteksi MPV/VLC di `Program Files`, `WindowsApps`, `scoop`, `chocolatey`).
- **Penyimpanan Temp File Cross-Platform**:
  - Mengganti path hardcoded `/tmp/` dengan direktori temp native sistem (`std::env::temp_dir()`) dan forward slashes yang aman untuk Windows dan script Lua MPV.
- **Self-Update Aman di Windows**:
  - Mencegah error file-locking executable saat self-update di Windows dengan auto-rename binary berjalan.

#### 6. Credits
- [lucasbuilds](https://github.com/lucasbuilds/animeku-cli) - For Base Repo
- [Wingky530](https://github.com/Wingky530/otakudesu-scraper) - For Otakudesu Scraper
- [annurdian](https://github.com/annurdien/IDLIX-API) - For Idlixku API
