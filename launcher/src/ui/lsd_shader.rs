use iced::mouse;

use iced::widget::shader;

use iced::{Color, Point, Rectangle};

pub use iced_renderer::wgpu::wgpu;

use std::borrow::Cow;

use std::time::Instant;



// [GPU OPTIMIZATION]

// Diseño de bloque de uniformes compatible con alineacion de 16 bytes.

// 48 bytes total (3 bloques de 16).

#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]

#[repr(C, align(16))]

struct Uniforms {

    time: f32,      // 4

    width: f32,     // 8

    height: f32,    // 12

    mouse_x: f32,   // 16 -> Fin bloque 1

    mouse_y: f32,   // 20

    intensity: f32, // 24

    accent_r: f32,  // 28

    accent_g: f32,  // 32 -> Fin bloque 2

    accent_b: f32,  // 36

    bg_r: f32,      // 40

    bg_g: f32,      // 44

    bg_b: f32,      // 48 -> Fin bloque 3

}



pub struct LiquidLsd {

    start_time: Instant,

    mouse_pos: Point,

    accent: Color,

    bg: Color,

    intensity: f32, // Factor progresivo 0.0 - 1.0

}



impl LiquidLsd {

    pub fn new(

        start_time: Instant,

        mouse_pos: Point,

        accent: Color,

        bg: Color,

        intensity: f32,

    ) -> Self {

        Self {

            start_time,

            mouse_pos,

            accent,

            bg,

            intensity,

        }

    }

}



impl<Message> shader::Program<Message> for LiquidLsd {

    type State = ();

    type Primitive = LsdPrimitive;



    fn draw(

        &self,

        _state: &Self::State,

        _cursor: mouse::Cursor,

        bounds: Rectangle,

    ) -> Self::Primitive {

        let time = self.start_time.elapsed().as_secs_f32();



        // El pulso de intensidad depende del tiempo Y del factor progresivo

        let raw_intensity = 0.8 + (time * 0.5).sin() * 0.2;

        let intensity = raw_intensity * self.intensity;



        LsdPrimitive {

            uniforms: Uniforms {

                time,

                width: bounds.width,

                height: bounds.height,

                mouse_x: self.mouse_pos.x,

                mouse_y: self.mouse_pos.y,

                intensity,

                accent_r: self.accent.r,

                accent_g: self.accent.g,

                accent_b: self.accent.b,

                bg_r: self.bg.r,

                bg_g: self.bg.g,

                bg_b: self.bg.b,

            },

        }

    }

}



#[derive(Debug, Clone, Copy)]

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

        // [GPU MEMORY] Escribimos directamente al buffer mapeado.

        queue.write_buffer(&pipeline.buffer, 0, bytemuck::bytes_of(&self.uniforms));

    }



    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {

        render_pass.set_pipeline(&pipeline.pipeline);

        render_pass.set_bind_group(0, &pipeline.bind_group, &[]);

        render_pass.draw(0..6, 0..1); // Dibujamos 6 vertices (2 triangulos = 1 quad full screen)

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

        // Compilacion de Shader:

        // [GPU OPTIMIZATION] Usamos WGSL precompilado/validado en runtime.

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {

            label: Some("Acid Crystal Shader"),

            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER_SOURCE)),

        });



        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {

            label: Some("Lsd Layout"),

            entries: &[wgpu::BindGroupLayoutEntry {

                binding: 0,

                // El vertex shader necesita uniforms, el fragment tambien.

                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,

                ty: wgpu::BindingType::Buffer {

                    ty: wgpu::BufferBindingType::Uniform,

                    has_dynamic_offset: false,

                    // Tamaño minimo debe coincidir exactamente con el struct align(16)

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



        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {

            label: Some("Lsd Render Pipeline"),

            layout: Some(&layout),

            vertex: wgpu::VertexState {

                module: &shader,

                entry_point: Some("vs_main"),

                buffers: &[],

                compilation_options: wgpu::PipelineCompilationOptions::default(),

            },

            primitive: wgpu::PrimitiveState::default(), // Topology triangle list

            depth_stencil: None,

            multisample: wgpu::MultisampleState::default(),

            fragment: Some(wgpu::FragmentState {

                module: &shader,

                entry_point: Some("fs_main"),

                targets: &[Some(wgpu::ColorTargetState {

                    format,

                    // REEMPLAZAR pixels existentes, no mezclar (blending desactivado ahorra computo para fondos)

                    blend: Some(wgpu::BlendState::REPLACE),

                    write_mask: wgpu::ColorWrites::ALL,

                })],

                compilation_options: wgpu::PipelineCompilationOptions::default(),

            }),

            multiview: None,

            cache: None,

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



        Self {

            pipeline,

            bind_group,

            buffer,

        }

    }

}



const SHADER_SOURCE: &str = r#"

struct Uniforms {

    time: f32,

    width: f32,

    height: f32,

    mouse_x: f32,

    mouse_y: f32,

    intensity: f32,

    accent_r: f32,

    accent_g: f32,

    accent_b: f32,

    bg_r: f32,

    bg_g: f32,

    bg_b: f32,

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

    let t = u.time * 0.15;

    let aspect = u.width / u.height;

    

    var uv = in.uv;

    uv.x *= aspect;

    

    let mouse_uv = vec2<f32>(u.mouse_x / u.width * aspect, u.mouse_y / u.height);

    

    // Simple rotating grid pattern (inline calculations)

    let angle = t;

    let c = cos(angle);

    let s = sin(angle);

    

    // Simple 2D rotation simulation

    let rot_x = uv.x * c - uv.y * s * 0.5;

    let rot_y = uv.x * s * 0.5 + uv.y * c;

    

    // Create grid lines

    let grid_scale = 8.0;

    let grid_x = fract(rot_x * grid_scale);

    let grid_y = fract(rot_y * grid_scale);

    

    // Simple line detection

    let line_x = smoothstep(0.45, 0.55, grid_x) + smoothstep(0.45, 0.55, 1.0 - grid_x);

    let line_y = smoothstep(0.45, 0.55, grid_y) + smoothstep(0.45, 0.55, 1.0 - grid_y);

    let lines = (line_x + line_y) * 0.5;

    

    // Theme colors

    let accent_color = vec3<f32>(u.accent_r, u.accent_g, u.accent_b);

    let bg_color = vec3<f32>(u.bg_r, u.bg_g, u.bg_b);

    

    // Mix colors with grid pattern

    var final_color = mix(bg_color, accent_color, lines * u.intensity * 0.6);

    

    // Simple mouse glow

    let d = distance(uv, mouse_uv);

    let m_glow = exp(-d * 1.0) * u.intensity;

    final_color += accent_color * m_glow;

    

    // Simple vignette

    let vignette = smoothstep(1.0, 0.7, distance(in.uv, vec2<f32>(0.5)));

    

    return vec4<f32>(final_color * (vignette * 0.9 + 0.1), 1.0);

}

"#;

