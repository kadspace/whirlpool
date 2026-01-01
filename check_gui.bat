@echo off
setlocal

:: Add Cargo to PATH if not present
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"

call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"

cd hellovstGUI
cargo check

endlocal
