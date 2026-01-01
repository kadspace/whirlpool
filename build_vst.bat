@echo off
setlocal

:: Add Cargo to PATH if not present (simplified check)
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

:: Initialize VS Build Tools environment
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"

if %ERRORLEVEL% neq 0 (
    echo Error: Could not initialize Visual Studio environment.
    echo Please ensure Visual Studio Build Tools 2022 are installed.
    exit /b 1
)

cd hello_vst

echo Cleaning up previous builds...
:: Optional: cargo clean


echo Building VST3...
cargo build --release --target x86_64-pc-windows-msvc

if %ERRORLEVEL% neq 0 (
    echo Build failed!
    exit /b 1
)

echo Bundling VST3...
cargo run --package xtask -- bundle --release --target x86_64-pc-windows-msvc

if %ERRORLEVEL% neq 0 (
    echo Xtask bundle failed. Attempting manual bundle...
    
    echo Creating directories...
    if not exist "target\bundled" mkdir "target\bundled"
    if not exist "target\bundled\HelloVst.vst3" mkdir "target\bundled\HelloVst.vst3"
    if not exist "target\bundled\HelloVst.vst3\Contents" mkdir "target\bundled\HelloVst.vst3\Contents"
    if not exist "target\bundled\HelloVst.vst3\Contents\x86_64-win" mkdir "target\bundled\HelloVst.vst3\Contents\x86_64-win"
    
    echo Checking source file...
    dir "target\x86_64-pc-windows-msvc\release\hello_vst.dll"

    echo Copying file...
    copy /Y "target\x86_64-pc-windows-msvc\release\hello_vst.dll" "target\bundled\HelloVst.vst3\Contents\x86_64-win\HelloVst.vst3"
    
    if %ERRORLEVEL% neq 0 (
         echo Manual bundle failed!
         exit /b 1
    )
)

echo Done! Module is in hello_vst/target/bundled/
endlocal
