use super::{ATLAS, HEIGHT, WIDTH, cells};
use rune::render::EffectSite;
use wgpu::util::DeviceExt;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    rect: [f32; 4],
    viewport: [f32; 4],
    clip: [f32; 4],
    fade: [u32; 4],
    animation: [f32; 4],
}

pub(super) struct Resources {
    pipelines: [(wgpu::TextureFormat, wgpu::RenderPipeline); 2],
    uniforms: wgpu::Buffer,
    bindings: wgpu::BindGroup,
}

impl Resources {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        let uniforms = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Home ASCII uniforms"),
            contents: bytemuck::bytes_of(&Uniforms::zero()),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let cell_texture = texture(
            device,
            queue,
            "Home ASCII cells",
            (WIDTH as u32, HEIGHT as u32),
            wgpu::TextureFormat::Rgba8Uint,
            4,
            &cells(),
        );
        let glyph_texture = texture(
            device,
            queue,
            "Home ASCII glyphs",
            (160, 28),
            wgpu::TextureFormat::R8Unorm,
            1,
            ATLAS,
        );
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("Home ASCII glyph filtering"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("Home ASCII"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                texture_entry(1, wgpu::TextureSampleType::Uint),
                texture_entry(2, wgpu::TextureSampleType::Float { filterable: true }),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let bindings = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("Home ASCII"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: uniforms.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&cell_texture),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&glyph_texture),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("Home ASCII"),
            source: wgpu::ShaderSource::Wgsl(
                format!(
                    "{}\n{}",
                    rune::render::CLIP_WGSL,
                    include_str!("shader.wgsl")
                )
                .into(),
            ),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("Home ASCII"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipelines = [format, wgpu::TextureFormat::Rgba16Float].map(|format| {
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("Home ASCII"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleStrip,
                    ..Default::default()
                },
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fragment"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            });
            (format, pipeline)
        });
        Self {
            pipelines,
            uniforms,
            bindings,
        }
    }

    pub(super) fn draw(
        &self,
        queue: &wgpu::Queue,
        pass: &mut wgpu::RenderPass<'_>,
        site: &EffectSite<'_>,
        phase: f32,
    ) {
        let Some((_, pipeline)) = self
            .pipelines
            .iter()
            .find(|(format, _)| *format == site.format)
        else {
            return;
        };
        let uniforms = Uniforms {
            rect: site.instance.rect,
            viewport: [
                site.size[0] as f32,
                site.size[1] as f32,
                site.origin[0],
                site.origin[1],
            ],
            clip: site.instance.clip,
            fade: site.instance.fade,
            animation: [
                phase / super::PERIOD as f32,
                site.data[0],
                f32::from(site.srgb_blend),
                0.0,
            ],
        };
        queue.write_buffer(&self.uniforms, 0, bytemuck::bytes_of(&uniforms));
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &self.bindings, &[]);
        pass.draw(0..4, 0..1);
    }
}

impl Uniforms {
    fn zero() -> Self {
        bytemuck::Zeroable::zeroed()
    }
}

fn texture_entry(binding: u32, sample_type: wgpu::TextureSampleType) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::FRAGMENT,
        ty: wgpu::BindingType::Texture {
            sample_type,
            view_dimension: wgpu::TextureViewDimension::D2,
            multisampled: false,
        },
        count: None,
    }
}

fn texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    label: &str,
    size: (u32, u32),
    format: wgpu::TextureFormat,
    bytes_per_pixel: u32,
    data: &[u8],
) -> wgpu::TextureView {
    let texture = device.create_texture_with_data(
        queue,
        &wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: size.0,
                height: size.1,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        },
        wgpu::util::TextureDataOrder::LayerMajor,
        data,
    );
    debug_assert_eq!(data.len(), (size.0 * size.1 * bytes_per_pixel) as usize);
    texture.create_view(&Default::default())
}
