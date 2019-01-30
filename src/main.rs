// #![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[macro_use]
extern crate glsl_to_spirv_macros_impl;

mod config;

use {
    crate::config::Config,
    gfx_backend_vulkan as back,
    // gfx_backend_dx12 as back,
    // gfx_backend_gl as back,
    gfx_hal::{
        buffer::{self, IndexBufferView},
        command::{self, RenderPassInlineEncoder},
        device::Device,
        format::{self, AsFormat, Aspects, ChannelType, Swizzle},
        image::{
            self, Filter, Kind as ImageKind, Layout as ImageLayout, SamplerInfo, Size as ImageSize,
            SubresourceRange, Tiling, Usage, ViewCapabilities, ViewKind, WrapMode,
        },
        memory::{self, Properties},
        pass::{self, Subpass},
        pool,
        pso::{self, PipelineStage, ShaderStageFlags},
        queue::{self, CommandQueue, QueueGroup, Submission},
        window::{Extent2D, PresentMode, Surface},
        Backbuffer, CommandPool, DescriptorPool, FrameSync, IndexType, Instance, Limits,
        MemoryType, PhysicalDevice, Primitive, Swapchain, SwapchainConfig,
    },
    livesplit_core::{
        auto_splitting,
        layout::{self, Layout, LayoutSettings},
        rendering::{Backend, IndexPair, Mesh, Renderer, Rgba, Transform},
        run::parser::composite,
        Timer, TimingMethod,
    },
    std::{
        fs::File,
        io::{prelude::*, BufReader, SeekFrom},
        mem,
    },
    winit::{
        dpi, ElementState, EventsLoop, Icon, KeyboardInput, MouseScrollDelta, VirtualKeyCode,
        WindowBuilder, WindowEvent,
    },
};

const DIMS: Extent2D = Extent2D {
    width: 300,
    height: 500,
};

const ENTRY_NAME: &str = "main";

#[derive(Debug, Clone, Copy)]
#[repr(C)]
struct Vertex {
    position: [f32; 2],
    texcoord: [f32; 2],
}

const COLOR_RANGE: SubresourceRange = SubresourceRange {
    aspects: Aspects::COLOR,
    levels: 0..1,
    layers: 0..1,
};

struct LongLivingData<B>
where
    B: gfx_hal::Backend,
{
    device: B::Device,
    memory_types: Vec<MemoryType>,
    command_pool: CommandPool<B, queue::capability::Graphics>,
    image_command_pool: CommandPool<B, queue::capability::Graphics>,
    queue_group: QueueGroup<B, queue::capability::Graphics>,
    frame_fence: B::Fence,
    image_fence: B::Fence,
    limits: Limits,
    desc_pool: B::DescriptorPool,
    set_layout: B::DescriptorSetLayout,
    sampler: B::Sampler,
    pipeline_textured: B::GraphicsPipeline,
    pipeline_layout_textured: B::PipelineLayout,
    pipeline_colored: B::GraphicsPipeline,
    pipeline_layout_colored: B::PipelineLayout,

    meshes: Vec<(B::Buffer, B::Buffer, usize, B::Memory, B::Memory)>,
    images: Vec<(B::DescriptorSet, B::ImageView, B::Image, B::Memory)>,
}

struct GfxBackend<'frame, 'data, 'other, 'window, B>
where
    B: gfx_hal::Backend,
{
    data: &'data mut LongLivingData<B>,
    encoder: &'frame mut RenderPassInlineEncoder<'other, B>,
    window: &'window winit::Window,
}

impl<'frame, 'data, 'other, 'window, B> Backend for GfxBackend<'frame, 'data, 'other, 'window, B>
where
    B: gfx_hal::Backend,
{
    fn create_mesh(&mut self, Mesh { vertices, indices }: &Mesh) -> IndexPair {
        if indices.is_empty() {
            return [0, 1, 0];
        }

        let indices_len = indices.len();

        let (vertices, vertices_mem) = upload_buffer(
            &self.data.device,
            &self.data.memory_types,
            vertices,
            buffer::Usage::VERTEX,
        );
        let (indices, indices_mem) = upload_buffer(
            &self.data.device,
            &self.data.memory_types,
            indices,
            buffer::Usage::INDEX,
        );
        let id = self.data.meshes.len();
        self.data
            .meshes
            .push((vertices, indices, indices_len, vertices_mem, indices_mem));
        [id, 0, 0]
    }

    fn render_mesh(
        &mut self,
        [idx, skip, _]: IndexPair,
        transform: Transform,
        [tl, tr, br, bl]: [Rgba; 4],
        texture: Option<IndexPair>,
    ) {
        if skip != 0 {
            return;
        }

        let (vertices, indices, indices_len, _, _) = &self.data.meshes[idx];
        let [x1, y1, z1, x2, y2, z2] = transform.to_column_major_array();

        unsafe {
            if let Some([tex_idx, _, _]) = texture {
                let (desc_set, _, _, _) = &self.data.images[tex_idx];

                self.encoder
                    .bind_graphics_pipeline(&self.data.pipeline_textured);
                self.encoder.bind_vertex_buffers(0, Some((vertices, 0)));
                self.encoder.bind_index_buffer(IndexBufferView {
                    buffer: indices,
                    offset: 0,
                    index_type: IndexType::U16,
                });
                self.encoder.bind_graphics_descriptor_sets(
                    &self.data.pipeline_layout_textured,
                    0,
                    Some(desc_set),
                    &[],
                );
                self.encoder.push_graphics_constants(
                    &self.data.pipeline_layout_textured,
                    ShaderStageFlags::VERTEX,
                    0,
                    &[
                        x1.to_bits(),
                        y1.to_bits(),
                        z1.to_bits(),
                        0.0f32.to_bits(),
                        x2.to_bits(),
                        y2.to_bits(),
                        z2.to_bits(),
                        0.0f32.to_bits(),
                        tl[0].to_bits(),
                        tl[1].to_bits(),
                        tl[2].to_bits(),
                        tl[3].to_bits(),
                        tr[0].to_bits(),
                        tr[1].to_bits(),
                        tr[2].to_bits(),
                        tr[3].to_bits(),
                        bl[0].to_bits(),
                        bl[1].to_bits(),
                        bl[2].to_bits(),
                        bl[3].to_bits(),
                        br[0].to_bits(),
                        br[1].to_bits(),
                        br[2].to_bits(),
                        br[3].to_bits(),
                    ],
                );
                self.encoder.draw_indexed(0..*indices_len as u32, 0, 0..1);
            } else {
                self.encoder
                    .bind_graphics_pipeline(&self.data.pipeline_colored);
                self.encoder.bind_vertex_buffers(0, Some((vertices, 0)));
                self.encoder.bind_index_buffer(IndexBufferView {
                    buffer: indices,
                    offset: 0,
                    index_type: IndexType::U16,
                });
                self.encoder.push_graphics_constants(
                    &self.data.pipeline_layout_colored,
                    ShaderStageFlags::VERTEX,
                    0,
                    &[
                        x1.to_bits(),
                        y1.to_bits(),
                        z1.to_bits(),
                        0.0f32.to_bits(),
                        x2.to_bits(),
                        y2.to_bits(),
                        z2.to_bits(),
                        0.0f32.to_bits(),
                        tl[0].to_bits(),
                        tl[1].to_bits(),
                        tl[2].to_bits(),
                        tl[3].to_bits(),
                        tr[0].to_bits(),
                        tr[1].to_bits(),
                        tr[2].to_bits(),
                        tr[3].to_bits(),
                        bl[0].to_bits(),
                        bl[1].to_bits(),
                        bl[2].to_bits(),
                        bl[3].to_bits(),
                        br[0].to_bits(),
                        br[1].to_bits(),
                        br[2].to_bits(),
                        br[3].to_bits(),
                    ],
                );
                self.encoder.draw_indexed(0..*indices_len as u32, 0, 0..1);
            }
        }
    }

    fn free_mesh(&mut self, [idx, _, _]: IndexPair) {
        // backend.device.destroy_buffer(vertex_buffer);
        // backend.device.destroy_buffer(index_buffer);
        // backend.device.free_memory(vertex_memory);
        // backend.device.free_memory(index_memory);
    }

    fn create_texture(&mut self, width: u32, height: u32, data: &[u8]) -> IndexPair {
        let image = upload_image(
            &self.data.device,
            &mut self.data.image_command_pool,
            &mut self.data.queue_group.queues[0],
            &mut self.data.image_fence,
            &mut self.data.desc_pool,
            &self.data.set_layout,
            &self.data.memory_types,
            &self.data.limits,
            &self.data.sampler,
            data,
            width,
            height,
        );
        let id = self.data.images.len();
        self.data.images.push(image);
        [id, 0, 0]
    }

    fn free_texture(&mut self, [texture, _, _]: IndexPair) {
        // backend.device.destroy_image(image_logo);
        // backend.device.destroy_image_view(image_srv);
        // backend.device.free_memory(image_memory);
    }

    fn resize(&mut self, height: f32) {
        // // FIXME: Resizing doesn't just affect the height when the DPI is not
        // // 100% on at least Windows.
        let window = self.window;
        let dpi = window.get_hidpi_factor();
        let old_logical_size = window.get_inner_size().unwrap();
        let new_physical_size = dpi::PhysicalSize::new(0.0, height as f64).to_logical(dpi);
        let new_logical_size =
            dpi::LogicalSize::new(old_logical_size.width as f64, new_physical_size.height);
        window.set_inner_size(new_logical_size);
    }
}

fn main() {
    env_logger::init();

    let mut events_loop = EventsLoop::new();
    let window_builder = WindowBuilder::new()
        .with_dimensions((300, 500).into())
        .with_title("LiveSplit One")
        .with_window_icon(Some(Icon::from_bytes(include_bytes!("icon.png")).unwrap()))
        .with_resizable(true)
        .with_transparency(true);

    #[cfg(not(feature = "gl"))]
    let (window, mut adapters, mut surface) = {
        let window = window_builder.build(&events_loop).unwrap();
        let instance = back::Instance::create("Foo", 1);
        let surface = instance.create_surface(&window);
        let adapters = instance.enumerate_adapters();
        (window, adapters, surface)
    };
    #[cfg(feature = "gl")]
    let (mut adapters, mut surface) = {
        let window = {
            let builder = back::config_context(
                back::glutin::ContextBuilder::new(),
                format::Rgba8Unorm::SELF,
                None,
            )
            .with_vsync(true)
            .with_hardware_acceleration(None)
            .with_srgb(true);

            back::glutin::GlWindow::new(window_builder, builder, &events_loop).unwrap()
        };

        let surface = back::Surface::from_window(window);
        let adapters = surface.enumerate_adapters();
        (adapters, surface)
    };

    let mut adapter = adapters.swap_remove(0);
    let memory_types = adapter.physical_device.memory_properties().memory_types;
    let limits = adapter.physical_device.limits();

    let (device, queue_group) = adapter
        .open_with::<_, gfx_hal::Graphics>(1, |family| surface.supports_queue_family(family))
        .unwrap();

    let command_pool = unsafe {
        device
            .create_command_pool_typed(&queue_group, pool::CommandPoolCreateFlags::empty())
            .unwrap()
    };
    let image_command_pool = unsafe {
        device
            .create_command_pool_typed(&queue_group, pool::CommandPoolCreateFlags::empty())
            .unwrap()
    };

    let set_layout = unsafe {
        device
            .create_descriptor_set_layout(
                &[
                    pso::DescriptorSetLayoutBinding {
                        binding: 0,
                        ty: pso::DescriptorType::SampledImage,
                        count: 1,
                        stage_flags: ShaderStageFlags::FRAGMENT,
                        immutable_samplers: false,
                    },
                    pso::DescriptorSetLayoutBinding {
                        binding: 1,
                        ty: pso::DescriptorType::Sampler,
                        count: 1,
                        stage_flags: ShaderStageFlags::FRAGMENT,
                        immutable_samplers: false,
                    },
                ],
                &[],
            )
            .unwrap()
    };

    // TODO: We may need more
    const DESCRIPTOR_POOL_SET_COUNT: usize = 256;

    let desc_pool = unsafe {
        device
            .create_descriptor_pool(
                DESCRIPTOR_POOL_SET_COUNT,
                &[
                    pso::DescriptorRangeDesc {
                        ty: pso::DescriptorType::SampledImage,
                        count: 128,
                    },
                    pso::DescriptorRangeDesc {
                        ty: pso::DescriptorType::Sampler,
                        count: 128,
                    },
                ],
            )
            .unwrap()
    };

    let mut frame_semaphore = device.create_semaphore().expect("Can't create semaphore");
    let frame_fence = device.create_fence(false).expect("Can't create fence");
    let image_fence = device.create_fence(false).expect("Can't create fence");

    // Image
    let sampler =
        unsafe { device.create_sampler(SamplerInfo::new(Filter::Linear, WrapMode::Clamp)) }
            .expect("Can't create sampler");

    // Swapchain setup
    let (caps, formats, _present_modes, mut composite_alphas) =
        surface.compatibility(&mut adapter.physical_device);
    let format = formats.map_or(format::Format::Rgba8Unorm, |formats| {
        formats
            .iter()
            .find(|format| format.base_format().1 == ChannelType::Unorm)
            .map(|format| *format)
            .unwrap_or(formats[0])
    });
    // TODO: Use present_modes and composite_alphas

    let mut swap_config = SwapchainConfig::from_caps(&caps, format, DIMS);
    swap_config.composite_alpha = composite_alphas.swap_remove(0);
    swap_config.present_mode = PresentMode::Fifo;
    let extent = swap_config.extent.to_extent();

    let (mut swap_chain, mut backbuffer) =
        unsafe { device.create_swapchain(&mut surface, swap_config, None) }
            .expect("Can't create swapchain");

    #[cfg(feature = "gl")]
    let samples = 1;
    #[cfg(not(feature = "gl"))]
    let samples = 8; // TODO:

    let render_pass = {
        let intermediary_attachment = pass::Attachment {
            format: Some(format),
            samples,
            ops: pass::AttachmentOps::new(
                pass::AttachmentLoadOp::Clear,
                pass::AttachmentStoreOp::DontCare,
            ),
            stencil_ops: pass::AttachmentOps::DONT_CARE,
            layouts: image::Layout::Undefined..image::Layout::ColorAttachmentOptimal,
        };
        let color_attachment = pass::Attachment {
            format: Some(format),
            samples: 1,
            ops: pass::AttachmentOps::new(
                pass::AttachmentLoadOp::DontCare,
                pass::AttachmentStoreOp::Store,
            ),
            stencil_ops: pass::AttachmentOps::DONT_CARE,
            layouts: image::Layout::Undefined..image::Layout::Present,
        };

        let subpass = pass::SubpassDesc {
            colors: &[(0, image::Layout::ColorAttachmentOptimal)],
            depth_stencil: None,
            inputs: &[],
            resolves: &[(1, image::Layout::ColorAttachmentOptimal)],
            preserves: &[],
        };

        // TODO: Is this necessary?
        let dependency = pass::SubpassDependency {
            passes: pass::SubpassRef::External..pass::SubpassRef::Pass(0),
            stages: PipelineStage::COLOR_ATTACHMENT_OUTPUT..PipelineStage::COLOR_ATTACHMENT_OUTPUT,
            accesses: image::Access::empty()
                ..(image::Access::COLOR_ATTACHMENT_READ | image::Access::COLOR_ATTACHMENT_WRITE),
        };

        unsafe {
            device.create_render_pass(
                &[intermediary_attachment, color_attachment],
                &[subpass],
                &[],
            )
        }
        .expect("Can't create render pass")
    };

    let (mut intermediary, mut intermediary_memory) = create_image(
        &device,
        &memory_types,
        extent.width,
        extent.height,
        1,
        samples,
        format,
        Tiling::Optimal,
        Usage::TRANSIENT_ATTACHMENT | Usage::COLOR_ATTACHMENT,
        Properties::DEVICE_LOCAL,
    );
    let mut intermediary_view = unsafe {
        device
            .create_image_view(
                &intermediary,
                image::ViewKind::D2,
                format,
                Swizzle::NO,
                COLOR_RANGE.clone(),
            )
            .unwrap()
    };

    let (mut frame_images, mut framebuffers) = match backbuffer {
        Backbuffer::Images(images) => {
            let pairs = images
                .into_iter()
                .map(|image| unsafe {
                    let rtv = device
                        .create_image_view(
                            &image,
                            image::ViewKind::D2,
                            format,
                            Swizzle::NO,
                            COLOR_RANGE.clone(),
                        )
                        .unwrap();
                    (image, rtv)
                })
                .collect::<Vec<_>>();
            let fbos = pairs
                .iter()
                .map(|&(_, ref rtv)| unsafe {
                    device
                        .create_framebuffer(&render_pass, vec![&intermediary_view, rtv], extent)
                        .unwrap()
                })
                .collect();
            (pairs, fbos)
        }
        Backbuffer::Framebuffer(fbo) => (Vec::new(), vec![fbo]),
    };

    // Pipeline setup
    let pipeline_layout_textured = unsafe {
        device.create_pipeline_layout(
            std::iter::once(&set_layout),
            &[(pso::ShaderStageFlags::VERTEX, 0..24)],
        )
    }
    .expect("Can't create pipeline layout");
    let pipeline_layout_colored =
        unsafe { device.create_pipeline_layout(&[], &[(pso::ShaderStageFlags::VERTEX, 0..24)]) }
            .expect("Can't create pipeline layout");
    let (pipeline_textured, pipeline_colored) = {
        let vs_module = unsafe {
            device
                .create_shader_module(glsl_to_spirv_macros::include_glsl_vs!("src/quad.vert"))
                .unwrap()
        };
        let fs_module_textured = unsafe {
            device
                .create_shader_module(glsl_to_spirv_macros::include_glsl_fs!(
                    "src/quad_textured.frag"
                ))
                .unwrap()
        };
        let fs_module_colored = unsafe {
            device
                .create_shader_module(glsl_to_spirv_macros::include_glsl_fs!(
                    "src/quad_colored.frag"
                ))
                .unwrap()
        };

        let (pipeline_textured, pipeline_colored) = {
            let (vs_entry, fs_entry_textured, fs_entry_colored) = (
                pso::EntryPoint {
                    entry: ENTRY_NAME,
                    module: &vs_module,
                    specialization: pso::Specialization::default(),
                },
                pso::EntryPoint {
                    entry: ENTRY_NAME,
                    module: &fs_module_textured,
                    specialization: pso::Specialization::default(),
                },
                pso::EntryPoint {
                    entry: ENTRY_NAME,
                    module: &fs_module_colored,
                    specialization: pso::Specialization::default(),
                },
            );

            let shader_entries_textured = pso::GraphicsShaderSet {
                vertex: vs_entry.clone(),
                hull: None,
                domain: None,
                geometry: None,
                fragment: Some(fs_entry_textured),
            };
            let shader_entries_colored = pso::GraphicsShaderSet {
                vertex: vs_entry,
                hull: None,
                domain: None,
                geometry: None,
                fragment: Some(fs_entry_colored),
            };

            let subpass = Subpass {
                index: 0,
                main_pass: &render_pass,
            };

            let mut pipeline_desc_textured = pso::GraphicsPipelineDesc::new(
                shader_entries_textured,
                Primitive::TriangleList,
                pso::Rasterizer::FILL,
                &pipeline_layout_textured,
                subpass,
            );
            let mut pipeline_desc_colored = pso::GraphicsPipelineDesc::new(
                shader_entries_colored,
                Primitive::TriangleList,
                pso::Rasterizer::FILL,
                &pipeline_layout_colored,
                subpass,
            );
            pipeline_desc_textured
                .blender
                .targets
                .push(pso::ColorBlendDesc(
                    pso::ColorMask::ALL,
                    pso::BlendState::ALPHA,
                ));
            pipeline_desc_textured
                .vertex_buffers
                .push(pso::VertexBufferDesc {
                    binding: 0,
                    stride: mem::size_of::<Vertex>() as u32,
                    rate: 0,
                });

            pipeline_desc_textured.attributes.push(pso::AttributeDesc {
                location: 0,
                binding: 0,
                element: pso::Element {
                    format: format::Format::Rg32Float,
                    offset: 0,
                },
            });
            pipeline_desc_textured.attributes.push(pso::AttributeDesc {
                location: 1,
                binding: 0,
                element: pso::Element {
                    format: format::Format::Rg32Float,
                    offset: 8,
                },
            });

            pipeline_desc_textured.multisampling = Some(pso::Multisampling {
                rasterization_samples: samples,
                sample_shading: None,
                sample_mask: !0,
                alpha_coverage: false,
                alpha_to_one: false,
            });

            pipeline_desc_colored
                .blender
                .targets
                .push(pso::ColorBlendDesc(
                    pso::ColorMask::ALL,
                    pso::BlendState::ALPHA,
                ));
            pipeline_desc_colored
                .vertex_buffers
                .push(pso::VertexBufferDesc {
                    binding: 0,
                    stride: mem::size_of::<Vertex>() as u32,
                    rate: 0,
                });

            pipeline_desc_colored.attributes.push(pso::AttributeDesc {
                location: 0,
                binding: 0,
                element: pso::Element {
                    format: format::Format::Rg32Float,
                    offset: 0,
                },
            });
            pipeline_desc_colored.attributes.push(pso::AttributeDesc {
                location: 1,
                binding: 0,
                element: pso::Element {
                    format: format::Format::Rg32Float,
                    offset: 8,
                },
            });

            pipeline_desc_colored.multisampling = Some(pso::Multisampling {
                rasterization_samples: samples,
                sample_shading: None,
                sample_mask: !0,
                alpha_coverage: false,
                alpha_to_one: false,
            });

            unsafe {
                (
                    device.create_graphics_pipeline(&pipeline_desc_textured, None),
                    device.create_graphics_pipeline(&pipeline_desc_colored, None),
                )
            }
        };

        unsafe {
            device.destroy_shader_module(vs_module);
            device.destroy_shader_module(fs_module_textured);
            device.destroy_shader_module(fs_module_colored);
        }

        (pipeline_textured.unwrap(), pipeline_colored.unwrap())
    };

    // Rendering setup
    let mut viewport = pso::Viewport {
        rect: pso::Rect {
            x: 0,
            y: 0,
            w: extent.width as _,
            h: extent.height as _,
        },
        depth: 0.0..1.0,
    };

    let mut config = Config::parse("config.toml").unwrap_or_default();

    let run = config.parse_run_or_default();
    let timer = Timer::new(run).unwrap().into_shared();
    if config.is_game_time() {
        timer
            .write()
            .set_current_timing_method(TimingMethod::GameTime);
    }

    let auto_splitter = auto_splitting::Runtime::new(timer.clone());
    config.maybe_load_auto_splitter(&auto_splitter);

    let mut layout = config.parse_layout_or_default();

    let mut renderer = Renderer::new();

    let mut backend = LongLivingData {
        device,
        memory_types,
        command_pool,
        image_command_pool,
        queue_group,
        frame_fence,
        image_fence,
        limits,
        desc_pool,
        set_layout,
        sampler,
        pipeline_textured,
        pipeline_layout_textured,
        pipeline_colored,
        pipeline_layout_colored,

        meshes: vec![],
        images: vec![],
    };

    let mut running = true;
    let mut recreate_swapchain = false;
    let mut resize_dims = Extent2D {
        width: extent.width as _,
        height: extent.height as _,
    };
    while running {
        events_loop.poll_events(|event| {
            if let winit::Event::WindowEvent { event, .. } = event {
                match event {
                    WindowEvent::CloseRequested => running = false,
                    WindowEvent::KeyboardInput {
                        input:
                            KeyboardInput {
                                state: ElementState::Pressed,
                                virtual_keycode: Some(key),
                                ..
                            },
                        ..
                    } => match key {
                        VirtualKeyCode::Numpad1 => timer.write().split_or_start(),
                        VirtualKeyCode::Numpad2 => timer.write().skip_split(),
                        VirtualKeyCode::Numpad3 => timer.write().reset(true),
                        VirtualKeyCode::Numpad4 => timer.write().switch_to_previous_comparison(),
                        VirtualKeyCode::Numpad5 => timer.write().toggle_pause(),
                        VirtualKeyCode::Numpad6 => timer.write().switch_to_next_comparison(),
                        VirtualKeyCode::Numpad8 => timer.write().undo_split(),
                        VirtualKeyCode::Return => config.save_splits(&timer.read()),
                        _ => {}
                    },
                    WindowEvent::MouseWheel { delta, .. } => {
                        let mut scroll = match delta {
                            MouseScrollDelta::LineDelta(_, y) => -y as i32,
                            MouseScrollDelta::PixelDelta(delta) => (delta.y / 15.0) as i32,
                        };
                        while scroll < 0 {
                            layout.scroll_up();
                            scroll += 1;
                        }
                        while scroll > 0 {
                            layout.scroll_down();
                            scroll -= 1;
                        }
                    }
                    // WindowEvent::MouseInput {
                    //     button: MouseButton::Left,
                    //     state: ElementState::Pressed,
                    //     ..
                    // } => {
                    //     dragging = Some((
                    //         cached_mouse_pos,
                    //         display.gl_window().get_position().unwrap(),
                    //     ));
                    // }
                    // WindowEvent::MouseInput {
                    //     button: MouseButton::Left,
                    //     state: ElementState::Released,
                    //     ..
                    // } => {
                    //     dragging = None;
                    // }
                    WindowEvent::DroppedFile(path) => {
                        let mut file = BufReader::new(File::open(&path).unwrap());
                        if composite::parse(&mut file, Some(path.clone()), true)
                            .map_err(drop)
                            .and_then(|run| {
                                timer.write().set_run(run.run).map_err(drop)?;
                                config.set_splits_path(path);
                                Ok(())
                            })
                            .is_err()
                        {
                            let _ = file.seek(SeekFrom::Start(0));
                            if let Ok(settings) = LayoutSettings::from_json(&mut file) {
                                layout = Layout::from_settings(settings);
                            } else {
                                let _ = file.seek(SeekFrom::Start(0));
                                if let Ok(parsed_layout) = layout::parser::parse(&mut file) {
                                    layout = parsed_layout;
                                }
                            }
                        }
                    }
                    winit::WindowEvent::Resized(dims) => {
                        if dims.width as u32 != resize_dims.width
                            || dims.height as u32 != resize_dims.height
                        {
                            #[cfg(feature = "gl")]
                            surface
                                .get_window()
                                .resize(dims.to_physical(surface.get_window().get_hidpi_factor()));
                            recreate_swapchain = true;
                            resize_dims.width = dims.width as u32;
                            resize_dims.height = dims.height as u32;
                        }
                    }
                    _ => (),
                }
            }
        });

        // Window was resized so we must recreate swapchain and framebuffers
        if recreate_swapchain {
            backend.device.wait_idle().unwrap();

            let (caps, formats, _present_modes, mut composite_alphas) =
                surface.compatibility(&mut adapter.physical_device);
            // Verify that previous format still exists so we may reuse it.
            assert!(formats.iter().any(|fs| fs.contains(&format)));

            let mut swap_config = SwapchainConfig::from_caps(&caps, format, resize_dims);
            swap_config.composite_alpha = composite_alphas.swap_remove(0);
            swap_config.present_mode = PresentMode::Fifo;
            let extent = swap_config.extent.to_extent();

            let (new_swap_chain, new_backbuffer) = unsafe {
                backend
                    .device
                    .create_swapchain(&mut surface, swap_config, Some(swap_chain))
            }
            .expect("Can't create swapchain");

            unsafe {
                // Clean up the old framebuffers, images and swapchain
                backend.device.destroy_image_view(intermediary_view);
                backend.device.destroy_image(intermediary);
                backend.device.free_memory(intermediary_memory);
                for framebuffer in framebuffers {
                    backend.device.destroy_framebuffer(framebuffer);
                }
                for (_, rtv) in frame_images {
                    backend.device.destroy_image_view(rtv);
                }
            }

            let intermediary_image = create_image(
                &backend.device,
                &backend.memory_types,
                extent.width,
                extent.height,
                1,
                samples,
                format,
                Tiling::Optimal,
                Usage::TRANSIENT_ATTACHMENT | Usage::COLOR_ATTACHMENT,
                Properties::DEVICE_LOCAL,
            );
            intermediary = intermediary_image.0;
            intermediary_memory = intermediary_image.1;
            intermediary_view = unsafe {
                backend
                    .device
                    .create_image_view(
                        &intermediary,
                        image::ViewKind::D2,
                        format,
                        Swizzle::NO,
                        COLOR_RANGE.clone(),
                    )
                    .unwrap()
            };

            backbuffer = new_backbuffer;
            swap_chain = new_swap_chain;

            let (new_frame_images, new_framebuffers) = match backbuffer {
                Backbuffer::Images(images) => {
                    let pairs = images
                        .into_iter()
                        .map(|image| unsafe {
                            let rtv = backend
                                .device
                                .create_image_view(
                                    &image,
                                    image::ViewKind::D2,
                                    format,
                                    Swizzle::NO,
                                    COLOR_RANGE.clone(),
                                )
                                .unwrap();
                            (image, rtv)
                        })
                        .collect::<Vec<_>>();
                    let fbos = pairs
                        .iter()
                        .map(|&(_, ref rtv)| unsafe {
                            backend
                                .device
                                .create_framebuffer(
                                    &render_pass,
                                    vec![&intermediary_view, rtv],
                                    extent,
                                )
                                .unwrap()
                        })
                        .collect();
                    (pairs, fbos)
                }
                Backbuffer::Framebuffer(fbo) => (Vec::new(), vec![fbo]),
            };

            framebuffers = new_framebuffers;
            frame_images = new_frame_images;
            viewport.rect.w = extent.width as _;
            viewport.rect.h = extent.height as _;
            recreate_swapchain = false;
        }

        let frame = unsafe {
            backend.device.reset_fence(&backend.frame_fence).unwrap();
            backend.command_pool.reset();
            match swap_chain.acquire_image(!0, FrameSync::Semaphore(&mut frame_semaphore)) {
                Ok(i) => i,
                Err(_) => {
                    recreate_swapchain = true;
                    continue;
                }
            }
        };

        // Rendering
        let mut cmd_buffer = backend
            .command_pool
            .acquire_command_buffer::<command::OneShot>();
        unsafe {
            cmd_buffer.begin();

            cmd_buffer.set_viewports(0, &[viewport.clone()]);
            cmd_buffer.set_scissors(0, &[viewport.rect]);

            {
                let mut encoder = cmd_buffer.begin_render_pass_inline(
                    &render_pass,
                    &framebuffers[frame as usize],
                    viewport.rect,
                    &[
                        command::ClearValue::Color(command::ClearColor::Float([
                            0.0, 0.0, 0.0, 0.0,
                        ])),
                        command::ClearValue::Color(command::ClearColor::Float([
                            0.0, 0.0, 0.0, 0.0,
                        ])),
                    ],
                );

                let layout_state = layout.state(&timer.read());

                if resize_dims.height > 0 {
                    #[cfg(feature = "gl")]
                    let window = surface.window();

                    renderer.render(
                        &mut GfxBackend {
                            data: &mut backend,
                            encoder: &mut encoder,
                            window: &window,
                        },
                        (resize_dims.width as _, resize_dims.height as _),
                        &layout_state,
                    );
                }
            }

            cmd_buffer.finish();

            let submission = Submission {
                command_buffers: Some(&cmd_buffer),
                wait_semaphores: Some((&frame_semaphore, PipelineStage::BOTTOM_OF_PIPE)),
                signal_semaphores: &[],
            };
            backend.queue_group.queues[0].submit(submission, Some(&mut backend.frame_fence));

            // TODO: replace with semaphore
            backend
                .device
                .wait_for_fence(&backend.frame_fence, !0)
                .unwrap();
            backend.command_pool.free(Some(cmd_buffer));

            // present frame
            if let Err(_) =
                swap_chain.present_nosemaphores(&mut backend.queue_group.queues[0], frame)
            {
                recreate_swapchain = true;
            }
        }
    }

    // cleanup!
    backend.device.wait_idle().unwrap();
    unsafe {
        backend
            .device
            .destroy_command_pool(backend.command_pool.into_raw());
        backend
            .device
            .destroy_command_pool(backend.image_command_pool.into_raw());
        backend.device.destroy_descriptor_pool(backend.desc_pool);
        backend
            .device
            .destroy_descriptor_set_layout(backend.set_layout);

        backend.device.destroy_sampler(backend.sampler);
        backend.device.destroy_fence(backend.frame_fence);
        backend.device.destroy_fence(backend.image_fence);
        backend.device.destroy_semaphore(frame_semaphore);
        backend.device.destroy_render_pass(render_pass);
        backend
            .device
            .destroy_graphics_pipeline(backend.pipeline_textured);
        backend
            .device
            .destroy_graphics_pipeline(backend.pipeline_colored);
        backend
            .device
            .destroy_pipeline_layout(backend.pipeline_layout_textured);
        backend
            .device
            .destroy_pipeline_layout(backend.pipeline_layout_colored);

        // Clean up swapchain
        backend.device.destroy_image_view(intermediary_view);
        backend.device.destroy_image(intermediary);
        backend.device.free_memory(intermediary_memory);
        for framebuffer in framebuffers {
            backend.device.destroy_framebuffer(framebuffer);
        }
        for (_, rtv) in frame_images {
            backend.device.destroy_image_view(rtv);
        }

        backend.device.destroy_swapchain(swap_chain);
    }
}

fn create_image<D, B>(
    device: &D,
    memory_types: &[MemoryType],
    width: image::Size,
    height: image::Size,
    mip_levels: image::Level,
    samples: image::NumSamples,
    format: format::Format,
    tiling: Tiling,
    usage: Usage,
    properties: memory::Properties,
) -> (B::Image, B::Memory)
where
    D: Device<B>,
    B: gfx_hal::Backend,
{
    unsafe {
        let mut image = device
            .create_image(
                ImageKind::D2(width, height, 1, samples),
                mip_levels,
                format,
                tiling,
                usage,
                ViewCapabilities::empty(),
            )
            .unwrap();

        let image_req = device.get_image_requirements(&image);

        let device_type = memory_types
            .iter()
            .enumerate()
            .position(|(id, memory_type)| {
                image_req.type_mask & (1 << id) != 0 && memory_type.properties.contains(properties)
            })
            .unwrap()
            .into();

        let image_memory = device.allocate_memory(device_type, image_req.size).unwrap();
        device
            .bind_image_memory(&image_memory, 0, &mut image)
            .unwrap();

        (image, image_memory)
    }
}

fn upload_buffer<D, B, T>(
    device: &D,
    memory_types: &[MemoryType],
    elements: &[T],
    usage: buffer::Usage,
) -> (B::Buffer, B::Memory)
where
    D: Device<B>,
    B: gfx_hal::Backend,
    T: Copy,
{
    let buffer_stride = mem::size_of::<T>() as u64;
    let buffer_len = elements.len() as u64 * buffer_stride;
    assert_ne!(buffer_len, 0);

    let mut buffer = unsafe { device.create_buffer(buffer_len, usage) }.unwrap();

    let buffer_req = unsafe { device.get_buffer_requirements(&buffer) };

    let upload_type = memory_types
        .iter()
        .enumerate()
        .position(|(id, mem_type)| {
            // type_mask is a bit field where each bit represents a memory type. If the bit is set
            // to 1 it means we can use that type for our buffer. So this code finds the first
            // memory type that has a `1` (or, is allowed), and is visible to the CPU.
            buffer_req.type_mask & (1 << id) != 0
                && mem_type
                    .properties
                    .contains(memory::Properties::CPU_VISIBLE)
        })
        .unwrap()
        .into();

    let buffer_memory = unsafe { device.allocate_memory(upload_type, buffer_req.size) }.unwrap();

    unsafe { device.bind_buffer_memory(&buffer_memory, 0, &mut buffer) }.unwrap();

    unsafe {
        let mut writer = device
            .acquire_mapping_writer::<T>(&buffer_memory, 0..buffer_req.size)
            .unwrap();
        writer[0..elements.len()].copy_from_slice(elements);
        device.release_mapping_writer(writer).unwrap();
    }

    (buffer, buffer_memory)
}

fn upload_image<D, B>(
    device: &D,
    command_pool: &mut CommandPool<B, queue::capability::Graphics>,
    queue: &mut CommandQueue<B, queue::capability::Graphics>,
    fence: &mut B::Fence,
    desc_pool: &mut <B as gfx_hal::Backend>::DescriptorPool,
    set_layout: &B::DescriptorSetLayout,
    memory_types: &[MemoryType],
    limits: &Limits,
    sampler: &B::Sampler,
    image_data: &[u8],
    width: u32,
    height: u32,
) -> (B::DescriptorSet, B::ImageView, B::Image, B::Memory)
where
    D: Device<B>,
    B: gfx_hal::Backend,
{
    let kind = ImageKind::D2(width as ImageSize, height as ImageSize, 1, 1);
    let row_alignment_mask = limits.min_buffer_copy_pitch_alignment as u32 - 1;
    let image_stride = 4usize;
    let row_pitch = (width * image_stride as u32 + row_alignment_mask) & !row_alignment_mask;
    let upload_size = (height * row_pitch) as u64;

    let mut image_upload_buffer =
        unsafe { device.create_buffer(upload_size, buffer::Usage::TRANSFER_SRC) }.unwrap();
    let image_mem_reqs = unsafe { device.get_buffer_requirements(&image_upload_buffer) };
    let image_upload_type = memory_types
        .iter()
        .enumerate()
        .position(|(id, mem_type)| {
            // type_mask is a bit field where each bit represents a memory type. If the bit is set
            // to 1 it means we can use that type for our buffer. So this code finds the first
            // memory type that has a `1` (or, is allowed), and is visible to the CPU.
            image_mem_reqs.type_mask & (1 << id) != 0
                && mem_type
                    .properties
                    .contains(memory::Properties::CPU_VISIBLE)
        })
        .unwrap()
        .into();
    let image_upload_memory =
        unsafe { device.allocate_memory(image_upload_type, image_mem_reqs.size) }.unwrap();

    unsafe { device.bind_buffer_memory(&image_upload_memory, 0, &mut image_upload_buffer) }
        .unwrap();

    // copy image data into staging buffer
    unsafe {
        let mut data = device
            .acquire_mapping_writer::<u8>(&image_upload_memory, 0..image_mem_reqs.size)
            .unwrap();
        for y in 0..height as usize {
            let row = &(*image_data)
                [y * (width as usize) * image_stride..(y + 1) * (width as usize) * image_stride];
            let dest_base = y * row_pitch as usize;
            data[dest_base..dest_base + row.len()].copy_from_slice(row);
        }
        device.release_mapping_writer(data).unwrap();
    }

    let mip_levels = (width.max(height) as f64).log2().floor() as u8 + 1;
    let mut image = unsafe {
        device.create_image(
            kind,
            mip_levels,
            format::Rgba8Unorm::SELF,
            Tiling::Optimal,
            Usage::TRANSFER_SRC | Usage::TRANSFER_DST | Usage::SAMPLED,
            ViewCapabilities::empty(),
        )
    }
    .unwrap(); // TODO: usage
    let image_req = unsafe { device.get_image_requirements(&image) };

    let device_type = memory_types
        .iter()
        .enumerate()
        .position(|(id, memory_type)| {
            image_req.type_mask & (1 << id) != 0
                && memory_type.properties.contains(Properties::DEVICE_LOCAL)
        })
        .unwrap()
        .into();
    let image_memory = unsafe { device.allocate_memory(device_type, image_req.size) }.unwrap();

    let full_range = SubresourceRange {
        aspects: Aspects::COLOR,
        levels: 0..mip_levels,
        layers: 0..1,
    };

    unsafe { device.bind_image_memory(&image_memory, 0, &mut image) }.unwrap();
    let image_srv = unsafe {
        device.create_image_view(
            &image,
            ViewKind::D2,
            format::Rgba8Unorm::SELF,
            Swizzle::NO,
            full_range.clone(),
        )
    }
    .unwrap();

    // copy buffer to texture
    unsafe {
        command_pool.reset();
        let mut cmd_buffer = command_pool.acquire_command_buffer::<command::OneShot>();
        cmd_buffer.begin();

        cmd_buffer.pipeline_barrier(
            PipelineStage::TOP_OF_PIPE..PipelineStage::TRANSFER,
            memory::Dependencies::empty(),
            Some(memory::Barrier::Image {
                states: (image::Access::empty(), image::Layout::Undefined)
                    ..(
                        image::Access::TRANSFER_WRITE,
                        image::Layout::TransferDstOptimal,
                    ),
                target: &image,
                families: None,
                range: full_range.clone(),
            }),
        );

        cmd_buffer.copy_buffer_to_image(
            &image_upload_buffer,
            &image,
            image::Layout::TransferDstOptimal,
            &[command::BufferImageCopy {
                buffer_offset: 0,
                buffer_width: row_pitch / (image_stride as u32),
                buffer_height: height as u32,
                image_layers: image::SubresourceLayers {
                    aspects: format::Aspects::COLOR,
                    level: 0,
                    layers: 0..1,
                },
                image_offset: image::Offset { x: 0, y: 0, z: 0 },
                image_extent: image::Extent {
                    width,
                    height,
                    depth: 1,
                },
            }],
        );

        let (mut mip_width, mut mip_height) = (width as i32, height as i32);

        for mip_level in 1..mip_levels {
            cmd_buffer.pipeline_barrier(
                PipelineStage::TRANSFER..PipelineStage::TRANSFER,
                memory::Dependencies::empty(),
                Some(memory::Barrier::Image {
                    states: (
                        image::Access::TRANSFER_WRITE,
                        image::Layout::TransferDstOptimal,
                    )
                        ..(
                            image::Access::TRANSFER_READ,
                            image::Layout::TransferSrcOptimal,
                        ),
                    target: &image,
                    families: None,
                    range: SubresourceRange {
                        aspects: Aspects::COLOR,
                        levels: mip_level - 1..mip_level,
                        layers: 0..1,
                    },
                }),
            );

            cmd_buffer.blit_image(
                &image,
                image::Layout::TransferSrcOptimal,
                &image,
                image::Layout::TransferDstOptimal,
                image::Filter::Linear,
                Some(command::ImageBlit {
                    src_subresource: image::SubresourceLayers {
                        aspects: format::Aspects::COLOR,
                        level: mip_level - 1,
                        layers: 0..1,
                    },
                    src_bounds: image::Offset::ZERO..image::Offset {
                        x: mip_width,
                        y: mip_height,
                        z: 1,
                    },
                    dst_subresource: image::SubresourceLayers {
                        aspects: format::Aspects::COLOR,
                        level: mip_level,
                        layers: 0..1,
                    },
                    dst_bounds: image::Offset::ZERO..image::Offset {
                        x: if mip_width > 1 { mip_width / 2 } else { 1 },
                        y: if mip_height > 1 { mip_height / 2 } else { 1 },
                        z: 1,
                    },
                }),
            );

            cmd_buffer.pipeline_barrier(
                PipelineStage::TRANSFER..PipelineStage::FRAGMENT_SHADER,
                memory::Dependencies::empty(),
                Some(memory::Barrier::Image {
                    states: (
                        image::Access::TRANSFER_READ,
                        image::Layout::TransferSrcOptimal,
                    )
                        ..(
                            image::Access::SHADER_READ,
                            image::Layout::ShaderReadOnlyOptimal,
                        ),
                    target: &image,
                    families: None,
                    range: SubresourceRange {
                        aspects: Aspects::COLOR,
                        levels: mip_level - 1..mip_level,
                        layers: 0..1,
                    },
                }),
            );

            if mip_width > 1 {
                mip_width /= 2;
            }
            if mip_height > 1 {
                mip_height /= 2;
            }
        }

        cmd_buffer.pipeline_barrier(
            PipelineStage::TRANSFER..PipelineStage::FRAGMENT_SHADER,
            memory::Dependencies::empty(),
            Some(memory::Barrier::Image {
                states: (
                    image::Access::TRANSFER_WRITE,
                    image::Layout::TransferDstOptimal,
                )
                    ..(
                        image::Access::SHADER_READ,
                        image::Layout::ShaderReadOnlyOptimal,
                    ),
                target: &image,
                families: None,
                range: SubresourceRange {
                    aspects: Aspects::COLOR,
                    levels: mip_levels - 1..mip_levels,
                    layers: 0..1,
                },
            }),
        );

        cmd_buffer.finish();

        device.reset_fence(fence).unwrap();

        queue.submit_nosemaphores(Some(&cmd_buffer), Some(fence));

        device
            .wait_for_fence(fence, !0)
            .expect("Can't wait for fence");

        command_pool.free(Some(cmd_buffer));
        device.destroy_buffer(image_upload_buffer);
        device.free_memory(image_upload_memory);

        let desc_set = desc_pool.allocate_set(set_layout).unwrap();

        device.write_descriptor_sets(vec![
            pso::DescriptorSetWrite {
                set: &desc_set,
                binding: 0,
                array_offset: 0,
                descriptors: Some(pso::Descriptor::Image(&image_srv, ImageLayout::Undefined)),
            },
            pso::DescriptorSetWrite {
                set: &desc_set,
                binding: 1,
                array_offset: 0,
                descriptors: Some(pso::Descriptor::Sampler(sampler)),
            },
        ]);

        (desc_set, image_srv, image, image_memory)
    }
}
