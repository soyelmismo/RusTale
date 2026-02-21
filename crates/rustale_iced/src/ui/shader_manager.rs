use rust_embed::RustEmbed;
use std::sync::OnceLock;

#[derive(RustEmbed)]
#[folder = "../../assets/shaders"]
struct ShaderAssets;

static SHADER_CACHE: OnceLock<Vec<String>> = OnceLock::new();

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

fn rot(a: f32) -> mat2x2<f32> {
    let c = cos(a);
    let s = sin(a);
    return mat2x2<f32>(vec2<f32>(c, s), vec2<f32>(-s, c));
}

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
    out.uv = pos * 0.5 + 0.5; 
    out.uv.y = 1.0 - out.uv.y; 
    return out;
}
"#;

fn load_all_shaders() -> Vec<String> {
    let mut shaders = Vec::new();
    let shader_files = vec!["mandelbulb.wgsl", "entropy.wgsl"];

    for file_name in shader_files {
        if let Some(file) = ShaderAssets::get(file_name) {
            match String::from_utf8(file.data.to_vec()) {
                Ok(shader_code) => {
                    println!("[SHADER] Loaded: {}", file_name);
                    shaders.push(shader_code);
                }
                Err(e) => {
                    eprintln!("[SHADER] Failed to load {}: {}", file_name, e);
                    shaders.push(crate::ui::lsd_shader::DEFAULT_FALLBACK.to_string());
                }
            }
        } else {
            eprintln!("[SHADER] Shader file not found: {}", file_name);
            shaders.push(crate::ui::lsd_shader::DEFAULT_FALLBACK.to_string());
        }
    }

    if shaders.is_empty() {
        println!("[SHADER] No shaders loaded, using fallback");
        shaders.push(crate::ui::lsd_shader::DEFAULT_FALLBACK.to_string());
    }

    shaders
}

pub fn build_uber_shader_with_index(shader_index: usize) -> String {
    let shaders = SHADER_CACHE.get_or_init(|| load_all_shaders());
    let shader_src = if shader_index < shaders.len() {
        &shaders[shader_index]
    } else {
        println!(
            "[SHADER] Invalid shader index {}, falling back to index 0",
            shader_index
        );
        &shaders[0]
    };
    format!("{}\n{}", HEADER, shader_src)
}

pub fn get_shader_count() -> usize {
    let shaders = SHADER_CACHE.get_or_init(|| load_all_shaders());
    shaders.len()
}
