@echo off
echo Compiling RusTale...

wsl bash -c "cd /mnt/e/Documentos/Code/RusTale && source ~/.cargo/env && cargo build --profile unopt"

if %ERRORLEVEL% EQU 0 (
    echo Build successful!
    echo Creating distribution...
    wsl bash -c "cd /mnt/e/Documentos/Code/RusTale && mkdir -p dist && cp target/unopt/rustale dist/ && cp target/unopt/aurora.so dist/ && cp -r launcher/assets dist/ && cp -r assets dist/ && echo 'Files in dist:' && ls -la dist/"
) else (
    echo Build failed
)

pause
