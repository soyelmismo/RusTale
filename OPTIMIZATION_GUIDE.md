# Guía de Optimización de Compilación para RusTale

## 🚀 Optimizaciones Aplicadas

### 1. Configuración de Cargo (`./.cargo/config.toml`)

- **Paralelización**: Configurado para usar 8 cores (ajustable según tu CPU)
- **Compilador nativo**: `target-cpu=native` para desarrollo y release
- **Optimización de macros**: `opt-level = 3` para procedural macros
- **Perfiles especializados**: Configuraciones optimizadas para dev, test, y bench

### 2. Perfiles de Compilación (`Cargo.toml`)

#### Perfil `unopt` (Ultra-rápido)
- `opt-level = 0`: Sin optimización para compilación máxima velocidad
- `codegen-units = 256`: Máxima paralelización
- `debug = 0`: Sin símbolos de debug
- `incremental = true`: Compilación incremental activada

#### Perfil `dev` (Desarrollo balanceado)
- `opt-level = 0`: Compilación rápida
- `debug = 0` + `strip = "debuginfo"`: Sin debug info en binario final
- `overflow-checks = false`: Desactivado para velocidad
- `codegen-units = 16`: Buena paralelización

#### Perfil `test` (Testing optimizado)
- `opt-level = 1`: Ligera optimización para tests más rápidos
- Configuración similar a dev pero optimizada para ejecución de tests

### 3. Optimización de Dependencias

#### Eliminadas (8 dependencias no utilizadas):
- `md5` - 0.8.0
- `memchr` - 2.7.6  
- `memmap2` - 0.9.9
- `portpicker` - 0.1.1
- `rayon` - 1.11.0
- `tao` - 0.34.5
- `thiserror` - 2.0.18
- `tokio-stream` - 0.1.18

#### Optimizadas:
- **Tokio**: Cambiado de `features = ["full"]` a características específicas necesarias
- **Iced**: Mantenidas características necesarias (`advanced` era requerida)

### 4. Herramientas de Análisis Instaladas

- `cargo-machete`: Detecta dependencias no utilizadas
- `cargo-shear`: Análisis avanzado de dependencias  
- `cargo-udeps`: Identifica dependencias innecesarias (requiere nightly)

### 5. Scripts de Desarrollo

#### `./scripts/dev.sh`
- Compilación ultra-rápida con perfil `unopt`
- Directorio target separado (`target-dev`)
- Limpieza automática de cache >2GB
- Opción de ejecución inmediata con `--run`

#### `./scripts/check-deps.sh`
- Análisis completo de dependencias
- Sugerencias de optimización
- Estadísticas de tamaño

### 6. Configuración de VS Code (`.vscode/settings.json`)

- **rust-analyzer** configurado para usar perfil `unopt`
- Variables de entorno optimizadas
- Exclusión de archivos de watch para mejor rendimiento
- Configuración de features y macros

## 📊 Comandos de Uso

### Desarrollo Rápido
```bash
# Compilar ultra-rápido
./scripts/dev.sh

# Compilar y ejecutar
./scripts/dev.sh --run

# Verificación rápida (sin generar binario)
cargo check --profile unopt
```

### Análisis de Dependencias
```bash
# Analizar dependencias no utilizadas
./scripts/check-deps.sh

# Ver árbol de dependencias
cargo tree --depth 1

# Análisis de tamaño
cargo tree | grep "^[├│└]" | head -20
```

### Perfiles Disponibles
```bash
# Desarrollo ultra-rápido
cargo build --profile unopt

# Desarrollo balanceado  
cargo build --profile dev

# Release optimizado
cargo build --profile release

# Testing rápido
cargo test --profile test
```

## 🎯 Mejoras Esperadas

### Tiempos de Compilación
- **Desarrollo**: 40-60% más rápido con perfil `unopt`
- **Testing**: 30-40% más rápido con perfil `test`
- **Incremental**: Mejoras significativas en rebuilds

### Tamaño de Binarios
- **Debug info**: Reducido significativamente
- **Cache**: Mejor gestión con directorios separados

### Experiencia de Desarrollo
- **rust-analyzer**: Respuestas más rápidas
- **IDE**: Menor consumo de recursos
- **Workflow**: Scripts automatizados

## 🔧 Mantenimiento

### Mensual
```bash
# Limpiar cache de compilación
rm -rf target target-dev

# Actualizar herramientas
cargo install-update -a

# Analizar dependencias
./scripts/check-deps.sh
```

### Semanal  
```bash
# Verificar dependencias no utilizadas
cargo machete

# Actualizar Rust
rustup update
```

## ⚠️ Notas Importantes

1. **Perfil `unopt`**: Solo para desarrollo, no para producción
2. **Features de Tokio/Iced**: Mantener solo las necesarias
3. **Cache**: Limpiar periódicamente para evitar crecimiento excesivo
4. **CI/CD**: Considerar configuraciones específicas para builds automatizados

## 🚀 Siguientes Pasos

1. **CI/CD**: Aplicar optimizaciones específicas para GitHub Actions
2. **Docker**: Configurar `cargo-chef` si se usa Docker
3. **Monitoring**: Establecer métricas de tiempo de compilación
4. **Hardware**: Considerar más cores CPU si es posible

---

*Optimizaciones basadas en las mejores prácticas de la comunidad Rust y el artículo "Tips For Faster Rust Compile Times" de corrode.dev*
