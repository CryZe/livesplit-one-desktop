#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod stream_markers;

use crate::config::Config;
use bytemuck::{Pod, Zeroable};
use livesplit_core::{auto_splitting, layout::LayoutState, rendering::software::Renderer, Timer};
use mimalloc::MiMalloc;
use minifb::{Key, KeyRepeat};

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

fn main() {
    let config = Config::parse("config.yaml").unwrap_or_default();
    config.setup_logging();

    let run = config.parse_run_or_default();
    let timer = Timer::new(run).unwrap().into_shared();
    config.configure_timer(&mut timer.write().unwrap());

    let mut markers = config.build_marker_client();

    let auto_splitter = auto_splitting::Runtime::new(timer.clone());
    config.maybe_load_auto_splitter(&auto_splitter);

    let _hotkey_system = config.create_hotkey_system(timer.clone());

    let mut layout = config.parse_layout_or_default();

    let mut window = config.build_window().unwrap();

    let mut renderer = Renderer::new();
    let mut layout_state = LayoutState::default();
    let mut buf = Vec::new();

    while window.is_open() {
        if let Some((_, val)) = window.get_scroll_wheel() {
            if val > 0.0 {
                layout.scroll_up();
            } else if val < 0.0 {
                layout.scroll_down();
            }
        }

        if window.is_key_pressed(Key::S, KeyRepeat::No)
            && (window.is_key_down(Key::LeftCtrl) || window.is_key_down(Key::RightCtrl))
        {
            config.save_splits(&timer.read().unwrap());
        }

        let (width, height) = window.get_size();
        if width != 0 && height != 0 {
            {
                let timer = timer.read().unwrap();
                markers.tick(&timer);
                layout.update_state(&mut layout_state, &timer.snapshot());
            }
            renderer.render(&layout_state, [width as _, height as _]);

            buf.resize(width * height, 0);

            transpose(
                bytemuck::cast_slice_mut(&mut buf),
                bytemuck::cast_slice(renderer.image_data()),
            );
        }
        window.update_with_buffer(&buf, width, height).unwrap();
    }
}

pub fn transpose(dst: &mut [[u8; 4]], src: &[[u8; 4]]) {
    #[derive(Copy, Clone, Pod, Zeroable)]
    #[repr(transparent)]
    pub struct Chunk([[u8; 4]; 8]);

    let (dst_before, dst, dst_after) = bytemuck::pod_align_to_mut::<_, Chunk>(dst);
    let (src_before, src, src_after) = bytemuck::pod_align_to::<_, Chunk>(src);

    for (dst, &[r, g, b, a]) in dst_before.iter_mut().zip(src_before) {
        *dst = [b, g, r, a];
    }
    for (dst, src) in dst.iter_mut().zip(src) {
        for (dst, &[r, g, b, a]) in dst.0.iter_mut().zip(&src.0) {
            *dst = [b, g, r, a];
        }
    }
    for (dst, &[r, g, b, a]) in dst_after.iter_mut().zip(src_after) {
        *dst = [b, g, r, a];
    }
}
