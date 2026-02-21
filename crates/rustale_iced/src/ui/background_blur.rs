use iced::Rectangle;
use iced::widget::shader;
pub use iced_renderer::wgpu::wgpu;
use std::borrow::Cow;
use std::sync::Arc;

// ==========================================================
// UNIFORMS FOR NATIVE BLUR
// ==========================================================

#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
#[repr(C, align(16))]
struct BlurUniforms {
    blur_amount: f32,     // 0.0 to 1.0
    time: f32,            // Animation time
    resolution: [f32; 2], // Canvas resolution
}

// ==========================================================
// WIDGET PROGRAM FOR BLUR
// ==========================================================

#[derive(Debug, Clone)]
pub struct BackgroundBlur {
    blur_amount: f32,
    image_data: Arc<Vec<u8>>,
    current_time: f32,
    last_uploaded: Arc<std::sync::atomic::AtomicBool>,
}

impl BackgroundBlur {
    pub fn new(image_bytes: Vec<u8>) -> Self {
        Self {
            blur_amount: 0.7, // Blur cremoso
            image_data: Arc::new(image_bytes),
            last_uploaded: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            current_time: 0.0,
        }
    }
}

impl<Message> shader::Program<Message> for BackgroundBlur {
    type State = ();
    type Primitive = BlurPrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: iced::mouse::Cursor,
        bounds: Rectangle,
    ) -> Self::Primitive {
        BlurPrimitive {
            uniforms: BlurUniforms {
                blur_amount: self.blur_amount,
                time: self.current_time,
                resolution: [bounds.width, bounds.height],
            },
            image_data: self.image_data.clone(),
            needs_upload: !self
                .last_uploaded
                .load(std::sync::atomic::Ordering::Relaxed),
            uploaded_signal: self.last_uploaded.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BlurPrimitive {
    uniforms: BlurUniforms,
    image_data: Arc<Vec<u8>>,
    needs_upload: bool,
    uploaded_signal: Arc<std::sync::atomic::AtomicBool>,
}

impl shader::Primitive for BlurPrimitive {
    type Pipeline = BlurPipeline;

    fn prepare(
        &self,
        pipeline: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        _bounds: &Rectangle,
        _viewport: &shader::Viewport,
    ) {
        // Enviar uniforms
        queue.write_buffer(
            &pipeline.uniform_buffer,
            0,
            bytemuck::bytes_of(&self.uniforms),
        );

        // Subir textura SOLO si es necesario (0% CPU en frames subsiguientes)
        if self.needs_upload {
            if let Ok(img) = image::load_from_memory(&self.image_data) {
                let rgba = img.to_rgba8();
                let dimensions = rgba.dimensions();

                let size = wgpu::Extent3d {
                    width: dimensions.0,
                    height: dimensions.1,
                    depth_or_array_layers: 1,
                };

                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("Background Texture"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8UnormSrgb,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });

                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    &rgba,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(4 * dimensions.0),
                        rows_per_image: Some(dimensions.1),
                    },
                    size,
                );

                // Recrear bind group con la nueva textura
                pipeline.update_texture(
                    device,
                    texture.create_view(&wgpu::TextureViewDescriptor::default()),
                );
                self.uploaded_signal
                    .store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    fn draw(&self, pipeline: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        if self
            .uploaded_signal
            .load(std::sync::atomic::Ordering::Relaxed)
        {
            render_pass.set_pipeline(&pipeline.pipeline);
            render_pass.set_bind_group(0, &pipeline.bind_group, &[]);
            render_pass.draw(0..6, 0..1);
            true
        } else {
            false
        }
    }
}

pub struct BlurPipeline {
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    uniform_buffer: wgpu::Buffer,
    layout: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
}

impl BlurPipeline {
    fn update_texture(&mut self, device: &wgpu::Device, view: wgpu::TextureView) {
        self.bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Blur Bind Group"),
            layout: &self.layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: self.uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.sampler),
                },
            ],
        });
    }
}

impl shader::Pipeline for BlurPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        const SHADER_WGSL: &str = r#"
struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

struct Uniforms {
    blur_amount: f32,      // 0.0 to 1.0
    time: f32,             // Animation time
    resolution: vec2<f32>, // Canvas resolution
};

@group(0) @binding(0) var<uniform> uniforms: Uniforms;
@group(0) @binding(1) var t_diffuse: texture_2d<f32>;
@group(0) @binding(2) var s_diffuse: sampler;

@vertex
fn vs_main(@builtin(vertex_index) vi: u32) -> VertexOutput {
    var out: VertexOutput;
    // Vértices explícitos para asegurar cobertura total del canvas (Full-screen Quad)
    let pos = array<vec2<f32>, 6>(
        vec2<f32>(-1.0, -1.0), vec2<f32>(1.0, -1.0), vec2<f32>(-1.0, 1.0),
        vec2<f32>(-1.0, 1.0), vec2<f32>(1.0, -1.0), vec2<f32>(1.0, 1.0)
    );
    out.position = vec4<f32>(pos[vi], 0.0, 1.0);
    out.uv = pos[vi] * 0.5 + 0.5;
    out.uv.y = 1.0 - out.uv.y; // Flip Y para Iced/WGPU
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let uv = in.uv;
    
    // Blur cremoso multi-tap (9 samples con bilinear filtering)
    // El offset escala con el blur_amount para suavizado progresivo
    let offset = uniforms.blur_amount * 0.008;
    
    var color = textureSample(t_diffuse, s_diffuse, uv) * 0.25;
    
    // Tap 1-4 (Cercanos)
    color += textureSample(t_diffuse, s_diffuse, uv + vec2<f32>(offset, offset)) * 0.15;
    color += textureSample(t_diffuse, s_diffuse, uv + vec2<f32>(-offset, -offset)) * 0.15;
    color += textureSample(t_diffuse, s_diffuse, uv + vec2<f32>(offset, -offset)) * 0.15;
    color += textureSample(t_diffuse, s_diffuse, uv + vec2<f32>(-offset, offset)) * 0.15;
    
    // Tap 5-8 (Lejanos)
    let offset2 = offset * 2.0;
    color += textureSample(t_diffuse, s_diffuse, uv + vec2<f32>(offset2, 0.0)) * 0.0375;
    color += textureSample(t_diffuse, s_diffuse, uv + vec2<f32>(-offset2, 0.0)) * 0.0375;
    color += textureSample(t_diffuse, s_diffuse, uv + vec2<f32>(0.0, offset2)) * 0.0375;
    color += textureSample(t_diffuse, s_diffuse, uv + vec2<f32>(0.0, -offset2)) * 0.0375;

    // Brillo base + animación sutil para que el 0% GPU se sienta vivo
    let wave = sin(uniforms.time * 0.1) * 0.03;
    let final_color = color.rgb * (2 + wave);
    
    return vec4<f32>(final_color, 1.0);
}
"#;

        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("High Fidelity Blur Shader"),
            source: wgpu::ShaderSource::Wgsl(Cow::Borrowed(SHADER_WGSL)),
        });

        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Blur Layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<BlurUniforms>() as u64,
                        ),
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        multisampled: false,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Blur Pipeline Layout"),
            bind_group_layouts: &[&layout],
            push_constant_ranges: &[],
        });

        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("Blur Uniform Buffer"),
            size: std::mem::size_of::<BlurUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        // Pipeline con TriangleList explícito
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("Blur Render Pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                buffers: &[],
                compilation_options: wgpu::PipelineCompilationOptions::default(),
            },
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
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
        });

        // Placeholder BindGroup inicial
        let placeholder_view = device
            .create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d::default(),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&wgpu::TextureViewDescriptor::default());

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Blur Bind Group"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniform_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&placeholder_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        Self {
            pipeline,
            bind_group,
            uniform_buffer,
            layout,
            sampler,
        }
    }
}
