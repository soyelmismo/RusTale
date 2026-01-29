use rust_embed::RustEmbed;

// Definimos la ubicación de los assets relativa al Cargo.toml de 'launcher'
// Como 'assets' está en la raíz del workspace (un nivel arriba de launcher), usamos "../"
#[derive(RustEmbed)]
#[folder = "../assets/shaders"]
struct ShaderAssets;

/// Estructura común de Uniforms que se antepone a todos los shaders.
/// Garantiza que los shaders externos no necesiten definirla y evita errores.
const HEADER: &str = r#"
struct Uniforms {
    time: f32,
    aspect: f32,
    mouse_x: f32,
    mouse_y: f32,
    accent_r: f32,
    accent_g: f32,
    accent_b: f32,
    intensity: f32,
    alpha: f32,
    shader_id: u32,
}
@group(0) @binding(0) var<uniform> u: Uniforms;

// Función de ayuda: Rotar 2D
fn rot(a: f32) -> mat2x2<f32> {
    let c = cos(a);
    let s = sin(a);
    return mat2x2<f32>(vec2<f32>(c, s), vec2<f32>(-s, c));
}

// Vertex Shader Estandar (Cuadrado pantalla completa)
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(@builtin(vertex_index) v_index: u32) -> VertexOutput {
    var vertices = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
        vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0), vec2<f32>(-1.0, 1.0)
    );
    let pos = vertices[v_index];
    var out: VertexOutput;
    out.position = vec4<f32>(pos, 0.0, 1.0);
    out.uv = pos * 0.5 + 0.5; // UV 0..1
    // Corrección para sistemas de coordenadas
    out.uv.y = 1.0 - out.uv.y; 
    return out;
}
"#;

pub fn build_uber_shader() -> String {
    let mut shader_functions = String::new(); // Código que va dentro de shader_N
    let mut global_helpers = String::new();   // Funciones auxiliares globales
    let mut switch_cases = String::new();
    
    println!("[Shader] Building embedded uber shader...");

    // Importante: Ordenar archivos para garantizar ID deterministas
    let mut filenames: Vec<String> = ShaderAssets::iter()
        .map(|f| f.into_owned())
        .filter(|name| name.ends_with(".wgsl"))
        .collect();
    
    filenames.sort(); 

    println!("[Shader] Found {} embedded shaders", filenames.len());

    let mut id_counter = 0;

    for filename in filenames {
        if let Some(file) = ShaderAssets::get(&filename) {
            if let Ok(content) = std::str::from_utf8(file.data.as_ref()) {
                println!("[Shader] Registering ID {}: {}", id_counter, filename);
                
                let mut safe_content = content.to_string();
                safe_content = safe_content.replace("atan2(", "atan(");
                
                // --- CORE FIX: Separar Helpers del Main Body ---
                // WGSL prohíbe definir funciones dentro de funciones.
                // Buscamos el separador usado en forest.wgsl o asumimos todo es body.
                
                let (helpers, body) = if let Some(idx) = safe_content.find("// --- MAIN SHADER ---") {
                    let (h, b) = safe_content.split_at(idx);
                    (h, b)
                } else {
                    ("", safe_content.as_str())
                };

                // 1. Agregar Helpers al scope global (si hay)
                if !helpers.trim().is_empty() {
                    global_helpers.push_str(&format!("// Helpers form {}\n", filename));
                    global_helpers.push_str(helpers);
                    global_helpers.push_str("\n");
                }

                // 2. Encapsular el Body en la función única shader_X
                let func_name = format!("shader_{}", id_counter);
                
                shader_functions.push_str(&format!(
                    "// Body of: {}\nfn {}(in: VertexOutput) -> vec4<f32> {{\n{}\n}}\n", 
                    filename, func_name, body
                ));
                
                switch_cases.push_str(&format!("        case {}u: {{ return {}(in); }}\n", id_counter, func_name));
                id_counter += 1;
            }
        }
    }

    // Estructura final:
    // 1. HEADER (Structs, Uniforms)
    // 2. GLOBAL HELPERS (Funciones extraídas de forest.wgsl)
    // 3. SHADER FUNCTIONS (Los cuerpos principales envueltos en fn shader_N)
    // 4. FS_MAIN (Switch gigante)
    let result = format!("{}\n{}\n{}\n@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {{
    var col = vec4<f32>(0.0);
    switch (u.shader_id) {{
{}
        default: {{ col = shader_0(in); }}
    }}
    // Aplicar mezcla de alpha global aquí (pre-multiplicado)
    return vec4<f32>(col.rgb, col.a * u.alpha);
}}
", HEADER, global_helpers, shader_functions, switch_cases);

    result
}

pub fn get_shader_count() -> usize {
    let external_count = ShaderAssets::iter()
        .map(|f| f.into_owned())
        .filter(|name| name.ends_with(".wgsl"))
        .count();
    external_count
}

