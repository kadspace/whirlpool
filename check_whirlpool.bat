@echo off
setlocal
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
cd whirlpool
cargo check
endlocal
