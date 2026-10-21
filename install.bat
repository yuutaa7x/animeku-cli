@echo off
setlocal enabledelayedexpansion
title animeku-cli Auto Installer

echo.
echo  =====================================================
echo    animeku-cli Auto Installer for Windows
echo  =====================================================
echo.

:: ---- FAST PATH: Try the prebuilt Windows binary first ----
call :try_prebuilt
if "!PREBUILT_OK!"=="1" goto :deps

:: ---- STEP 1: Check / Install Rust ----
where cargo >nul 2>&1
if %ERRORLEVEL% NEQ 0 (
    echo [1/4] Rust not found. Downloading Rust installer...
    curl -# -L -o "%TEMP%\rustup-init.exe" "https://win.rustup.rs/x86_64"
    if not exist "%TEMP%\rustup-init.exe" (
        echo [ERROR] Failed to download Rust. Check your internet connection.
        pause
        exit /b 1
    )
    "%TEMP%\rustup-init.exe" -y --profile minimal --default-toolchain stable-x86_64-pc-windows-gnu
    if !ERRORLEVEL! NEQ 0 (
        echo [ERROR] Rust installation failed.
        pause
        exit /b 1
    )
    set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
    echo [OK] Rust installed.
) else (
    echo [1/4] Rust already installed. Skipping.
)
echo.

:: ---- STEP 2: Check MSVC linker ----
where link.exe >nul 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [2/4] MSVC linker found. Skipping GNU setup.
    echo [3/4] Skipping MSYS2/GCC setup.
    goto :install
)

echo [2/4] No MSVC linker. Setting up GNU toolchain...
rustup toolchain install stable-x86_64-pc-windows-gnu --profile minimal --force
if %ERRORLEVEL% NEQ 0 (
    echo [ERROR] Failed to install GNU toolchain.
    pause
    exit /b 1
)
rustup default stable-x86_64-pc-windows-gnu
if %ERRORLEVEL% NEQ 0 (
    echo [ERROR] Failed to set GNU toolchain as default.
    pause
    exit /b 1
)
echo [OK] GNU toolchain set as default.
echo.

:: ---- STEP 3: Check / Install GCC ----
where gcc >nul 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [3/4] GCC already found. Skipping MSYS2 install.
    goto :install
)

if exist "C:\msys64\usr\bin\bash.exe" (
    echo [3/4] MSYS2 found. Installing GCC...
    goto :install_gcc
)

echo [3/4] Installing MSYS2...

:: Try winget first
where winget >nul 2>&1
if %ERRORLEVEL% EQU 0 (
    echo [INFO] Using winget to install MSYS2...
    winget install --id MSYS2.MSYS2 -e --silent --source winget --accept-package-agreements --accept-source-agreements
    if !ERRORLEVEL! EQU 0 goto :install_gcc
    echo [WARN] winget failed. Falling back to direct download...
)

:: Fallback: use curl to get latest release URL then download
echo [INFO] Getting latest MSYS2 download URL...
for /f "usebackq delims=" %%A in (`powershell -NoProfile -Command "(Invoke-RestMethod 'https://api.github.com/repos/msys2/msys2-installer/releases/latest').assets | Where-Object { $_.name -like '*x86_64*.exe' } | Select-Object -First 1 -ExpandProperty browser_download_url"`) do set MSYS2_URL=%%A

if "!MSYS2_URL!"=="" (
    echo [ERROR] Could not get MSYS2 download URL.
    pause
    exit /b 1
)

echo [INFO] Downloading MSYS2 from !MSYS2_URL!
curl -# -L -o "%TEMP%\msys2-installer.exe" "!MSYS2_URL!"

if not exist "%TEMP%\msys2-installer.exe" (
    echo [ERROR] Failed to download MSYS2. Check your internet connection.
    pause
    exit /b 1
)

:: 7-Zip SFX extract: -y = auto yes, -oC:\ = extract to C:\ (creates C:\msys64)
set MSYS2_OUTDIR=C:\
"%TEMP%\msys2-installer.exe" -y -o%MSYS2_OUTDIR%
if %ERRORLEVEL% NEQ 0 (
    echo [ERROR] MSYS2 extraction failed.
    pause
    exit /b 1
)
echo [OK] MSYS2 installed.

:install_gcc
echo.
echo [3/4] Installing mingw-w64 GCC via pacman...
C:\msys64\usr\bin\bash.exe -lc "mkdir -p /tmp && pacman -S --needed --noconfirm --overwrite '/mingw64' --overwrite '/mingw64/*' mingw-w64-x86_64-gcc"
if %ERRORLEVEL% NEQ 0 (
    echo [WARN] GCC install failed. Refreshing MSYS2 packages and retrying...
    C:\msys64\usr\bin\bash.exe -lc "mkdir -p /tmp && pacman -Syu --noconfirm"
    C:\msys64\usr\bin\bash.exe -lc "mkdir -p /tmp && pacman -S --needed --noconfirm --overwrite '/mingw64' --overwrite '/mingw64/*' mingw-w64-x86_64-gcc"
)
if %ERRORLEVEL% NEQ 0 (
    echo [WARN] Still failing. Moving the existing C:\msys64\mingw64 aside and retrying...
    if exist "C:\msys64\mingw64.bak" rmdir /s /q "C:\msys64\mingw64.bak"
    if exist "C:\msys64\mingw64" move "C:\msys64\mingw64" "C:\msys64\mingw64.bak"
    C:\msys64\usr\bin\bash.exe -lc "mkdir -p /tmp && pacman -S --needed --noconfirm mingw-w64-x86_64-gcc"
)
if %ERRORLEVEL% NEQ 0 (
    echo [ERROR] GCC installation failed.
    echo [INFO] Jalankan manual di MSYS2:
    echo        pacman -Syu --noconfirm
    echo        pacman -S --needed --noconfirm --overwrite '/mingw64' --overwrite '/mingw64/*' mingw-w64-x86_64-gcc
    pause
    exit /b 1
)
if not exist "C:\msys64\mingw64\bin\gcc.exe" (
    echo [ERROR] GCC executable tidak ditemukan setelah instalasi.
    pause
    exit /b 1
)

:: Add mingw to PATH permanently (skip if already added)
powershell -NoProfile -Command "$p=[Environment]::GetEnvironmentVariable('PATH','User'); if($p -notlike '*msys64\mingw64\bin*'){[Environment]::SetEnvironmentVariable('PATH',$p+';C:\msys64\mingw64\bin','User')}"
set "PATH=C:\msys64\mingw64\bin;%PATH%"
echo [OK] GCC installed and added to PATH.
echo.

:: ---- STEP 4: Install animeku-cli ----
:install
echo [4/4] Installing animeku-cli from source...
echo.

cargo install --git https://github.com/yuutaa7x/animeku-cli --locked --force
if %ERRORLEVEL% NEQ 0 (
    echo.
    echo  [ERROR] Installation failed. Check errors above.
    pause
    exit /b 1
)
if exist "%USERPROFILE%\.cargo\bin\animeku-cli.exe" (
    copy /y "%USERPROFILE%\.cargo\bin\animeku-cli.exe" "%USERPROFILE%\.cargo\bin\animeku.exe" >nul 2>&1
)
goto :deps

:deps
where mpv >nul 2>&1
if !ERRORLEVEL! NEQ 0 (
    if exist "%ProgramFiles%\MPV Player\mpv.exe" (
        echo [INFO] MPV player is already installed at %ProgramFiles%\MPV Player\mpv.exe
    ) else (
        echo.
        echo [INFO] MPV player is not installed yet.
        where winget >nul 2>&1
        if !ERRORLEVEL! EQU 0 (
            echo [INFO] Installing MPV automatically via winget...
            winget install --id shinchiro.mpv -e --source winget --accept-package-agreements --accept-source-agreements
        ) else (
            echo [INFO] Please install MPV from https://mpv.io/installation/
        )
    )
)

where ffmpeg >nul 2>&1
if !ERRORLEVEL! NEQ 0 (
    echo.
    echo [INFO] ffmpeg is not installed yet. Episode download requires it.
    where winget >nul 2>&1
    if !ERRORLEVEL! EQU 0 (
        echo [INFO] Installing ffmpeg automatically via winget...
        winget install --id Gyan.FFmpeg -e --source winget --accept-package-agreements --accept-source-agreements
    ) else (
        echo [INFO] Please install ffmpeg from https://ffmpeg.org/download.html
    )
)

echo.
echo  =====================================================
echo    SUCCESS - animeku-cli installed
echo    Run it with: animeku or animeku-cli
echo  =====================================================
echo.
pause
endlocal
exit /b 0

:: ---- FAST PATH: Prebuilt Windows binary ----
:try_prebuilt
set "PREBUILT_OK=0"
set "ANIMEKU_URL=https://github.com/yuutaa7x/animeku-cli/releases/latest/download/animeku-cli-windows-x86_64.exe"
echo [INFO] Checking prebuilt binary...
where powershell >nul 2>&1
if %ERRORLEVEL% NEQ 0 (
    echo [INFO] PowerShell not found. Falling back to source build.
    exit /b 0
)

echo [INFO] Downloading prebuilt binary...
curl -fL -# -o "%TEMP%\animeku-cli.exe" "!ANIMEKU_URL!"
if !ERRORLEVEL! NEQ 0 (
    echo [INFO] No prebuilt release found. Falling back to source build.
    del /q "%TEMP%\animeku-cli.exe" >nul 2>&1
    exit /b 0
)
if not exist "%TEMP%\animeku-cli.exe" (
    echo [WARN] Download failed. Falling back to source build.
    exit /b 0
)

:: Verify the checksum when the release provides one.
curl -fsSL -o "%TEMP%\animeku-cli.exe.sha256" "!ANIMEKU_URL!.sha256" >nul 2>&1
if not exist "%TEMP%\animeku-cli.exe.sha256" (
    echo [WARN] Checksum is not available. Falling back to source build.
    del /q "%TEMP%\animeku-cli.exe" >nul 2>&1
    exit /b 0
)
call :verify_prebuilt_checksum
if !ERRORLEVEL! NEQ 0 (
    echo [WARN] Checksum mismatch. Falling back to source build.
    del /q "%TEMP%\animeku-cli.exe" "%TEMP%\animeku-cli.exe.sha256" >nul 2>&1
    exit /b 0
)

:prebuilt_install
if not exist "%USERPROFILE%\.cargo\bin" mkdir "%USERPROFILE%\.cargo\bin"
move /y "%TEMP%\animeku-cli.exe" "%USERPROFILE%\.cargo\bin\animeku-cli.exe" >nul
if !ERRORLEVEL! NEQ 0 (
    echo [WARN] Could not replace animeku-cli.exe. Close any running animeku-cli and rerun the installer.
    exit /b 0
)
copy /y "%USERPROFILE%\.cargo\bin\animeku-cli.exe" "%USERPROFILE%\.cargo\bin\animeku.exe" >nul 2>&1

:: Make sure the install directory is available in new terminals.
powershell -NoProfile -Command "$p=[Environment]::GetEnvironmentVariable('PATH','User'); if($p -notlike '*\.cargo\bin*'){[Environment]::SetEnvironmentVariable('PATH',$p+';%USERPROFILE%\.cargo\bin','User')}"
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

set "PREBUILT_OK=1"
echo [OK] Prebuilt binary installed.
exit /b 0

:verify_prebuilt_checksum
powershell -NoProfile -Command "$expected=(Get-Content '%TEMP%\animeku-cli.exe.sha256' -Raw).Split()[0].ToLower(); $actual=(Get-FileHash '%TEMP%\animeku-cli.exe' -Algorithm SHA256).Hash.ToLower(); if ($actual -ne $expected) { exit 1 }"
exit /b %ERRORLEVEL%
