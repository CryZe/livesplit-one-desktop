use livesplit_core::{
    layout::LayoutState,
    rendering::{self, Backend, Mesh, Rgba, Transform, Vertex},
};
use raw_window_handle::HasRawWindowHandle;
use wgpu::{
    Adapter, AddressMode, BackendBit, BindGroupDescriptor, BindGroupLayout, BindGroupLayoutBinding,
    BindGroupLayoutDescriptor, Binding, BindingResource, BindingType, BlendDescriptor, BlendFactor,
    BlendOperation, Buffer, BufferCopyView, BufferUsage, Color, ColorStateDescriptor, ColorWrite,
    CompareFunction, Device, Extent3d, FilterMode, IndexFormat, InputStepMode, LoadOp, Origin3d,
    PipelineLayoutDescriptor, PowerPreference, PresentMode, PrimitiveTopology,
    ProgrammableStageDescriptor, Queue, RasterizationStateDescriptor, RenderPass,
    RenderPassColorAttachmentDescriptor, RenderPassDescriptor, RenderPipeline,
    RenderPipelineDescriptor, RequestAdapterOptions, Sampler, SamplerDescriptor, ShaderStage,
    StoreOp, Surface, SwapChain, SwapChainDescriptor, TextureCopyView, TextureDescriptor,
    TextureDimension, TextureFormat, TextureUsage, TextureView, TextureViewDimension,
    VertexAttributeDescriptor, VertexBufferDescriptor, VertexFormat,
};

const SAMPLES: u32 = 8;

struct Context<'a> {
    device: &'a mut Device,
    queue: &'a mut Queue,
    pass: RenderPass<'a>,
    color_render_pipeline: &'a RenderPipeline,
    color_bind_group_layout: &'a BindGroupLayout,
    texture_render_pipeline: &'a RenderPipeline,
    texture_bind_group_layout: &'a BindGroupLayout,
    sampler: &'a Sampler,
    resize_hint: Option<(f32, f32)>,
}

type MeshData = (Buffer, Buffer, u32);
type TextureData = TextureView;

impl Backend for Context<'_> {
    type Mesh = MeshData;
    type Texture = TextureData;

    fn create_mesh(&mut self, mesh: &Mesh) -> Self::Mesh {
        let indices = mesh.indices();
        let vertices = mesh.vertices();

        let index_buffer = self
            .device
            .create_buffer_mapped(indices.len(), BufferUsage::INDEX)
            .fill_from_slice(indices);

        let vertex_buffer = self
            .device
            .create_buffer_mapped(vertices.len(), BufferUsage::VERTEX)
            .fill_from_slice(vertices);

        (vertex_buffer, index_buffer, indices.len() as _)
    }

    fn render_mesh(
        &mut self,
        (vertices, indices, count): &Self::Mesh,
        transform: Transform,
        [tl, tr, br, bl]: [Rgba; 4],
        texture: Option<&Self::Texture>,
    ) {
        let [x1, y1, z1, x2, y2, z2] = transform.to_column_major_array();
        let buffer = self
            .device
            .create_buffer_mapped(6, BufferUsage::UNIFORM)
            .fill_from_slice(&[[x1, y1, z1, 0.0], [x2, y2, z2, 0.0], tl, tr, bl, br]);

        let bind_group = if let Some(texture_view) = texture {
            self.pass.set_pipeline(self.texture_render_pipeline);
            self.device.create_bind_group(&BindGroupDescriptor {
                layout: self.texture_bind_group_layout,
                bindings: &[
                    Binding {
                        binding: 0,
                        resource: BindingResource::Buffer {
                            buffer: &buffer,
                            range: 0..(6 * 4 * 4),
                        },
                    },
                    Binding {
                        binding: 1,
                        resource: BindingResource::TextureView(texture_view),
                    },
                    Binding {
                        binding: 2,
                        resource: BindingResource::Sampler(self.sampler),
                    },
                ],
            })
        } else {
            self.pass.set_pipeline(self.color_render_pipeline);
            self.device.create_bind_group(&BindGroupDescriptor {
                layout: self.color_bind_group_layout,
                bindings: &[Binding {
                    binding: 0,
                    resource: BindingResource::Buffer {
                        buffer: &buffer,
                        range: 0..(6 * 4 * 4),
                    },
                }],
            })
        };
        self.pass.set_index_buffer(indices, 0);
        self.pass.set_vertex_buffers(0, &[(vertices, 0)]);
        self.pass.set_bind_group(0, &bind_group, &[]);
        self.pass.draw_indexed(0..*count, 0, 0..1);
    }

    fn free_mesh(&mut self, _mesh: Self::Mesh) {}

    fn create_texture(&mut self, width: u32, height: u32, data: &[u8]) -> Self::Texture {
        let texture_extent = Extent3d {
            width,
            height,
            depth: 1,
        };
        // let mip_level_count = (width.max(height) as f64).log2().floor() as u32 + 1;
        let texture = self.device.create_texture(&TextureDescriptor {
            size: texture_extent,
            array_layer_count: 1,
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsage::SAMPLED | TextureUsage::COPY_DST | TextureUsage::COPY_SRC,
        });
        let texture_view = texture.create_default_view();

        let buffer = self
            .device
            .create_buffer_mapped(data.len(), BufferUsage::COPY_SRC)
            .fill_from_slice(data);
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_texture(
            BufferCopyView {
                buffer: &buffer,
                offset: 0,
                row_pitch: 4 * width,
                image_height: height,
            },
            TextureCopyView {
                texture: &texture,
                mip_level: 0,
                array_layer: 0,
                origin: Origin3d {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                },
            },
            texture_extent,
        );

        // let (mut mip_width, mut mip_height) = (width, height);
        // for mip_level in 1..mip_level_count {
        //     encoder.copy_texture_to_texture(
        //         TextureCopyView {
        //             texture: &texture,
        //             mip_level: mip_level - 1,
        //             array_layer: 0,
        //             origin: Origin3d {
        //                 x: 0.0,
        //                 y: 0.0,
        //                 z: 0.0,
        //             },
        //         },
        //         TextureCopyView {
        //             texture: &texture,
        //             mip_level,
        //             array_layer: 0,
        //             origin: Origin3d {
        //                 x: 0.0,
        //                 y: 0.0,
        //                 z: 0.0,
        //             },
        //         },
        //         Extent3d {
        //             width: if mip_width > 1 { mip_width / 2 } else { 1 },
        //             height: if mip_height > 1 { mip_height / 2 } else { 1 },
        //             depth: 1,
        //         },
        //     );

        //     if mip_width > 1 {
        //         mip_width /= 2;
        //     }
        //     if mip_height > 1 {
        //         mip_height /= 2;
        //     }
        // }

        self.queue.submit(&[encoder.finish()]);

        texture_view
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
    device: Device,
    queue: Queue,
    color_render_pipeline: RenderPipeline,
    color_bind_group_layout: BindGroupLayout,
    texture_render_pipeline: RenderPipeline,
    texture_bind_group_layout: BindGroupLayout,
    sampler: Sampler,
    intermediary_view: TextureView,
    dimensions: (f32, f32),
}

impl Renderer {
    pub fn new(window: &impl HasRawWindowHandle, [width, height]: [u32; 2]) -> Option<Self> {
        let surface = Surface::create(window);

        let adapter = Adapter::request(&RequestAdapterOptions {
            power_preference: PowerPreference::Default,
            backends: BackendBit::PRIMARY,
        })?;

        let (device, queue) = adapter.request_device(&Default::default());

        let swap_desc = SwapChainDescriptor {
            usage: TextureUsage::OUTPUT_ATTACHMENT,
            format: TextureFormat::Bgra8Unorm,
            width,
            height,
            present_mode: PresentMode::Vsync,
        };
        let (swap_chain, intermediary_view) = create_swap_chain(&surface, &device, [width, height]);

        let mut u32_data = Vec::new();
        u32_data.extend(
            glsl_to_spirv_macros::include_glsl_vs!("src/renderer/quad.vert")
                .chunks(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])),
        );
        let vs = device.create_shader_module(&u32_data);

        u32_data.clear();
        u32_data.extend(
            glsl_to_spirv_macros::include_glsl_fs!("src/renderer/quad_colored.frag")
                .chunks(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])),
        );
        let color_fs = device.create_shader_module(&u32_data);

        u32_data.clear();
        u32_data.extend(
            glsl_to_spirv_macros::include_glsl_fs!("src/renderer/quad_textured.frag")
                .chunks(4)
                .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]])),
        );
        let texture_fs = device.create_shader_module(&u32_data);

        let color_bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            bindings: &[BindGroupLayoutBinding {
                binding: 0,
                visibility: ShaderStage::VERTEX,
                ty: BindingType::UniformBuffer { dynamic: false },
            }],
        });

        let texture_bind_group_layout =
            device.create_bind_group_layout(&BindGroupLayoutDescriptor {
                bindings: &[
                    BindGroupLayoutBinding {
                        binding: 0,
                        visibility: ShaderStage::VERTEX,
                        ty: BindingType::UniformBuffer { dynamic: false },
                    },
                    BindGroupLayoutBinding {
                        binding: 1,
                        visibility: ShaderStage::FRAGMENT,
                        ty: BindingType::SampledTexture {
                            multisampled: false,
                            dimension: TextureViewDimension::D2,
                        },
                    },
                    BindGroupLayoutBinding {
                        binding: 2,
                        visibility: ShaderStage::FRAGMENT,
                        ty: BindingType::Sampler,
                    },
                ],
            });

        let color_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            bind_group_layouts: &[&color_bind_group_layout],
        });

        let texture_pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            bind_group_layouts: &[&texture_bind_group_layout],
        });

        let mut pipeline_desc = RenderPipelineDescriptor {
            layout: &color_pipeline_layout,
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
            index_format: IndexFormat::Uint16,
            vertex_buffers: &[VertexBufferDescriptor {
                stride: core::mem::size_of::<Vertex>() as _,
                step_mode: InputStepMode::Vertex,
                attributes: &[
                    VertexAttributeDescriptor {
                        format: VertexFormat::Float2,
                        offset: 0,
                        shader_location: 0,
                    },
                    VertexAttributeDescriptor {
                        format: VertexFormat::Float2,
                        offset: 2 * 4,
                        shader_location: 1,
                    },
                ],
            }],
            sample_count: SAMPLES,
            sample_mask: !0,
            alpha_to_coverage_enabled: false,
        };

        let color_render_pipeline = device.create_render_pipeline(&pipeline_desc);

        pipeline_desc.layout = &texture_pipeline_layout;
        pipeline_desc.fragment_stage = Some(ProgrammableStageDescriptor {
            module: &texture_fs,
            entry_point: "main",
        });

        let texture_render_pipeline = device.create_render_pipeline(&pipeline_desc);

        let sampler = device.create_sampler(&SamplerDescriptor {
            address_mode_u: AddressMode::ClampToEdge,
            address_mode_v: AddressMode::ClampToEdge,
            address_mode_w: AddressMode::ClampToEdge,
            mag_filter: FilterMode::Linear,
            min_filter: FilterMode::Linear,
            mipmap_filter: FilterMode::Linear,
            lod_min_clamp: -8000.0,
            lod_max_clamp: 8000.0,
            compare_function: CompareFunction::Never,
        });

        Some(Self {
            renderer: rendering::Renderer::new(),
            surface,
            swap_chain,
            device,
            queue,
            color_render_pipeline,
            color_bind_group_layout,
            texture_render_pipeline,
            texture_bind_group_layout,
            sampler,
            intermediary_view,
            dimensions: (width as _, height as _),
        })
    }

    pub fn resize(&mut self, [width, height]: [u32; 2]) {
        let (swap_chain, intermediary_view) =
            create_swap_chain(&self.surface, &self.device, [width, height]);
        self.swap_chain = swap_chain;
        self.intermediary_view = intermediary_view;
        self.dimensions = (width as _, height as _);
    }

    pub fn render_frame(&mut self, state: &LayoutState) -> Option<(f32, f32)> {
        let frame = self.swap_chain.get_next_texture();
        if self.dimensions.0 == 0.0 || self.dimensions.1 == 0.0 {
            return None;
        }
        let mut encoder = self.device.create_command_encoder(&Default::default());

        let pass = encoder.begin_render_pass(&RenderPassDescriptor {
            color_attachments: &[RenderPassColorAttachmentDescriptor {
                attachment: &self.intermediary_view,
                resolve_target: Some(&frame.view),
                load_op: LoadOp::Clear,
                store_op: StoreOp::Store,
                clear_color: Color::TRANSPARENT,
            }],
            depth_stencil_attachment: None,
        });

        let mut context = Context {
            device: &mut self.device,
            queue: &mut self.queue,
            pass,
            color_bind_group_layout: &self.color_bind_group_layout,
            color_render_pipeline: &self.color_render_pipeline,
            texture_bind_group_layout: &self.texture_bind_group_layout,
            texture_render_pipeline: &self.texture_render_pipeline,
            sampler: &self.sampler,
            resize_hint: None,
        };
        self.renderer.render(&mut context, self.dimensions, state);
        let resize_hint = context.resize_hint;
        drop(context);

        self.queue.submit(&[encoder.finish()]);

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
        present_mode: PresentMode::Vsync,
    };
    let swap_chain = device.create_swap_chain(&surface, &swap_desc);
    let intermediary_view = device
        .create_texture(&TextureDescriptor {
            size: Extent3d {
                width,
                height,
                depth: 1,
            },
            array_layer_count: 1,
            mip_level_count: 1,
            sample_count: SAMPLES,
            dimension: TextureDimension::D2,
            format: TextureFormat::Bgra8Unorm,
            usage: TextureUsage::OUTPUT_ATTACHMENT,
        })
        .create_default_view();

    (swap_chain, intermediary_view)
}
