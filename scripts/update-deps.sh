#!/bin/bash

# Script para actualizar automáticamente todas las dependencias a la última versión
# Uso: ./scripts/update-deps.sh [--dry-run]

set -e

DRY_RUN=false

# Parsear argumentos
while [[ $# -gt 0 ]]; do
    case $1 in
        --dry-run)
            DRY_RUN=true
            shift
            ;;
        *)
            echo "Uso: $0 [--dry-run]"
            echo "  --dry-run: Muestra los cambios sin aplicarlos"
            exit 1
            ;;
    esac
done

echo "🔄 Actualizando dependencias de RusTale Workspace..."

# Instalar herramientas necesarias si no están instaladas
if ! command -v cargo-edit &> /dev/null; then
    echo "📦 Instalando cargo-edit..."
    cargo install cargo-edit
fi

if ! command -v cargo-workspaces &> /dev/null; then
    echo "� Instalando cargo-workspaces..."
    cargo install cargo-workspaces
fi

# Función para actualizar un crate específico
update_crate() {
    local crate_path=$1
    local crate_name=$2
    
    echo "📦 Actualizando $crate_name..."
    cd "$crate_path"
    
    if [ "$DRY_RUN" = true ]; then
        echo "🔍 Simulación para $crate_name:"
        cargo upgrade --dry-run
    else
        echo "⬆️  Actualizando $crate_name:"
        cargo upgrade
    fi
    
    cd - > /dev/null
    echo ""
}

# Actualizar launcher workspace
echo "📋 Actualizando launcher workspace..."
update_crate "launcher" "launcher workspace"

# Actualizar cada crate individualmente para asegurar cobertura completa
CRATES=("rustale_engine" "rustale_shared" "rustale_iced" "rustale_server" "rustale_tray")

for crate in "${CRATES[@]}"; do
    echo "📦 Actualizando crate individual: $crate"
    update_crate "launcher/crates/$crate" "$crate"
done

# Actualizar security crate y sus macros
echo "📋 Actualizando rustale_security..."
update_crate "rustale_security" "rustale_security"

echo "📋 Actualizando rustale_security_macros..."
update_crate "rustale_security/rustale_security_macros" "rustale_security_macros"

echo ""
echo "🧹 Limpiando caché de Cargo..."
cargo clean

echo ""
echo "✅ Verificando que todo compila..."

# Verificar launcher workspace
echo "🔍 Verificando launcher workspace..."
cd launcher
cargo check
cargo test --no-run
cd ..

# Verificar security crate y macros
echo "🔍 Verificando rustale_security..."
cd rustale_security
cargo check
cargo test --no-run
cd ..

echo "🔍 Verificando rustale_security_macros..."
cd rustale_security/rustale_security_macros
cargo check
cargo test --no-run
cd ../..

echo ""
if [ "$DRY_RUN" = true ]; then
    echo "✅ Simulación completada. Ejecuta sin --dry-run para aplicar los cambios."
else
    echo "✅ Actualización completada exitosamente!"
    echo "💡 Recomendación: Ejecuta 'cargo audit' para verificar seguridad de las nuevas versiones."
fi
