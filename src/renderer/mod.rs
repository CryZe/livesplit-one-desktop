use futures_executor::block_on;
use livesplit_core::{
    layout::LayoutState,
    rendering::{self, Backend, Mesh, Rgba, Transform, Vertex},
};
use raw_window_handle::HasRawWindowHandle;
use std::{mem, rc::Rc};
use wgpu::{
    util::{make_spirv, BufferInitDescriptor, DeviceExt},
    vertex_attr_array, AddressMode, BackendBit, BindGroup, BindGroupDescriptor, BindGroupEntry,
    BindGroupLayout, BindGroupLayoutDescriptor, BindGroupLayoutEntry, BindingResource, BindingType,
    BlendDescriptor, BlendFactor, BlendOperation, Buffer, BufferSize, BufferUsage, Color,
    ColorStateDescriptor, ColorWrite, Device, Extent3d, FilterMode, IndexFormat, InputStepMode,
    Instance, LoadOp, Operations, Origin3d, PipelineLayoutDescriptor, PowerPreference, PresentMode,
    PrimitiveTopology, ProgrammableStageDescriptor, Queue, RasterizationStateDescriptor,
    RenderPassColorAttachmentDescriptor, RenderPassDescriptor, RenderPipeline,
    RenderPipelineDescriptor, RequestAdapterOptions, Sampler, SamplerDescriptor, ShaderStage,
    Surface, SwapChain, SwapChainDescriptor, SwapChainError, TextureComponentType, TextureCopyView,
    TextureDataLayout, TextureDescriptor, TextureDimension, TextureFormat, TextureUsage,
    TextureView, TextureViewDimension, VertexBufferDescriptor, VertexStateDescriptor,
};
use wgpu_mipmap::{MipmapGenerator, RecommendedMipmapGenerator};

const SAMPLES: u32 = 8;

struct DrawCall {
    mesh: MeshData,
    bind_group: BindGroup,
    use_texture_pipeline: bool,
}

struct Context {
    device: Device,
    queue: Queue,
    draw_calls: Vec<DrawCall>,
    color_bind_group_layout: BindGroupLayout,
    texture_bind_group_layout: BindGroupLayout,
    sampler: Sampler,
    resize_hint: Option<(f32, f32)>,
}

type MeshData = Rc<(Buffer, Buffer, u32)>;
type TextureData = TextureView;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
struct Uniforms {
    transform: [[f32; 4]; 2],
    color_tl: [f32; 4],
    color_tr: [f32; 4],
    color_bl: [f32; 4],
    color_br: [f32; 4],
}

impl Backend for Context {
    type Mesh = MeshData;
    type Texture = TextureData;

    fn create_mesh(&mut self, mesh: &Mesh) -> Self::Mesh {
        Rc::new((
            self.device.create_buffer_init(&BufferInitDescriptor {
                label: None,
                contents: mesh.vertices_as_bytes(),
                usage: BufferUsage::VERTEX,
            }),
            self.device.create_buffer_init(&BufferInitDescriptor {
                label: None,
                contents: mesh.indices_as_bytes(),
                usage: BufferUsage::INDEX,
            }),
            mesh.indices().len() as _,
        ))
    }

    fn render_mesh(
        &mut self,
        mesh: &MeshData,
        transform: Transform,
        [color_tl, color_tr, color_br, color_bl]: [Rgba; 4],
        texture: Option<&TextureData>,
    ) {
        let [x1, y1, z1, x2, y2, z2] = transform.to_column_major_array();
        let buffer = self.device.create_buffer_init(&BufferInitDescriptor {
            label: None,
            contents: bytemuck::bytes_of(&Uniforms {
                transform: [[x1, y1, z1, 0.0], [x2, y2, z2, 0.0]],
                color_tl,
                color_tr,
                color_bl,
                color_br,
            }),
            usage: BufferUsage::UNIFORM,
        });

        let bind_group = if let Some(texture_view) = texture {
            self.device.create_bind_group(&BindGroupDescriptor {
                label: None,
                layout: &self.texture_bind_group_layout,
                entries: &[
                    BindGroupEntry {
                        binding: 0,
                        resource: BindingResource::Buffer(buffer.slice(..)),
                    },
                    BindGroupEntry {
                        binding: 1,
                        resource: BindingResource::TextureView(texture_view),
                    },
                    BindGroupEntry {
                        binding: 2,
                        resource: BindingResource::Sampler(&self.sampler),
                    },
                ],
            })
        } else {
            self.device.create_bind_group(&BindGroupDescriptor {
                label: None,
                layout: &self.color_bind_group_layout,
                entries: &[BindGroupEntry {
                    binding: 0,
                    resource: BindingResource::Buffer(buffer.slice(..)),
                }],
            })
        };

        self.draw_calls.push(DrawCall {
            mesh: mesh.clone(),
            bind_group,
            use_texture_pipeline: texture.is_some(),
        });
    }

    fn free_mesh(&mut self, _mesh: Self::Mesh) {}

    fn create_texture(&mut self, width: u32, height: u32, data: &[u8]) -> Self::Texture {
        let texture_extent = Extent3d {
            width,
            height,
            depth: 1,
        };
        let mip_level_count = (width.max(height) as f64).log2().floor() as u32 + 1;
        let descriptor = TextureDescriptor {
            label: None,
            size: texture_extent,
            mip_level_count,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsage::SAMPLED | TextureUsage::COPY_DST | TextureUsage::COPY_SRC,
        };
        let texture = self.device.create_texture(&descriptor);

        self.queue.write_texture(
            TextureCopyView {
                texture: &texture,
                mip_level: 0,
                origin: Origin3d { x: 0, y: 0, z: 0 },
            },
            data,
            TextureDataLayout {
                offset: 0,
                bytes_per_row: 4 * width,
                rows_per_image: height,
            },
            texture_extent,
        );

        let mut encoder = self.device.create_command_encoder(&Default::default());

        RecommendedMipmapGenerator::new(&self.device)
            .generate(&self.device, &mut encoder, &texture, &descriptor)
            .unwrap(); // TODO: Unwrap

        self.queue.submit(Some(encoder.finish()));

        texture.create_view(&Default::default())
    }

    fn free_texture(&mut self, _texture: Self::Texture) {}

    fn resize(&mut self, width: f32, height: f32) {
        self.resize_hint = Some((width, height));
    }
}

pub struct Renderer {
    renderer: rendering::Renderer<MeshData, TextureData>,
    surface: Surface,
    swap_chain: SwapChain,
    color_render_pipeline: RenderPipeline,
    texture_render_pipeline: RenderPipeline,
    intermediary_view: TextureView,
    dimensions: (f32, f32),
    context: Context,
}

impl Renderer {
    pub fn new(window: &impl HasRawWindowHandle, [width, height]: [u32; 2]) -> Option<Self> {
        let instance = Instance::new(BackendBit::PRIMARY);
        let surface = unsafe { instance.create_surface(window) }; // TODO: This function should be unsafe then.

        let adapter = block_on(instance.request_adapter(&RequestAdapterOptions {
            power_preference: PowerPreference::Default,
            compatible_surface: Some(&surface),
        }))?;

        let (device, queue) = block_on(adapter.request_device(&Default::default(), None)).ok()?;

        let swap_desc = SwapChainDescriptor {
            usage: TextureUsage::OUTPUT_ATTACHMENT,
            format: TextureFormat::Bgra8Unorm,
            width,
            height,
            present_mode: PresentMode::Fifo,
        };
        let (swap_chain, intermediary_view) = create_swap_chain(&surface, &device, [width, height]);

        let vs = {
            #[allow(dead_code)]
            #[derive(glsl_to_spirv_macros_impl::GLSLEmbedImpl)]
            #[src = "
#version 450

layout(binding = 0) uniform Data {
    mat2x4 transform;
    vec4 color_tl;
    vec4 color_tr;
    vec4 color_bl;
    vec4 color_br;
} data;

layout(location = 0) in vec2 position;
layout(location = 1) in vec2 texcoord;

layout(location = 0) out vec4 color;
layout(location = 1) out vec2 outTexcoord;

void main() {
    vec4 left = mix(data.color_tl, data.color_bl, texcoord.y);
    vec4 right = mix(data.color_tr, data.color_br, texcoord.y);
    color = mix(left, right, texcoord.x);

    vec2 pos = vec4(position, 1, 0) * data.transform;
    gl_Position = vec4(vec2(2, -2) * pos.xy + vec2(-1, 1), 0, 1);
    outTexcoord = texcoord;
}
"]
            #[ty = "vs"]
            struct Dummy;
            &DATA as &'static [u8]
        };

        let color_fs = {
            #[allow(dead_code)]
            #[derive(glsl_to_spirv_macros_impl::GLSLEmbedImpl)]
            #[src = "
#version 450

layout(location = 0) in vec4 color;
layout(location = 1) in vec2 texcoord;
layout(location = 0) out vec4 outColor;

void main() {
    outColor = color;
}
"]
            #[ty = "fs"]
            struct Dummy;
            &DATA as &'static [u8]
        };

        let texture_fs = {
            #[allow(dead_code)]
            #[derive(glsl_to_spirv_macros_impl::GLSLEmbedImpl)]
            #[src = "
#version 450

layout(location = 0) in vec4 color;
layout(location = 1) in vec2 texcoord;
layout(location = 0) out vec4 outColor;

layout(binding = 1) uniform texture2D u_texture;
layout(binding = 2) uniform sampler u_sampler;

void main() {
    outColor = color * texture(sampler2D(u_texture, u_sampler), texcoord);
}
"]
            #[ty = "fs"]
            struct Dummy;
            &DATA as &'static [u8]
        };

        let vs = device.create_shader_module(make_spirv(vs));
        let color_fs = device.create_shader_module(make_spirv(color_fs));
        let texture_fs = device.create_shader_module(make_spirv(texture_fs));

        let color_bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: None,
            entries: &[BindGroupLayoutEntry {
                binding: 0,
                visibility: ShaderStage::VERTEX,
                ty: BindingType::UniformBuffer {
                    dynamic: false,
                    min_binding_size: BufferSize::new(mem::size_of::<Uniforms>() as _),
                },
                count: None,
            }],
        });

        let texture_bind_group_layout =
            device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                label: None,
                entries: &[
                    BindGroupLayoutEntry {
                        binding: 0,
                        visibility: ShaderStage::VERTEX,
                        ty: BindingType::UniformBuffer {
                            dynamic: false,
                            min_binding_size: BufferSize::new(mem::size_of::<Uniforms>() as _),
                        },
                        count: None,
                    },
                    BindGroupLayoutEntry {
                        binding: 1,
                        visibility: ShaderStage::FRAGMENT,
                        ty: BindingType::SampledTexture {
                            multisampled: false,
                            dimension: TextureViewDimension::D2,
                            component_type: TextureComponentType::Uint,
                        },
                        count: None,
                    },
                    BindGroupLayoutEntry {
                        binding: 2,
                        visibility: ShaderStage::FRAGMENT,
                        ty: BindingType::Sampler { comparison: false },
                        count: None,
                    },
                ],
            });

        let color_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&color_bind_group_layout],
            push_constant_ranges: &[],
        });

        let texture_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[&texture_bind_group_layout],
            push_constant_ranges: &[],
        });

        let mut pipeline_desc = RenderPipelineDescriptor {
            label: None,
            layout: Some(&color_pipeline_layout),
            vertex_stage: ProgrammableStageDescriptor {
                module: &vs,
                entry_point: "main",
            },
            fragment_stage: Some(ProgrammableStageDescriptor {
                module: &color_fs,
                entry_point: "main",
            }),
            rasterization_state: Some(RasterizationStateDescriptor::default()),
            primitive_topology: PrimitiveTopology::TriangleList,
            color_states: &[ColorStateDescriptor {
                format: swap_desc.format,
                color_blend: BlendDescriptor {
                    src_factor: BlendFactor::SrcAlpha,
                    dst_factor: BlendFactor::OneMinusSrcAlpha,
                    operation: BlendOperation::Add,
                },
                alpha_blend: BlendDescriptor {
                    src_factor: BlendFactor::One,
                    dst_factor: BlendFactor::OneMinusSrcAlpha,
                    operation: BlendOperation::Add,
                },
                write_mask: ColorWrite::ALL,
            }],
            depth_stencil_state: None,
            vertex_state: VertexStateDescriptor {
                index_format: IndexFormat::Uint16,
                vertex_buffers: &[VertexBufferDescriptor {
                    stride: mem::size_of::<Vertex>() as _,
                    step_mode: InputStepMode::Vertex,
                    attributes: &vertex_attr_array![0 => Float2, 1 => Float2],
                }],
            },
            sample_count: SAMPLES,
            sample_mask: !0,
            alpha_to_coverage_enabled: false,
        };

        let color_render_pipeline = device.create_render_pipeline(&pipeline_desc);

        pipeline_desc.layout = Some(&texture_pipeline_layout);
        pipeline_desc.fragment_stage = Some(ProgrammableStageDescriptor {
            module: &texture_fs,
            entry_point: "main",
        });

        let texture_render_pipeline = device.create_render_pipeline(&pipeline_desc);

        let sampler = device.create_sampler(&SamplerDescriptor {
            label: None,
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: FilterMode::Linear,
            lod_min_clamp: -8000.0,
            lod_max_clamp: 8000.0,
            compare: None,
            anisotropy_clamp: None,
        });

        Some(Self {
            renderer: rendering::Renderer::new(),
            surface,
            swap_chain,
            color_render_pipeline,
            texture_render_pipeline,
            intermediary_view,
            dimensions: (width as _, height as _),
            context: Context {
                device,
                queue,
                draw_calls: Vec::new(),
                color_bind_group_layout,
                texture_bind_group_layout,
                sampler,
                resize_hint: None,
            },
        })
    }

    pub fn resize(&mut self, [width, height]: [u32; 2]) {
        let (swap_chain, intermediary_view) =
            create_swap_chain(&self.surface, &self.context.device, [width, height]);
        self.swap_chain = swap_chain;
        self.intermediary_view = intermediary_view;
        self.dimensions = (width as _, height as _);
    }

    pub fn render_frame(&mut self, state: &LayoutState) -> Option<(f32, f32)> {
        let frame = self.swap_chain.get_current_frame();

        if self.dimensions.0 == 0.0 || self.dimensions.1 == 0.0 {
            return None;
        }

        if matches!(
            frame,
            Err(SwapChainError::Lost) | Err(SwapChainError::Outdated)
        ) {
            self.resize([self.dimensions.0 as _, self.dimensions.1 as _]);
            return None;
        }
        let frame = frame.unwrap(); // TODO: Handle SwapChainError::Timeout/OutOfMemory

        let mut encoder = self
            .context
            .device
            .create_command_encoder(&Default::default());

        self.context.draw_calls.clear();

        self.renderer
            .render(&mut self.context, self.dimensions, state);
        let resize_hint = self.context.resize_hint.take();

        let mut pass = encoder.begin_render_pass(&RenderPassDescriptor {
            color_attachments: &[RenderPassColorAttachmentDescriptor {
                attachment: &self.intermediary_view,
                resolve_target: Some(&frame.output.view),
                ops: Operations {
                    load: LoadOp::Clear(Color::TRANSPARENT),
                    store: true,
                },
            }],
            depth_stencil_attachment: None,
        });

        for draw_call in &self.context.draw_calls {
            pass.set_pipeline(if draw_call.use_texture_pipeline {
                &self.texture_render_pipeline
            } else {
                &self.color_render_pipeline
            });

            let (vertices, indices, count) = &*draw_call.mesh;
            pass.set_index_buffer(indices.slice(..));
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.set_bind_group(0, &draw_call.bind_group, &[]);
            pass.draw_indexed(0..*count, 0, 0..1);
        }

        drop(pass);

        self.context.queue.submit(Some(encoder.finish()));

        resize_hint
    }
}

fn create_swap_chain(
    surface: &Surface,
    device: &Device,
    [width, height]: [u32; 2],
) -> (SwapChain, TextureView) {
    let swap_desc = SwapChainDescriptor {
        usage: TextureUsage::OUTPUT_ATTACHMENT,
        format: TextureFormat::Bgra8Unorm,
        width,
        height,
        present_mode: PresentMode::Fifo,
    };
    let swap_chain = device.create_swap_chain(&surface, &swap_desc);
    let intermediary_view = device
        .create_texture(&TextureDescriptor {
            label: None,
            size: Extent3d {
                width,
                height,
                depth: 1,
            },
            mip_level_count: 1,
            sample_count: SAMPLES,
            dimension: TextureDimension::D2,
            format: TextureFormat::Bgra8Unorm,
            usage: TextureUsage::OUTPUT_ATTACHMENT,
        })
        .create_view(&Default::default());

    (swap_chain, intermediary_view)
}
