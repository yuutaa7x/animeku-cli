<div align="center">
  
![thanks ai hehe](https://i.ibb.co/yqjnrCP/8-WOp-J0-XSca-EN5krf-NNXb-F-transformed.png)

Streaming anime dan film Asia dengan subtitle Indonesia lewat terminal.

*Fork dari [lucasbuilds/animeku-cli](https://github.com/lucasbuilds/animeku-cli)* | Vibecoded BTW (sorry for this bad things)

</div>

### Tutorial Penginstalan & Penggunaan

https://github.com/user-attachments/assets/5d029903-ba1f-46c4-ab72-f021d8500a7c

### Instalasi
> [!NOTE]
> Pemutar media (**mpv** sangat disarankan) tetap dibutuhkan. Fitur **download episode** juga membutuhkan **ffmpeg**. Rust hanya dibutuhkan untuk instalasi manual atau saat fallback build dari source; installer prebuilt tidak memerlukan Rust. Jika ingin memasang Rust, klik [tautan berikut](https://rustup.rs/).
>
> - **Windows**: Install MPV dan ffmpeg melalui terminal (PowerShell / CMD):
>   ```powershell
>   winget install --id shinchiro.mpv
>   winget install --id Gyan.FFmpeg
>   ```
> - **Linux**: `sudo apt install mpv ffmpeg` (Ubuntu/Debian) atau `sudo pacman -S mpv ffmpeg` (Arch).
> - **Android (Termux)**: `pkg install rust mpv ffmpeg`.

#### Auto Install (Windows — satu baris)

Buka **PowerShell** atau **CMD** lalu jalankan perintah berikut:

```powershell
powershell -c "irm https://raw.githubusercontent.com/yuutaa7x/animeku-cli/refs/heads/main/install.bat -OutFile install.bat; .\install.bat"
```

Installer Windows akan mencoba mengunduh binary rilis terbaru terlebih dahulu. Jika rilis belum tersedia, installer otomatis fallback ke build dari source.

#### Auto Install (Linux / macOS — satu baris)

Buka terminal lalu jalankan:

```bash
curl -sL https://raw.githubusercontent.com/yuutaa7x/animeku-cli/refs/heads/main/install.sh | bash
```

Installer akan mencoba binary rilis prebuilt terlebih dahulu (Linux x86_64/aarch64 dan macOS Intel/Apple Silicon), memverifikasi SHA-256, lalu memasangnya ke `~/.local/bin` atau `~/.cargo/bin`. Jika asset belum tersedia atau verifikasi gagal, installer otomatis fallback ke build dari source (Rust akan dipasang bila belum ada). MPV dan ffmpeg juga dipasang otomatis bila memungkinkan.

#### Auto Install (Android / Termux — satu baris)

Buka aplikasi Termux lalu jalankan script dibawah ini untuk auto-install:

```bash
curl -sL https://raw.githubusercontent.com/yuutaa7x/animeku-cli/refs/heads/main/install_termux.sh | bash
```

Tunggu instalasi selesai, lalu install VLC for Android dari Play Store sebelum menonton. Script juga menyertakan ffmpeg untuk fitur download episode.

#### Manual Install (semua platform)

*Copy paste* perintah berikut:
```bash
cargo install --git https://github.com/yuutaa7x/animeku-cli --locked
```

Silakan tunggu proses penginstallan selesai dan jika sudah, ketik `animeku-cli` untuk menjalankannya.

#### Update

Binary hasil installer bisa memperbarui dirinya sendiri tanpa Rust:

```bash
animeku-cli --check-update   # cek versi terbaru
animeku-cli --update         # unduh, verifikasi SHA-256, lalu pasang
```

Update juga tersedia lewat **Pengaturan → Perbarui Animeku-CLI**. Asset yang dipakai mengikuti platform:

| Platform | Asset rilis |
| --- | --- |
| Windows x86_64 | `animeku-cli-windows-x86_64.exe` |
| Linux x86_64 | `animeku-cli-linux-x86_64` |
| Linux aarch64 | `animeku-cli-linux-aarch64` |
| macOS Intel | `animeku-cli-macos-x86_64` |
| macOS Apple Silicon | `animeku-cli-macos-aarch64` |

Setiap asset disertai file `.sha256`. Jika asset tidak tersedia, update otomatis fallback ke `cargo install`. Source checkout lokal tetap memakai `git pull` + `cargo install --path .` agar perubahan development tidak tertimpa binary rilis.

#### Troubleshooting Windows: konflik `/mingw64`

Jika installer berhenti di `GCC installation failed` dengan pesan `/mingw64 exists in filesystem`, jalankan perintah berikut dari **CMD**:

```bat
C:\msys64\usr\bin\bash.exe -lc "mkdir -p /tmp && pacman -S --needed --noconfirm --overwrite '/mingw64' --overwrite '/mingw64/*' mingw-w64-x86_64-gcc"
```

Jika masih gagal, refresh database MSYS2 lalu ulangi:

```bat
C:\msys64\usr\bin\bash.exe -lc "pacman -Syu --noconfirm"
```

Installer versi terbaru sudah mencoba langkah ini otomatis, termasuk memindahkan folder `C:\msys64\mingw64` ke `mingw64.bak` sebagai upaya terakhir.

### Uninstall

#### Auto Uninstall (Windows — satu baris)

Buka **PowerShell** atau **CMD** lalu jalankan perintah berikut:

```powershell
powershell -c "irm https://raw.githubusercontent.com/yuutaa7x/animeku-cli/refs/heads/main/uninstall.bat -OutFile uninstall.bat; .\uninstall.bat"
```

#### Auto Uninstall (Linux / macOS / Termux — satu baris)

```bash
curl -sL https://raw.githubusercontent.com/yuutaa7x/animeku-cli/refs/heads/main/uninstall.sh | bash
```

#### Manual Uninstall (Linux / macOS)

```bash
cargo uninstall animeku-cli 2>/dev/null || true
rm -f ~/.local/bin/animeku-cli ~/.local/bin/animeku ~/.cargo/bin/animeku-cli ~/.cargo/bin/animeku
```

Untuk Windows, hapus `animeku-cli.exe` dan `animeku.exe` dari `%USERPROFILE%\.cargo\bin`.


### Keunggulan & Fitur
- Sangat mudah digunakan langsung dari command line.
- Menonton Anime, Live Action, Film, dan TV Series secara gratis tanpa iklan.
- Hasil pencarian lengkap dengan pilihan server & resolusi (1080p, 720p, 480p, 360p).
- Otomatis memuat Subtitle Indonesia langsung ke player.
- Download episode dengan pilihan kualitas/server, progress bar, nama file kustom, dan folder tujuan.
- Download batch/rentang episode dengan antrean berurutan, kualitas otomatis mengikuti pilihan awal, progres keseluruhan + estimasi sisa waktu, dan opsi lewati/timpa file yang sudah ada.
- Antrean batch disimpan otomatis ke `~/.animeku_batch_queue.json`. Kalau download dibatalkan atau gagal, menu **Lanjutkan Batch** dan opsi di pemilih episode bisa melanjutkan item yang belum selesai tanpa mengunduh ulang episode yang sudah beres.
- Retry batch 3 kali dengan exponential backoff (1s, 2s). Episode yang tetap gagal dicatat ke `animeku-batch.log` di folder download, lalu dicoba lagi saat batch dilanjutkan.
- Download bisa dibatalkan dengan `Esc`, `q`, atau `Ctrl+C`; ffmpeg dihentikan, file `.part` dibersihkan, dan ringkasan parsial ditampilkan.
- Subtitle `.vtt` disimpan sebagai sidecar secara default. Pengaturan **Embed subtitle** bisa menanam subtitle langsung ke `.mkv`/`.mp4` (SRT/mov_text) agar video dan subtitle jadi satu file.
- Terintegrasi dengan Discord Rich Presence (RPC).
- Fitur Riwayat Tontonan multi-entry (menyimpan histori judul & episode yang ditonton).
- Fitur Riwayat Pencarian (suggestion otomatis saat mengetik).
- Resume Otomatis (melanjutkan episode dari menit/detik terakhir yang ditonton).
- Auto-Next Episode setelah selesai memutar.
- Pengaturan persisten: player default, auto-next, provider, Discord RPC, banner compact, poster episode, embed subtitle, dan folder download.
- Dukungan CLI flags untuk pencarian langsung, resume, update, cek update, pemilihan provider/player, dan menonaktifkan RPC.
- Pengecekan dependensi dan Check Health sistem yang interaktif.
- Retry otomatis dan fallback Idlix ↔ Otakudesu saat provider error atau tidak mengembalikan hasil.
- Cache metadata persisten untuk hasil pencarian dan daftar episode, sehingga pencarian ulang lebih cepat tanpa mengambil stream lama.
- Watchlist untuk menyimpan judul, lalu langsung memilih episode atau melanjutkan tontonan terakhir dari menu utama.
- **Cek Episode Baru** di watchlist menyimpan jumlah episode terakhir dan memberi notifikasi berapa episode baru yang tersedia. Dari detail watchlist kamu juga bisa langsung **Download Episode Baru** sebagai batch.
- **Jelajahi anime (AniList)** menampilkan trending anime, skor, tahun, genre, dan sinopsis. Detail AniList juga bisa dibuka dari watchlist.

### Changelog

#### v0.4.1 — Perbaikan urutan episode baru lintas season (21 Oktober 2026)

- **Download Episode Baru** sekarang mendeteksi urutan daftar episode terbaru/terlama dengan memperhitungkan season, sehingga judul multi-season tidak salah mengambil episode lama.

#### v0.4.0 — Prebuilt multi-platform, batch resume & AniList (21 Oktober 2026)

- Rilis prebuilt sekarang tersedia untuk Windows x86_64, Linux x86_64/aarch64, dan macOS Intel/Apple Silicon. Setiap asset punya checksum SHA-256.
- Installer Linux/macOS mencoba binary prebuilt lebih dulu, memverifikasi checksum, lalu fallback ke build source bila perlu. Windows installer tetap memakai asset `animeku-cli-windows-x86_64.exe`.
- Self-update baru: `--check-update`, `--update`, dan menu **Pengaturan → Perbarui Animeku-CLI**. Binary rilis diunduh, diverifikasi, lalu diganti secara atomik; source checkout lokal tetap memakai `git pull` + `cargo install --path .`.
- Antrean batch disimpan di `~/.animeku_batch_queue.json` dan bisa dilanjutkan dari menu utama atau pemilih episode. Status per episode (`Pending`, `Completed`, `Skipped`, `Failed`) tidak hilang saat aplikasi ditutup.
- Retry batch dinaikkan menjadi 3 percobaan dengan exponential backoff. Kegagalan ditulis ke `animeku-batch.log` di folder download.
- Pengaturan **Embed subtitle** menanam subtitle ke dalam `.mkv`/`.mp4`; sidecar `.vtt` tetap menjadi default.
- Integrasi AniList: menu trending, detail skor/genre/sinopsis, dan tombol **Info AniList** di watchlist.
- Watchlist menyimpan jumlah episode terakhir. Aksi **Cek Episode Baru** memberi notifikasi episode baru, dan **Download Episode Baru** membuat batch otomatis dari episode setelah tontonan terakhir.
- Maintenance: release profile LTO + strip, HTTP client memakai rustls (tanpa OpenSSL), CI menjalankan clippy `-D warnings`, dan test baru untuk queue batch, checksum update, AniList, serta mapping subtitle.

#### v0.3.8 — Progres batch & pembatalan download (21 Oktober 2026)

- Progres keseluruhan batch sekarang menampilkan posisi `Batch x/y`, jumlah berhasil/dilewati/gagal, dan estimasi sisa waktu setelah tiap episode selesai diproses.
- Download episode maupun batch bisa dibatalkan dengan `Esc`, `q`, atau `Ctrl+C`. ffmpeg langsung dihentikan, file `.part` dihapus, dan batch berhenti tanpa melanjutkan episode berikutnya.
- Saat batch dibatalkan, ringkasan parsial menampilkan jumlah episode yang sudah diproses dan berapa yang belum diproses, lalu kembali ke pemilih episode.
- Retry per episode tetap berjalan, tetapi pembatalan tidak memicu retry dan tidak meninggalkan file setengah jadi.
- Installer Windows kini memakai binary rilis terbaru bila tersedia, sehingga tidak perlu compile Rust/GCC dari source. Jika belum ada rilis, installer tetap fallback ke build dari source.
- Memperbaiki pesan `Installation failed` palsu setelah instalasi sukses, yang disebabkan karakter `)` dan `!` di dalam blok `if` pada batch installer.

#### v0.3.7 — Download batch episode (21 Oktober 2026)

- Aksi baru `[ ⬇ Download Batch Episode... ]` di pemilih server/kualitas untuk serial dengan lebih dari satu episode.
- Pilih rentang episode mulai/sampai, folder tujuan cukup sekali, lalu tentukan kebijakan untuk file yang sudah ada: lewati atau timpa.
- Kualitas yang dipilih di episode awal dipakai otomatis untuk seluruh batch. Kalau kualitas itu tidak tersedia di salah satu episode, stream pertama dipakai dan muncul peringatan.
- Setiap episode me-resolve stream tepat sebelum diunduh. Ini penting untuk Idlix karena playlist lokal antar-episode saling menimpa.
- Antrean berjalan berurutan, mencoba ulang sekali jika download gagal, tetap lanjut ke episode berikutnya, lalu menampilkan ringkasan berhasil/dilewati/gagal.
- Opsi lewati file yang sudah ada bisa dipakai untuk melanjutkan batch yang terputus tanpa mengunduh ulang episode yang selesai.
- Setelah batch selesai, aplikasi kembali ke pemilih episode agar stream Idlix yang sudah basi tidak ikut diputar.

#### v0.3.6 — Perbaikan download setelah validasi live (21 Oktober 2026)

- Idlix: playlist HLS yang menyamarkan segmen sebagai `.webp`, `.jpg`, `.js`, `.css`, `.svg`, `.png`, atau `.html` sekarang bisa diunduh. ffmpeg versi baru memisahkan pengecekan `allowed_segment_extensions` dan mengaktifkan `extension_picky`; animeku-cli menyesuaikan opsi tersebut secara otomatis.
- Otakudesu: halaman desustream versi baru yang membungkus pemutar Blogger sekarang di-resolve sampai URL `googlevideo`, sehingga download tidak lagi berhenti di halaman embed.
- Menambahkan smoke test live (opt-in, `#[ignore]`) untuk memastikan video dan audio hasil download Idlix/Otakudesu benar-benar terbaca oleh `ffprobe`.

#### v0.3.5 — Download episode dengan ffmpeg (21 Oktober 2026)

- Aksi baru `[ ⬇ Download Episode Ini ]` di pemilih server/kualitas. Setelah itu muncul pemilih kualitas/server terpisah sebelum download dimulai.
- Folder tujuan dan nama file ditanyakan setiap download. Folder terakhir diingat sebagai default dan bisa diubah lewat **Pengaturan → Folder download**.
- Progress download menampilkan persentase, ukuran file, dan kecepatan. Hasil ditulis ke file `.part` lalu di-rename setelah ffmpeg selesai, sehingga file setengah jadi tidak dianggap sebagai video final.
- Idlix desktop: playlist HLS video dan audio terpisah digabung menjadi satu file. Idlix Android: master playlist yang sudah berisi audio dipakai langsung. Otakudesu: URL MP4/Google Video diunduh dengan `Referer` dan user agent yang sesuai.
- Subtitle Idlix disalin sebagai file `.vtt` sidecar dengan nama yang sama seperti video.
- Default folder: `~/Downloads/Animeku-CLI` (Android: `/sdcard/Download/Animeku-CLI`). Default format: `.mkv` untuk Idlix, `.mp4` untuk Otakudesu.
- Membutuhkan `ffmpeg`. Check Health sekarang menampilkan status ffmpeg, dan auto-install dependensi ikut memasang ffmpeg.
- Perbaikan cache stream Idlix: playlist lokal selalu diambil ulang agar tidak memutar atau mengunduh episode yang salah setelah berpindah episode.

#### v0.3.4 — Watchlist (21 Oktober 2026)

- Menu **Watchlist** baru di menu utama, lengkap dengan jumlah judul yang disimpan.
- Judul bisa ditandai lewat menu `[ ☆ Tambah ke Watchlist ]` di daftar episode atau pemilih server/kualitas.
- Dari watchlist kamu bisa langsung memilih episode, melanjutkan episode terakhir, memutar ulang episode yang sudah tamat, atau menghapus judul.
- Status tontonan terakhir dan provider ditampilkan di daftar watchlist.
- Watchlist disimpan di `~/.animeku_watchlist.json`.
- Menu Pengaturan sekarang punya aksi **Hapus watchlist**.

#### v0.3.3 — Cache metadata persisten (21 Oktober 2026)

- Hasil pencarian dan daftar episode sekarang disimpan di cache disk dan dipakai lagi antar-sesi.
- Cache pencarian berlaku 6 jam dan cache daftar episode 12 jam. Setelah kedaluwarsa, data diambil ulang dari provider.
- Stream URL tetap tidak pernah disimpan; setiap playback selalu mengambil URL baru agar tidak kena token/stream kedaluwarsa.
- Cache ikut dipakai saat fallback Idlix ↔ Otakudesu, termasuk untuk mencari judul dan episode yang cocok di provider satunya.
- Entri lama dibersihkan otomatis dan jumlah entri dibatasi agar file cache tidak terus membengkak.
- Menu Pengaturan → hapus cache metadata & provider sekarang juga menghapus cache metadata persisten.
- Lokasi cache: `dirs::cache_dir()/animeku-cli/metadata-cache.json` (Linux biasanya `~/.cache/animeku-cli/metadata-cache.json`).

#### v0.3.2 — Reliability: retry, timeout & fallback provider (21 Oktober 2026)

- Pencarian, daftar episode, dan stream sekarang dicoba ulang hingga 3 kali dengan jeda singkat sebelum menyerah.
- Timeout eksplisit untuk request Otakudesu (connect 10 detik, total 20 detik). Idlix tetap memakai timeout curl yang sudah ada.
- Jika provider aktif gagal atau hasilnya kosong, animeku-cli otomatis mencoba provider satunya. Fallback mencocokkan judul dan nomor episode sebelum melanjutkan playback.
- Provider yang baru dipakai ditampilkan di layar, termasuk alasan fallback. Auto-next, riwayat tontonan, dan Discord RPC memakai movie/episode/provider hasil fallback, bukan ID lama.
- Cache stream tetap hanya di memori dan tidak pernah ditulis ke disk.

#### v0.3.1 — Perbaikan status loading (21 Oktober 2026)

- Mengganti spinner animasi dengan status loading satu baris. Spinner berbasis carriage return masih bisa ter-wrap dan meninggalkan baris duplikat di terminal sempit atau emulator tertentu.
- Status loading sekarang hanya dicetak sekali, tidak menimpa baris lain, dan hasil operasi tetap muncul setelahnya.

#### v0.3.0 — UX Overhaul & CLI Flags (21 Oktober 2026)

Rilis ini fokus pada kenyamanan navigasi, riwayat, dan kontrol dari terminal.

##### Navigasi & Input
- Menu interaktif baru dengan footer hint yang konsisten. `Esc`, `q` / `q+Enter`, dan `Ctrl+C` sekarang berfungsi untuk kembali tanpa menutup aplikasi secara tidak sengaja.
- Main menu baru: Continue Watching, Cari anime/film, Riwayat, Pengaturan, Cek Kesehatan, Update, dan Keluar.
- Input pencarian live dengan cursor, Home/End, Delete, `Ctrl+U`, `Ctrl+W`, paste, dan Tab completion.
- Saran judul menggabungkan hasil AniList dengan riwayat pencarian lokal, jadi tetap berguna saat jaringan lambat.
- Membatalkan playback tidak lagi menyimpan riwayat atau memicu auto-next.
- Playback lewat browser tidak lagi keluar dari aplikasi; setelah selesai, tekan Enter untuk kembali.
- Resume dari riwayat tontonan sekarang mendukung auto-next episode berikutnya.

##### Episode & Pencarian
- Episode picker menampilkan penanda `▶` untuk episode terakhir dan `✓` untuk episode yang sudah ditonton.
- Pencarian episode global: ketik saat memilih episode untuk mencari ke semua halaman, bukan hanya halaman aktif.
- Navigasi halaman episode dan hasil pencarian bisa memakai tombol panah kiri/kanan.
- Hasil pencarian menampilkan halaman aktif, jumlah hasil, dan total episode.
- Footer hint tetap terlihat saat daftar episode atau hasil pencarian dipaginasi.

##### Riwayat, Provider & Pengaturan
- Riwayat tontonan menampilkan status, posisi terakhir, waktu relatif, dan provider.
- Konfirmasi sebelum menghapus riwayat, plus aksi hapus semua riwayat.
- Aksi "Pilih Episode Lain" langsung dari detail riwayat.
- Provider pilihan per judul diingat, begitu juga hasil deteksi AniList, sehingga pencarian berulang tidak melakukan deteksi dari awal.
- Override provider lewat config, CLI, atau aksi "Ganti Provider & Cari".
- Menu Pengaturan untuk player default, auto-next, provider, Discord RPC, banner compact, poster episode, folder download, hapus riwayat, hapus watchlist, dan hapus cache metadata/provider.
- Config disimpan sebagai JSON di `dirs::config_dir()/animeku-cli/config.json` (Linux biasanya `~/.config/animeku-cli/config.json`).

##### Tampilan & Error Handling
- Status loading ringan tanpa dependency tambahan.
- Error pencarian, daftar episode, dan server stream ditampilkan dengan pesan yang lebih ramah dan kembali ke layar yang sesuai.
- Banner compact otomatis untuk terminal pendek, plus opsi banner compact di Pengaturan.
- Poster episode di-cache di memori, diskalakan mengikuti ukuran terminal, dan punya timeout agar tidak menggantung.
- Perbaikan copywriting dan pesan bantuan.

##### CLI Flags
```bash
animeku-cli --search "Solo Leveling"
animeku-cli --provider otakudesu --player mpv "One Piece"
animeku-cli --resume
animeku-cli --no-rpc
animeku-cli "Judul Anime"
```
- `--search <JUDUL>`: langsung cari tanpa membuka menu utama.
- `--provider <auto|idlix|otakudesu>`: pilih provider yang dicoba lebih dulu. Jika provider tersebut error atau tidak menemukan hasil, fallback otomatis ke provider satunya.
- `--player <ask|mpv|vlc|browser>`: pilih pemutar untuk sesi ini.
- `--resume`: lanjutkan tontonan terakhir.
- `--no-rpc`: matikan Discord Rich Presence untuk sesi ini.
- `-h` / `--help` dan `-V` / `--version`.

#### v0.2.0 — Perbedaan dari Upstream
Dibandingkan dengan repo upstream (`lucasbuilds/animeku-cli`), fork ini membawa sejumlah perbaikan besar:

##### 1. Menu & Riwayat Tontonan / Pencarian
- **Menu Utama & Submenu Terstruktur**:
  - Ditambahkan fitur interaktif `Check Health` pada menu utama untuk memantau status aplikasi.
  - Submenu dipisah menjadi `Watch Any Anime / Movie` (pencarian baru) dan `Watch & Search History` (gabungan riwayat).
- **Auto-Tracking & Resume Playback Multi-Entry**:
  - Menyimpan *banyak histori tontonan sekaligus* (multi-entry), bisa resume per-judul kapan pun secara presisi.
  - Opsi auto-next episode memudahkan lanjut marathon ke episode selanjutnya tanpa bolak-balik menu.
  - Pilihan untuk menghapus riwayat secara fleksibel.
- **Search History Terintegrasi**:
  - Suggestion interaktif saat mengetik judul pada menu pencarian.
  - Pencarian konten membedakan provider secara otomatis (Movie & Series -> otomatis Idlix; Anime -> pilih Idlix/Otakudesu).

##### 2. Provider Streaming
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

##### 3. Discord Rich Presence (RPC)
- Integrasi Discord RPC real-time yang bersih:
  - Membersihkan format judul (menghapus tag `[Movie]`, `Subtitle Indonesia`, dan kurung kosong).
  - Menampilkan status dinamis (*Full Movie* untuk film atau *Season X • Episode Y* untuk serial).
  - Menampilkan durasi tonton live (*elapsed timestamp*) dan poster thumbnail anime/film.

##### 4. Pemutar Media & Tampilan CLI
- **Integrasi MPV Lebih Matang**:
  - Konfigurasi otomatis demuxer lavf, whitelist protokol HLS, dan peredam spam log demuxer (`--msg-level=ffmpeg/demuxer=error`).
  - Dukungan resume posisi memutar via flag `--start`.
  - Opsi alternatif pemutar media: VLC dan Browser.
  - Pengecekan dependensi instan (langsung masuk ke menu jika MPV sudah terpasang tanpa splash redundan).
- **Refaktor & Kualitas Kode**:
  - Bersih dari compiler warnings (0 warnings).
  - Eliminasi dead code, magic numbers/strings, dan deep nesting.
  - Ditambahkan unit test & integration test otomatis (`cargo test`).

##### 5. Kompatibilitas Windows & Cross-Platform
- **Auto-Install & Deteksi Package Manager**:
  - Script install satu baris tersedia untuk Windows (`.bat`) maupun Linux/macOS (`.sh`).
  - Memperbaiki deteksi package manager Windows (`winget`, `choco`, `scoop`) yang sebelumnya gagal.
  - Mendukung auto-install MPV resmi di Windows melalui winget (`shinchiro.mpv` dengan fallback `mpv.net`).
  - Pencarian jalur binary cerdas (mendeteksi MPV/VLC di `Program Files`, `WindowsApps`, `scoop`, `chocolatey`).
- **Penyimpanan Temp File Cross-Platform**:
  - Mengganti path hardcoded `/tmp/` dengan direktori temp native sistem (`std::env::temp_dir()`) dan forward slashes yang aman untuk Windows dan script Lua MPV.
- **Self-Update Aman di Windows**:
  - Mencegah error file-locking executable saat self-update di Windows dengan auto-rename binary berjalan.

##### 6. Catatan Teknis
- File sesi (cookie Idlix, playlist sementara, subtitle, dan debug log) kini disimpan di direktori data privat user, bukan di `/tmp` yang bisa dibaca user lain.
- Domain provider dapat diganti tanpa rebuild melalui environment variable:
  ```bash
  ANIMEKU_IDLIX_BASE_URL=https://domain-baru.idlixku.com animeku-cli
  ANIMEKU_OTAKUDESU_BASE_URL=https://domain-baru.otakudesu.blog animeku-cli
  ```
- Proxy HLS lokal melakukan streaming segmen (tidak menampung seluruh video di memori), meneruskan `Range`/`Content-Range` dengan benar, dan otomatis berhenti saat playback selesai.

##### 7. Credits
- [lucasbuilds](https://github.com/lucasbuilds/animeku-cli) - For Base Repo
- [Wingky530](https://github.com/Wingky530/otakudesu-scraper) - For Otakudesu Scraper
- [annurdian](https://github.com/annurdien/IDLIX-API) - For Idlixku API
