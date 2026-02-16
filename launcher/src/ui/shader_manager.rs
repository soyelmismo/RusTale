use rust_embed::RustEmbed;

// Definimos la ubicacion de los assets relativa al Cargo.toml de 'launcher'
// Como 'assets' esta en la raiz del workspace (un nivel arriba de launcher), usamos "../"
#[derive(RustEmbed)]
#[folder = "../assets/shaders"]
struct ShaderAssets;

/// Estructura comun de Uniforms que se antepone a todos los shaders.
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
    next_shader_id: u32,
    transition: f32,
}
@group(0) @binding(0) var<uniform> u: Uniforms;

// Funcion de ayuda: Rotar 2D
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
    // Correccion para sistemas de coordenadas
    out.uv.y = 1.0 - out.uv.y; 
    return out;
}
"#;

pub fn build_uber_shader() -> String {

    // Cargar el archivo especifico
    let entropy_src = if let Some(file) = ShaderAssets::get("entropy.wgsl") {
        match String::from_utf8(file.data.to_vec()) {
            Ok(s) => s,
            Err(_) => super::lsd_shader::DEFAULT_FALLBACK.to_string(),
        }
    } else {
        super::lsd_shader::DEFAULT_FALLBACK.to_string()
    };

    // The HEADER provides Uniforms, u, VertexOutput, vs_main, and rot()
    // We simply append the logic from entropy.wgsl
    format!("{}\n{}", HEADER, entropy_src)
}

pub fn get_shader_count() -> usize {
    1 // Solo existe el Universo
}

