<div align="center">
  
![thanks ai hehe](https://i.ibb.co/yqjnrCP/8-WOp-J0-XSca-EN5krf-NNXb-F-transformed.png)

Streaming anime dan film Asia dengan subtitle Indonesia lewat terminal.

*Fork dari [lucasbuilds/animeku-cli](https://github.com/lucasbuilds/animeku-cli)* | Vibecoded BTW (sorry for this bad things)

</div>

### instalasi
> [!NOTE]
> Pastikan kamu sudah menginstall Rust dan pemutar media (**mpv** sangat disarankan), jika belum silahkan klik [tautan berikut](https://rustup.rs/).
> 
> Untuk pengguna android (Termux), install dengan menggunakan perintah `pkg install rust mpv`.

Langkah berikutnya silahkan *copy paste* teks dibawah:
```bash
cargo install --git https://github.com/yuutaa7x/animeku-cli
```

Silahkan tunggu proses penginstallan selesai dan jika sudah, ketik `animeku-cli` untuk menjalankannya.

### Keunggulan & Fitur
- Sangat mudah digunakan langsung dari command line.
- Menonton Anime, Live Action, Film, dan TV Series secara gratis tanpa iklan.
- Hasil pencarian lengkap dengan pilihan server & resolusi (1080p, 720p, 480p, 360p).
- Otomatis memuat Subtitle Indonesia langsung ke player.
- Terintegrasi dengan Discord Rich Presence (RPC).

### Changelog (Perbedaan dari Upstream)
Dibandingkan dengan repo upstream (`lucasbuilds/animeku-cli`), fork ini membawa sejumlah perbaikan besar:

#### 1. Provider Streaming
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

#### 2. Discord Rich Presence (RPC)
- Integrasi Discord RPC real-time yang bersih:
  - Membersihkan format judul (menghapus tag `[Movie]`, `Subtitle Indonesia`, dan kurung kosong).
  - Menampilkan status dinamis (*Full Movie* untuk film atau *Season X • Episode Y* untuk serial).
  - Menampilkan durasi tonton live (*elapsed timestamp*) dan poster thumbnail anime/film.

#### 3. Pemutar Media & Tampilan CLI
- **Integrasi MPV Lebih Matang**:
  - Konfigurasi otomatis demuxer lavf, whitelist protokol HLS, dan peredam spam log demuxer (`--msg-level=ffmpeg/demuxer=error`).
  - Opsi alternatif pemutar media: VLC dan Browser.
  - Pengecekan dependensi instan (langsung masuk ke menu jika MPV sudah terpasang tanpa splash redundan).
- **Refaktor & Kualitas Kode**:
  - Bersih dari compiler warnings (0 warnings).
  - Eliminasi dead code, magic numbers/strings, dan deep nesting.
  - Ditambahkan unit test & integration test otomatis (`cargo test`).

#### 4. Credits
- [lucasbuilds](https://github.com/lucasbuilds/animeku-cli) - For Base Repo
- [Wingky530](https://github.com/Wingky530/otakudesu-scraper) - For Otakudesu Scraper
- [annurdian](https://github.com/annurdien/IDLIX-API) - For Idlixku API
