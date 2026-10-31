@echo off
REM Compila Grabbit per Windows. Prerequisito: Rust da https://rustup.rs
REM (durante l'installazione accetta anche gli "strumenti di compilazione C++ di Visual Studio").
cd /d "%~dp0\..\.."
cargo build --release || exit /b 1
copy /Y target\release\grabbit.exe grabbit-windows-x86_64.exe
echo.
echo Fatto: %CD%\grabbit-windows-x86_64.exe
