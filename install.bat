@echo off
setlocal enabledelayedexpansion
title animeku-cli Auto Installer

echo.
echo  =====================================================
echo    animeku-cli Auto Installer for Windows
echo  =====================================================
echo.

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
C:\msys64\usr\bin\bash.exe -lc "pacman -Sy --noconfirm mingw-w64-x86_64-gcc"
if %ERRORLEVEL% NEQ 0 (
    echo [ERROR] GCC installation failed.
    pause
    exit /b 1
)

:: Add mingw to PATH permanently (skip if already added)
powershell -NoProfile -Command "$p=[Environment]::GetEnvironmentVariable('PATH','User'); if($p -notlike '*msys64*'){[Environment]::SetEnvironmentVariable('PATH',$p+';C:\msys64\mingw64\bin','User')}"
set "PATH=C:\msys64\mingw64\bin;%PATH%"
echo [OK] GCC installed and added to PATH.
echo.

:: ---- STEP 4: Install animeku-cli ----
:install
echo [4/4] Installing animeku-cli...
echo.

cargo install --git https://github.com/yuutaa7x/animeku-cli

if %ERRORLEVEL% EQU 0 (
    if exist "%USERPROFILE%\.cargo\bin\animeku-cli.exe" (
        copy /y "%USERPROFILE%\.cargo\bin\animeku-cli.exe" "%USERPROFILE%\.cargo\bin\animeku.exe" >nul 2>&1
    )

    where mpv >nul 2>&1
    if !ERRORLEVEL! NEQ 0 (
        echo.
        echo [INFO] MPV player is not installed yet.
        where winget >nul 2>&1
        if !ERRORLEVEL! EQU 0 (
            echo [INFO] Installing MPV automatically via winget...
            winget install --id shinchiro.mpv -e --accept-package-agreements --accept-source-agreements
        ) else (
            echo [INFO] Please install MPV from https://mpv.io/installation/
        )
    )

    echo.
    echo  =====================================================
    echo    SUCCESS! animeku-cli installed!
    echo    Run it with: animeku  (or animeku-cli)
    echo  =====================================================
) else (
    echo.
    echo  [ERROR] Installation failed. Check errors above.
)

echo.
pause
endlocal
