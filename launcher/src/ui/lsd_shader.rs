use iced::mouse;
use iced::widget::shader;
use iced::{Color, Point, Rectangle};
pub use iced_renderer::wgpu::wgpu;
use std::borrow::Cow;
use std::time::Instant;
use std::panic::AssertUnwindSafe;

// ==========================================================
// UNIFORMES: La "Memoria compartida" CPU -> GPU
// ==========================================================
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C, align(16))]
struct Uniforms {
    // BLOQUE 1 (16b)
    time: f32,
    aspect: f32,
    mouse_x: f32, // Normalizado -1 a 1
    mouse_y: f32, // Normalizado -1 a 1

    // BLOQUE 2 (16b)
    accent_r: f32,
    accent_g: f32,
    accent_b: f32,
    intensity: f32, // Control global del brillo

    // BLOQUE 3 (16b)
    alpha: f32,     // Opacidad para transiciones
    shader_id: u32, // Que algoritmo usar (0, 1, 2...)
    next_shader_id: u32, // ID Siguiente para transicion
    transition: f32,     // Progreso de transicion 0.0 a 1.0
}

// ==========================================================
// WIDGET PROGRAM
// ==========================================================
#[derive(Debug, Clone)]
pub struct LsdShader {
    start_time: Instant,
    mouse_pos: Point,
    accent: Color,
    shader_id: u32,   // ID del shader actual
    alpha: f32,       // Transparencia (0.0 a 1.0)
    intensity: f32,   // Intensidad dinamica basada en mouse_stillness
    click_intensity: f32, // Pico de intensidad para ondas de choque
    last_click_time: Instant, // Para controlar el decaimiento del pulso
    next_shader_id: u32, // ID del shader siguiente para transiciones
    transition: f32,     // Progreso de transicion (0.0 a 1.0)
}

impl LsdShader {
    pub fn new(
        start_time: Instant,
        mouse_pos: Point,
        accent: Color,
        shader_id: u32,
        alpha: f32,
        intensity: f32,
    ) -> Self {
        Self {
            start_time,
            mouse_pos,
            accent,
            shader_id,
            alpha,
            intensity,
            click_intensity: 0.0,
            last_click_time: Instant::now(),
            next_shader_id: 0,
            transition: 0.0,
        }
    }

    pub fn trigger_click(&mut self) {
        self.click_intensity = 1.3; // Pico fuerte para la onda de choque
        self.last_click_time = Instant::now();
    }

    pub fn update_mouse_position(&mut self, pos: Point) {
        self.mouse_pos = pos;
    }

    pub fn update_shader_id(&mut self, shader_id: u32) {
        self.shader_id = shader_id;
    }

    pub fn update_accent(&mut self, accent: Color) {
        self.accent = accent;
    }

    pub fn update_transition(&mut self, next_id: u32, progress: f32) {
        self.next_shader_id = next_id;
        self.transition = progress;
    }

    pub fn update_alpha(&mut self, alpha: f32) {
        self.alpha = alpha;
    }
}

impl<Message> shader::Program<Message> for LsdShader {
    type State = ();
    type Primitive = LsdPrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: mouse::Cursor,
        bounds: Rectangle,
    ) -> Self::Primitive {
        // Ralentizamos el tiempo para efectos mas hipnoticos
        let time = self.start_time.elapsed().as_secs_f32() * 0.5;
        // Forzamos el calculo de aspect usando los bounds logicos REALES de la ventana
        // que Iced acaba de calcular en el layout pass.
        let aspect = bounds.width / bounds.height.max(1.0);

        // Decaimiento del pulso de clic (exponencial)
        let click_decay = (self.last_click_time.elapsed().as_secs_f32() * 8.0).exp();
        let current_click_intensity = self.click_intensity / click_decay;
        
        // Combinar intensidad base con pulso de clic
        let combined_intensity = self.intensity + current_click_intensity;

        LsdPrimitive {
            uniforms: Uniforms {
                time,
                aspect, // <--- Este valor debe ser reactivo puro a los bounds
                mouse_x: (self.mouse_pos.x / bounds.width) * 2.0 - 1.0,
                mouse_y: (self.mouse_pos.y / bounds.height) * 2.0 - 1.0,
                accent_r: self.accent.r,
                accent_g: self.accent.g,
                accent_b: self.accent.b,
                intensity: combined_intensity.min(10.0), // Limitar para evitar explosiones
                alpha: if self.transition > 0.0 { self.alpha * self.transition } else { self.alpha }, // TIP EXTRA: Durante el resize, reduce el alpha del shader a 0.5 para que el lag visual se note menos.
                shader_id: self.shader_id,
                next_shader_id: self.next_shader_id,
                transition: self.transition,
            },
        }
    }
}

#[derive(Debug, Clone)]
pub struct LsdPrimitive {
    uniforms: Uniforms,
}

impl shader::Primitive for LsdPrimitive {
    type Pipeline = LsdPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        _device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        _viewport: &shader::Viewport,
    ) {
        // Enviar datos a GPU
        queue.write_buffer(&pipeline.buffer, 0, bytemuck::bytes_of(&self.uniforms));
    }

    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        render_pass.set_pipeline(&pipeline.pipeline);
        render_pass.set_bind_group(0, &pipeline.bind_group, &[]);
        render_pass.draw(0..6, 0..1);
        true
    }
}

pub struct LsdPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    buffer: wgpu::Buffer,
}

impl shader::Pipeline for LsdPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        // HACK SEGURO: Usamos `CURRENT_WGSL_CODE` cargado al inicio.
        let source_code = get_global_wgsl();

        // FIX: Envolver el closure en AssertUnwindSafe
        let shader = std::panic::catch_unwind(AssertUnwindSafe(|| {
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Dynamic RusTale Uber-Shader"),
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(&source_code)),
            })
        })).unwrap_or_else(|_| {
            eprintln!("[SHADER] Failed to create shader module! Using fallback.");
            device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("Fallback Shader"),
                source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(DEFAULT_FALLBACK)),
            })
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Lsd Layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<Uniforms>() as u64),
                },
                count: None,
            }],
        });

        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Lsd Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let pipeline = std::panic::catch_unwind(AssertUnwindSafe(|| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Lsd Render Pipeline"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_main"),
                    buffers: &[],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs_main"),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        // IMPORTANTE: Alpha Blending Activado para transiciones fluidas
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING), 
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: wgpu::PipelineCompilationOptions::default(),
                }),
                multiview: None,
                cache: None,
            })
        })).unwrap_or_else(|_| {
            eprintln!("[SHADER] Failed to create render pipeline! Creating minimal pipeline.");
            // FIX: Envolver tambien este bloque en AssertUnwindSafe
            std::panic::catch_unwind(AssertUnwindSafe(|| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("Minimal Fallback Pipeline"),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs_main"),
                        buffers: &[],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs_main"),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING), 
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    multiview: None,
                    cache: None,
                })
            })).unwrap_or_else(|_| {
                eprintln!("[SHADER] Failed to create minimal pipeline! Using default.");
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("Default Pipeline"),
                    layout: Some(&layout),
                    vertex: wgpu::VertexState {
                        module: &shader,
                        entry_point: Some("vs_main"),
                        buffers: &[],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    },
                    primitive: wgpu::PrimitiveState::default(),
                    depth_stencil: None,
                    multisample: wgpu::MultisampleState::default(),
                    fragment: Some(wgpu::FragmentState {
                        module: &shader,
                        entry_point: Some("fs_main"),
                        targets: &[Some(wgpu::ColorTargetState {
                            format,
                            blend: Some(wgpu::BlendState::ALPHA_BLENDING), 
                            write_mask: wgpu::ColorWrites::ALL,
                        })],
                        compilation_options: wgpu::PipelineCompilationOptions::default(),
                    }),
                    multiview: None,
                    cache: None,
                })
            })
        });

        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Lsd Uniforms"),
            size: std::mem::size_of::<Uniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Lsd Bind Group"),
            layout: &bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });

        Self { pipeline, bind_group, buffer }
    }
}

// SISTEMA DE CARGA GLOBAL (Singleton Seguro para Shader source)
use std::sync::OnceLock;
static GLOBAL_WGSL: OnceLock<String> = OnceLock::new();

pub fn set_global_wgsl(code: String) {
    // Solo se permite configurar una vez al inicio del programa.
    // Para recarga en caliente necesitariamos recrear el Pipeline, lo cual Iced hace si cambia el ID.
    let _ = GLOBAL_WGSL.set(code);
}

pub fn set_safe_mode_shader() {
    let _ = GLOBAL_WGSL.set(SAFE_MODE_SHADER.to_string());
}

/// Detecta si el hardware actual podria tener problemas con shaders complejos
pub fn should_use_safe_mode() -> bool {
    // Verificar variables de entorno que podrian indicar problemas
    if std::env::var("RUSTALE_FORCE_SAFE_MODE").is_ok() {
        return true;
    }
    
    // Verificar si estamos en un entorno virtual o contenedor
    if std::env::var("WSL_DISTRO_NAME").is_ok() || 
       std::env::var("DOCKER_CONTAINER").is_ok() {
        println!("[SHADER] Detected virtual environment, enabling safe mode");
        return true;
    }
    
    // Verificar si hay drivers de software conocidos
    if let Ok(adapter) = std::env::var("WGPU_ADAPTER_NAME") {
        let adapter_lower = adapter.to_lowercase();
        if adapter_lower.contains("microsoft") && 
           (adapter_lower.contains("basic") || adapter_lower.contains("warp")) {
            println!("[SHADER] Detected software renderer, enabling safe mode");
            return true;
        }
    }
    
    false
}

pub fn get_global_wgsl() -> &'static str {
    GLOBAL_WGSL.get().map(|s| s.as_str()).unwrap_or(DEFAULT_FALLBACK)
}

// Shader de emergencia por si falla la carga
const DEFAULT_FALLBACK: &str = r#"
@vertex fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    return vec4<f32>(0.0, 0.0, 0.0, 1.0); 
}
@fragment fn fs_main() -> @location(0) vec4<f32> {
    return vec4<f32>(1.0, 0.0, 0.0, 1.0); 
}
"#;

// Shader simple para modo seguro (cuadrado de color solido)
const SAFE_MODE_SHADER: &str = r#"
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

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    // Simple color solido con el accent color
    return vec4<f32>(u.accent_r, u.accent_g, u.accent_b, u.alpha);
}
"#;
