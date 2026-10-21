@echo off
setlocal enabledelayedexpansion
title animeku-cli Uninstaller

echo.
echo  =====================================================
echo    animeku-cli Uninstaller for Windows
echo  =====================================================
echo.

:: ---- STEP 1: Uninstall animeku-cli ----
echo [1/4] Uninstalling animeku-cli...
set "REMOVED=0"
if exist "%USERPROFILE%\.cargo\bin\animeku-cli.exe" (
    del /f /q "%USERPROFILE%\.cargo\bin\animeku-cli.exe" >nul 2>&1
    if not exist "%USERPROFILE%\.cargo\bin\animeku-cli.exe" set "REMOVED=1"
)
if exist "%USERPROFILE%\.cargo\bin\animeku.exe" (
    del /f /q "%USERPROFILE%\.cargo\bin\animeku.exe" >nul 2>&1
    if not exist "%USERPROFILE%\.cargo\bin\animeku.exe" set "REMOVED=1"
)
where cargo >nul 2>&1
if %ERRORLEVEL% EQU 0 (
    cargo uninstall animeku-cli >nul 2>&1
    if !ERRORLEVEL! EQU 0 set "REMOVED=1"
)
if "!REMOVED!"=="1" (
    echo [OK] animeku-cli removed.
) else (
    echo [WARN] animeku-cli was not found or could not be removed.
    echo        Close any running animeku-cli and try again.
)
echo.

:: ---- STEP 2: Remove GNU toolchain (optional) ----
echo [2/4] Remove GNU Rust toolchain?
echo   Y = Yes, remove stable-x86_64-pc-windows-gnu
echo   N = No, keep it
echo.
set /p REMOVE_GNU="Your choice (Y/N): "
if /i "!REMOVE_GNU!"=="Y" (
    where rustup >nul 2>&1
    if %ERRORLEVEL% EQU 0 (
        rustup toolchain uninstall stable-x86_64-pc-windows-gnu
        echo [OK] GNU toolchain removed.
    ) else (
        echo [SKIP] rustup not found.
    )
) else (
    echo [SKIP] Keeping GNU toolchain.
)
echo.

:: ---- STEP 3: Uninstall Rust completely (optional) ----
echo [3/4] Remove Rust and Cargo completely?
echo   Y = Yes, run rustup self uninstall
echo   N = No, keep Rust
echo.
set /p REMOVE_RUST="Your choice (Y/N): "
if /i "!REMOVE_RUST!"=="Y" (
    where rustup >nul 2>&1
    if %ERRORLEVEL% EQU 0 (
        rustup self uninstall -y
        echo [OK] Rust removed.
    ) else (
        echo [SKIP] Rust not found.
    )
) else (
    echo [SKIP] Keeping Rust.
)
echo.

:: ---- STEP 4: Remove MSYS2 (optional) ----
echo [4/4] Remove MSYS2 (C:\msys64)?
echo   Y = Yes, delete C:\msys64 folder
echo   N = No, keep MSYS2
echo.
set /p REMOVE_MSYS2="Your choice (Y/N): "
if /i "!REMOVE_MSYS2!"=="Y" (
    if exist "C:\msys64" (
        echo [INFO] Removing C:\msys64 (this may take a moment)...
        rmdir /s /q "C:\msys64"
        if %ERRORLEVEL% EQU 0 (
            echo [OK] MSYS2 removed.
        ) else (
            echo [ERROR] Failed to remove C:\msys64. Try running as Administrator.
        )
    ) else (
        echo [SKIP] C:\msys64 not found.
    )

    :: Also remove from user PATH
    echo [INFO] Removing C:\msys64\mingw64\bin from PATH...
    powershell -NoProfile -Command "$p=[Environment]::GetEnvironmentVariable('PATH','User'); $clean=$p -replace ';?C:\\msys64\\mingw64\\bin','' ; [Environment]::SetEnvironmentVariable('PATH',$clean,'User')"
    echo [OK] PATH cleaned.
) else (
    echo [SKIP] Keeping MSYS2.
)
echo.

echo  =====================================================
echo    Uninstall complete!
echo  =====================================================
echo.
pause
endlocal
