@echo off
setlocal

:: Add Cargo to PATH if not present (simplified check)
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"

cd whirlpool

echo Cleaning up previous builds...
:: Optional: cargo clean

echo Building Whirlpool VST3...
cargo build --release --target x86_64-pc-windows-msvc

if %ERRORLEVEL% neq 0 (
    echo Build failed!
    exit /b 1
)

echo Bundling Whirlpool VST3...
:: xtask skipped, using manual bundle directly as it is more reliable for now

echo Creating directories...
if not exist "target\bundled" mkdir "target\bundled"
if not exist "target\bundled\Whirlpool.vst3" mkdir "target\bundled\Whirlpool.vst3"
if not exist "target\bundled\Whirlpool.vst3\Contents" mkdir "target\bundled\Whirlpool.vst3\Contents"
if not exist "target\bundled\Whirlpool.vst3\Contents\x86_64-win" mkdir "target\bundled\Whirlpool.vst3\Contents\x86_64-win"

echo Checking source file...
dir "target\x86_64-pc-windows-msvc\release\whirlpool.dll"

echo Copying file...
copy /Y "target\x86_64-pc-windows-msvc\release\whirlpool.dll" "target\bundled\Whirlpool.vst3\Contents\x86_64-win\Whirlpool.vst3"

if %ERRORLEVEL% neq 0 (
     echo Manual bundle failed!
     exit /b 1
)

echo Done! Module is in hellovstGUI/target/bundled/
endlocal
