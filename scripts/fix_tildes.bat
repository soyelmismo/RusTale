@echo off
echo Replacing accented vowels in .rs files...

setlocal enabledelayedexpansion
set count=0
set changed=0

REM Process only .rs files recursively
for /r "." %%f in (*.rs) do (
    REM Skip target directory
    echo %%f | findstr /i /c:"\\target\\" >nul
    if errorlevel 1 (
        echo Checking: %%f
        
        REM Create temp file with same timestamps
        set "tempfile=%%~dpnf_temp%%~xf"
        
        REM Copy original timestamps to temp file first
        copy "%%f" "!tempfile!" >nul 2>&1
        
        REM Use PowerShell but preserve original file attributes
        powershell -Command ^
            "$original = Get-Content -Path '%%f' -Raw -Encoding UTF8; " ^
            "$modified = $original -replace 'á', 'á' -replace 'é', 'é' -replace 'í', 'í' -replace 'ó', 'ó' -replace 'ú', 'ú' -replace 'Á', 'Á' -replace 'É', 'É' -replace 'Í', 'Í' -replace 'Ó', 'Ó' -replace 'Ú', 'Ú' -replace 'ñ', 'ñ' -replace 'Ñ', 'Ñ' -replace 'ü', 'ü' -replace 'Ü', 'Ü'; " ^
            "if ($original -ne $modified) { " ^
            "    Set-Content -Path '!tempfile!' -Value $modified -Encoding UTF8 -NoNewline; " ^
            "    exit 1; " ^
            "} else { " ^
            "    exit 0; " ^
            "}"
        
        REM Check if PowerShell made changes
        if !errorlevel! equ 1 (
            REM Only replace if content actually changed
            move /y "!tempfile!" "%%f" >nul 2>&1
            echo   -> MODIFIED
            set /a changed+=1
        ) else (
            REM No changes needed, delete temp file
            del "!tempfile!" >nul 2>&1
            echo   -> unchanged
        )
        
        set /a count+=1
    )
)

echo.
echo Checked %count% .rs files
echo Actually modified %changed% files
echo Done!
pause
