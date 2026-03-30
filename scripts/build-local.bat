@echo off
setlocal

:: Get version from Cargo.toml (script runs from repo root)
for /f "tokens=3 delims= " %%v in ('findstr /r "^version" Cargo.toml') do (
    set VERSION=%%v
)
set VERSION=%VERSION:"=%

set TARGET=x86_64-pc-windows-msvc
set BINARY_NAME=tina4-windows-amd64.exe
set TAG=v%VERSION%

echo === Tina4 v%VERSION% — Build, Sign and Release ===
echo.

:: Ask whether to also tag and release
set /p RELEASE="Tag and push v%VERSION% to GitHub? (y/n): "

if /i "%RELEASE%"=="y" (
    echo.
    echo === Tagging v%VERSION% ===
    git tag %TAG%
    if errorlevel 1 (
        echo Tag already exists or git failed. Continuing with local build only.
    ) else (
        git push origin %TAG%
        if errorlevel 1 (
            echo Push failed.
            exit /b 1
        )
        echo Tag pushed. CI will build Linux and macOS binaries.
    )
)

echo.
echo === Building Windows binary ===
cargo build --release
if errorlevel 1 (
    echo Build failed.
    exit /b 1
)

echo === Copying binary ===
copy /Y "target\release\tina4.exe" "%BINARY_NAME%"
if errorlevel 1 (
    echo Copy failed.
    exit /b 1
)

echo === Signing binary ===
powershell -Command "& 'D:\projects\sign.bat' '%CD%\%BINARY_NAME%'"
if errorlevel 1 (
    echo Signing failed.
    exit /b 1
)

echo === Packaging ===
if exist "%BINARY_NAME%.zip" del "%BINARY_NAME%.zip"
powershell -Command "Compress-Archive -Path '%BINARY_NAME%' -DestinationPath '%BINARY_NAME%.zip'"
if errorlevel 1 (
    echo Packaging failed.
    exit /b 1
)

if /i "%RELEASE%"=="y" (
    echo.
    echo === Uploading to GitHub Release %TAG% ===
    gh release upload %TAG% "%BINARY_NAME%" "%BINARY_NAME%.zip" --clobber
    if errorlevel 1 (
        echo Upload failed. You can upload manually with:
        echo   gh release upload %TAG% %BINARY_NAME% %BINARY_NAME%.zip
        exit /b 1
    )
    echo Uploaded successfully.
)

echo.
echo Done: %BINARY_NAME% and %BINARY_NAME%.zip
endlocal
