#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod stream_markers;

use crate::config::Config;
use livesplit_core::{layout::LayoutState, rendering::software::Renderer, HotkeySystem, Timer};
use minifb::{Key, KeyRepeat};

fn main() {
    let config = Config::parse("config.yaml").unwrap_or_default();
    config.setup_logging();

    let run = config.parse_run_or_default();
    let timer = Timer::new(run).unwrap().into_shared();
    config.configure_timer(&mut timer.write());

    let mut markers = config.build_marker_client();

    let mut hotkey_system = HotkeySystem::new(timer.clone()).unwrap();
    config.configure_hotkeys(&mut hotkey_system);

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

        if window.is_key_pressed(Key::Enter, KeyRepeat::No) {
            config.save_splits(&timer.read());
        }

        let (width, height) = window.get_size();

        {
            let timer = timer.read();
            markers.tick(&timer);
            layout.update_state(&mut layout_state, &timer.snapshot());
        }
        renderer.render(&layout_state, [width as _, height as _]);

        buf.resize(width * height, 0);

        transpose(
            bytemuck::cast_slice_mut(&mut buf),
            bytemuck::cast_slice(renderer.image_data()),
        );

        window.update_with_buffer(&buf, width, height).unwrap();
    }
}

pub fn transpose(dst: &mut [[u8; 4]], src: &[[u8; 4]]) {
    #[repr(transparent)]
    pub struct Chunk([[u8; 4]; 4]);

    unsafe {
        let (dst_before, dst, dst_after) = dst.align_to_mut::<Chunk>();
        let (src_before, src, src_after) = src.align_to::<Chunk>();

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
}
