#!/usr/bin/env pwsh

# Script para actualizar automáticamente todas las dependencias a la última versión
# Uso: ./update-deps.ps1 [-DryRun]

param(
    [switch]$DryRun
)

$ErrorActionPreference = "Stop"

Write-Host "🔄 Actualizando dependencias de RusTale Workspace..." -ForegroundColor Cyan

# Función para verificar si un comando cargo existe
function Test-CargoCommand {
    param([string]$Command)
    try {
        $result = cargo $Command --version 2>$null
        return $true
    }
    catch {
        return $false
    }
}

# Instalar herramientas necesarias si no están instaladas
if (-not (Test-CargoCommand "upgrade")) {
    Write-Host "📦 Instalando cargo-edit..." -ForegroundColor Yellow
    cargo install cargo-edit
} else {
    Write-Host "✅ cargo-edit ya está instalado" -ForegroundColor Green
}

if (-not (Test-CargoCommand "workspaces")) {
    Write-Host "📦 Instalando cargo-workspaces..." -ForegroundColor Yellow
    cargo install cargo-workspaces
} else {
    Write-Host "✅ cargo-workspaces ya está instalado" -ForegroundColor Green
}

# Función para actualizar un crate específico
function Update-Crate {
    param(
        [string]$CratePath,
        [string]$CrateName
    )
    
    Write-Host "📦 Actualizando $CrateName..." -ForegroundColor Green
    Push-Location $CratePath
    
    try {
        if ($DryRun) {
            Write-Host "🔍 Simulación para ${CrateName}:" -ForegroundColor Blue
            cargo upgrade --dry-run
        } else {
            Write-Host "⬆️  Actualizando ${CrateName}:" -ForegroundColor Green
            cargo upgrade
        }
    }
    finally {
        Pop-Location
    }
    Write-Host ""
}

# Actualizar launcher workspace
Write-Host "📋 Actualizando launcher workspace..." -ForegroundColor Cyan
Update-Crate "launcher" "launcher workspace"

# Actualizar cada crate individualmente para asegurar cobertura completa
$CRATES = @("rustale_engine", "rustale_shared", "rustale_iced", "rustale_server", "rustale_tray", "aurora", "auth_server")

foreach ($crate in $CRATES) {
    Write-Host "📦 Actualizando crate individual: $crate" -ForegroundColor Green
    if ($crate -eq "aurora") {
        Update-Crate "launcher/aurora" $crate
    } elseif ($crate -eq "auth_server") {
        Update-Crate "launcher/auth_server" $crate
    } else {
        Update-Crate "launcher/crates/$crate" $crate
    }
}

# Actualizar security crate y sus macros
Write-Host "📋 Actualizando rustale_security..." -ForegroundColor Cyan
Update-Crate "rustale_security" "rustale_security"

Write-Host "📋 Actualizando rustale_security_macros..." -ForegroundColor Cyan
Update-Crate "rustale_security/rustale_security_macros" "rustale_security_macros"

Write-Host ""
if ($DryRun) {
    Write-Host "✅ Simulación completada. Ejecuta sin -DryRun para aplicar los cambios." -ForegroundColor Green
} else {
    Write-Host "✅ Actualización completada exitosamente!" -ForegroundColor Green
    Write-Host "💡 Recomendación: Ejecuta 'cargo audit' para verificar seguridad de las nuevas versiones." -ForegroundColor Cyan
}
