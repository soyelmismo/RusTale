#!/bin/bash

# Script de desarrollo rápido para RusTale
# Optimizado para compilaciones ultrarrápidas durante desarrollo

set -e

echo "🚀 Iniciando compilación de desarrollo optimizada..."

# Variables de entorno para compilación rápida
export CARGO_INCREMENTAL=1
export CARGO_TARGET_DIR="$PWD/target-dev"
export RUSTFLAGS="-C target-cpu=native"
export RUST_LOG=debug

# Limpiar cache si es muy grande (>2GB)
if [ -d "$CARGO_TARGET_DIR" ]; then
    SIZE=$(du -sb "$CARGO_TARGET_DIR" | cut -f1)
    if [ "$SIZE" -gt 2147483648 ]; then
        echo "🧹 Limpiando cache de compilación (>2GB)..."
        rm -rf "$CARGO_TARGET_DIR"
    fi
fi

# Compilación rápida
echo "📦 Compilando con perfil de desarrollo optimizado..."
cargo build --profile unopt

echo "✅ Compilación completada en $(date)"
echo "📊 Estadísticas del target:"
du -sh "$CARGO_TARGET_DIR" 2>/dev/null || echo "Target directory no encontrado"

# Opcional: ejecutar inmediatamente
if [ "$1" = "--run" ]; then
    echo "🎮 Ejecutando aplicación..."
    "$CARGO_TARGET_DIR/unopt/rustale"
fi
