#!/bin/bash

# Script para análisis y optimización de dependencias
# Ejecutar periódicamente para mantener el proyecto optimizado

set -e

echo "🔍 Analizando dependencias de RusTale..."

echo -e "\n📊 Árbol de dependencias (nivel 1):"
cargo tree --depth 1

echo -e "\n🔪 Buscando dependencias no utilizadas:"
echo "=== cargo-machete ==="
cargo machete || true

echo -e "\n✂️ cargo-shear analysis:"
cargo shear || true

echo -e "\n🧹 cargo-udeps (requiere nightly):"
if cargo +nightly --version >/dev/null 2>&1; then
    cargo +nightly udeps || true
else
    echo "⚠️  Rust nightly no instalado. Ejecuta: rustup install nightly"
fi

echo -e "\n📈 Análisis de tamaño de dependencias:"
echo "=== Las 10 dependencias más grandes ==="
cargo tree | grep "^[├│└]" | head -20

echo -e "\n💡 Sugerencias de optimización:"
echo "- Considera reemplazar reqwest con ureq para HTTP simple"
echo "- Evalúa si necesitas todas las características de tokio"
echo "- Revisa si iced puede usar menos características"
echo "- Considera usar miniserde en lugar de serde para casos simples"

echo -e "\n✅ Análisis completado"
