use std::env;
use std::fs;
use std::path::{PathBuf};
use std::process::Command;
use sha2::{Sha256, Digest};

fn main() {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap();
    let root_dir = PathBuf::from(manifest_dir).parent().unwrap().to_path_buf();
    
    // --- 1. VIGILANCIA DE SHADERS (Bulletproof) ---
    // Ruta absoluta a los assets
    let shaders_dir = root_dir.join("assets").join("shaders");

    println!("cargo:rerun-if-changed={}", shaders_dir.display());

    // Iteramos recursivamente (o plano) para marcar CADA archivo .wgsl
    if let Ok(entries) = fs::read_dir(&shaders_dir) {
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                // Si es un archivo y termina en .wgsl
                if path.is_file() {
                    if let Some(ext) = path.extension() {
                        if ext == "wgsl" {
                            // Esta linea le grita a Cargo: "¡Si este archivo se toca, recompila!"
                            println!("cargo:rerun-if-changed={}", path.display());
                        }
                    }
                }
            }
        }
    }
    // ----------------------------------------------

    // Compilar recursos de Windows (icono y version)
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("../assets/logo.ico");
        res.compile().expect("Failed to compile Windows resources");
    }

    // Detectar perfil
    let profile = env::var("PROFILE").unwrap();
    let aurora_dir = root_dir.join("aurora");

    // Detectar OS y extension
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    let (lib_name, ext) = match target_os.as_str() {
        "windows" => ("aurora", "dll"),
        "linux" => ("libaurora", "so"),
        "macos" => ("libaurora", "dylib"),
        _ => panic!("Unsupported OS"),
    };

    println!("cargo:rerun-if-changed=../aurora/src/lib.rs");
    println!("cargo:rerun-if-changed=../aurora/Cargo.toml");

    let aurora_target_dir = root_dir.join("target").join("aurora_build");

    let mut cmd = Command::new("cargo");
    cmd.arg("build")
        .arg("--lib")
        .current_dir(&aurora_dir)
        .env("CARGO_TARGET_DIR", &aurora_target_dir);

    if profile == "release" {
        cmd.arg("--release");
    }

    let status = cmd.status().expect("Failed to run cargo for aurora");

    if !status.success() {
        panic!("Aurora library build failed!");
    }

    let lib_out_dir = aurora_target_dir.join(&profile);
    let lib_path = lib_out_dir.join(format!("{}.{}", lib_name, ext));

    // Obtener el directorio de construcción del launcher (target/debug o target/release)
    let launcher_target_dir = root_dir.join("target").join(&profile);
    
    // Asegurar que el directorio de construcción del launcher exista
    if !launcher_target_dir.exists() {
        fs::create_dir_all(&launcher_target_dir).expect("Failed to create launcher target directory");
    }

    if lib_path.exists() {
        // Copiar el binario compilado al directorio de construcción del launcher
        let dest_path = launcher_target_dir.join(format!("aurora.{}", ext));
        std::fs::copy(&lib_path, &dest_path).expect("Failed to copy compiled aurora lib to launcher target");
        
        // Calcular SHA-256 del binario Aurora
        let aurora_bytes = fs::read(&lib_path).expect("Failed to read aurora binary for checksum");
        let mut hasher = Sha256::new();
        hasher.update(&aurora_bytes);
        let checksum = format!("{:x}", hasher.finalize());
        
        // Pasar el checksum como variable de entorno al compilador
        println!("cargo:rustc-env=AURORA_CHECKSUM={}", checksum);
        
        println!("Aurora library copied to: {:?}", dest_path);
        println!("Aurora checksum embedded: {}", checksum);
    } else {
        panic!(
            "Could not find compiled aurora library at {:?}.",
            lib_path
        );
    }
}
