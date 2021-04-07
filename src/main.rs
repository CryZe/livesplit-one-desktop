#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
// mod renderer;
mod stream_markers;

use crate::config::Config;
use livesplit_core::{
    layout::{self, Layout, LayoutSettings, LayoutState},
    rendering::software::BorrowedSoftwareRenderer,
    run::parser::composite,
    HotkeySystem, Timer,
};
use pixels::{Pixels, SurfaceTexture};
use std::{
    fs::File,
    io::{prelude::*, BufReader, SeekFrom},
};
use winit::{
    dpi::PhysicalSize,
    event::{ElementState, Event, KeyboardInput, MouseScrollDelta, VirtualKeyCode, WindowEvent},
    event_loop::{ControlFlow, EventLoop},
};

fn main() {
    let mut config = Config::parse("config.yaml").unwrap_or_default();
    config.setup_logging();

    let run = config.parse_run_or_default();
    let timer = Timer::new(run).unwrap().into_shared();
    config.configure_timer(&mut timer.write());

    let mut markers = config.build_marker_client();

    let mut hotkey_system = HotkeySystem::new(timer.clone()).unwrap();
    config.configure_hotkeys(&mut hotkey_system);

    let mut layout = config.parse_layout_or_default();

    #[cfg(windows)]
    use winit::platform::windows::EventLoopExtWindows;
    #[cfg(windows)]
    let event_loop = EventLoop::<()>::new_no_raw_input();
    #[cfg(not(windows))]
    let event_loop = EventLoop::new();

    let window = config.build_window().build(&event_loop).unwrap();

    let size = window.inner_size();
    let mut renderer = BorrowedSoftwareRenderer::new();

    let mut pixels = {
        let surface_texture = SurfaceTexture::new(size.width, size.height, &window);
        Pixels::new(size.width, size.height, surface_texture)
            .expect("Failed to init the pixel buffer")
    };

    let mut layout_state = LayoutState::default();

    event_loop.run(move |event, _, control_flow| match event {
        Event::MainEventsCleared => window.request_redraw(),
        Event::RedrawRequested(..) => {
            let timer = timer.read();
            markers.tick(&timer);
            layout.update_state(&mut layout_state, &timer.snapshot());
            drop(timer);

            let frame = pixels.get_frame();

            if let Some((width, height)) =
                renderer.render(&layout_state, frame, [size.width, size.height], size.width)
            {
                window.set_inner_size(PhysicalSize {
                    width: width.round() as u32,
                    height: height.round() as u32,
                });
            }

            if let Err(e) = pixels.render() {
                log::error!("{}", e);
            }
        }
        Event::WindowEvent { event, .. } => match event {
            WindowEvent::CloseRequested => *control_flow = ControlFlow::Exit,
            WindowEvent::KeyboardInput {
                input:
                    KeyboardInput {
                        state: ElementState::Pressed,
                        virtual_keycode: Some(VirtualKeyCode::Return),
                        ..
                    },
                ..
            } => config.save_splits(&timer.read()),
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
            WindowEvent::Resized(new_size) => pixels.resize(new_size.width, new_size.height),
            _ => {}
        },
        _ => {}
    });
}
