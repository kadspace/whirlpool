
@echo off
setlocal


:: Add Cargo to PATH if not present (simplified check)
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
cd hello_vst
cargo check
endlocal
