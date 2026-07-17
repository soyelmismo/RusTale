#!/bin/bash
set -e

cd "$(dirname "$0")"

echo "=== Compilando RusTale ==="

# Prevenir errores con el crate 'ring' en entornos restrictivos
export RUSTFLAGS="-C target-cpu=x86-64-v2"

if [ "$1" == "--release" ]; then
    echo "Construyendo versión RELEASE (optimizado, toma más tiempo)..."
    cargo build --release
    echo "✔ Listo! Ejecutable en: target/release/rustale"
else
    echo "Construyendo versión DEBUG (desarrollo rápido)..."
    cargo build
    echo "✔ Listo! Ejecutable en: target/debug/rustale"
fi
