use std::env;
use std::path::PathBuf;
use std::process::Command;

fn main() {
    // -----------------------------------------------------------
    // [CRITICAL] VIGILAR CARPETA DE SHADERS
    // Si algún archivo .wgsl cambia, se añade o se elimina, 
    // se fuerza la recompilación para actualizar rust-embed.
    println!("cargo:rerun-if-changed=../assets/shaders");
    // -----------------------------------------------------------

    // Compilar recursos de Windows (icono y version)
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/logo.ico");
        res.compile().expect("Failed to compile Windows resources");
    }

    // Detectar si estamos compilando en modo release o debug
    let profile = env::var("PROFILE").unwrap();
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let root_dir = PathBuf::from(manifest_dir).parent().unwrap().to_path_buf();
    let aurora_dir = root_dir.join("aurora");

    // Detectar extension segun sistema operativo
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let (lib_name, ext) = match target_os.as_str() {
        "windows" => ("aurora", "dll"),
        "linux" => ("libaurora", "so"),
        "macos" => ("libaurora", "dylib"),
        _ => panic!("Unsupported OS"),
    };

    println!("cargo:rerun-if-changed=../aurora/src/lib.rs");
    println!("cargo:rerun-if-changed=../aurora/Cargo.toml");

    // Definir carpeta de compilacion separada para evitar DEADLOCK
    // Usaremos target/aurora_build en lugar de target/
    let aurora_target_dir = root_dir.join("target").join("aurora_build");

    // Construir el comando
    let mut cmd = Command::new("cargo");
    cmd.arg("build")
        .arg("--lib")
        .current_dir(&aurora_dir)
        .env("CARGO_TARGET_DIR", &aurora_target_dir); // <--- ESTO EVITA EL BLOQUEO

    // Si el launcher se compila en release, compilamos aurora en release
    if profile == "release" {
        cmd.arg("--release");
    }

    let status = cmd.status().expect("Failed to run cargo for aurora");

    if !status.success() {
        panic!("Aurora library build failed!");
    }

    // Ruta donde quedo el DLL compilado (dentro de aurora_build)
    // Nota: Cargo siempre pone los artefactos en 'debug' o 'release' dentro del target dir
    let lib_out_dir = aurora_target_dir.join(&profile);
    let lib_path = lib_out_dir.join(format!("{}.{}", lib_name, ext));

    // Copiar a la carpeta de salida del build script (OUT_DIR) para embeber
    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = PathBuf::from(out_dir).join("aurora_embed.bin");

    if lib_path.exists() {
        std::fs::copy(&lib_path, &dest_path).expect("Failed to copy compiled aurora lib");
        println!("cargo:warning=Aurora embedded from {:?}", lib_path);
    } else {
        panic!(
            "Could not find compiled aurora library at {:?}. Files in dir: {:?}",
            lib_path,
            std::fs::read_dir(lib_out_dir)
        );
    }
}
