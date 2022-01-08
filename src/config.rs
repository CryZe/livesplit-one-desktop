use crate::stream_markers;
use core::fmt;
use livesplit_core::{
    auto_splitting,
    layout::{self, Layout, LayoutSettings},
    run::{parser::composite, saver::livesplit::save_timer},
    HotkeyConfig, HotkeySystem, Run, Segment, SharedTimer, Timer, TimingMethod,
};
use serde::Deserialize;
use std::{
    fs::{self, File},
    io::{BufReader, BufWriter, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    #[serde(default)]
    general: General,
    log: Option<Log>,
    #[serde(default)]
    window: Window,
    #[serde(default)]
    hotkeys: HotkeyConfig,
    #[serde(default)]
    connections: Connections,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct General {
    splits: Option<PathBuf>,
    layout: Option<PathBuf>,
    timing_method: Option<TimingMethod>,
    comparison: Option<String>,
    auto_splitter: Option<PathBuf>,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct Log {
    path: PathBuf,
    level: Option<log::LevelFilter>,
    #[serde(default)]
    clear: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "kebab-case")]
#[serde(default)]
struct Window {
    width: usize,
    height: usize,
    always_on_top: bool,
    transparency: bool,
    fps: f32,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
#[serde(default)]
struct Connections {
    twitch: Option<String>,
}

impl Default for Window {
    fn default() -> Window {
        Self {
            width: 300,
            height: 500,
            always_on_top: false,
            transparency: true,
            fps: 60.0,
        }
    }
}

impl Config {
    pub fn parse(path: impl AsRef<Path>) -> Option<Config> {
        let buf = fs::read(path).ok()?;
        serde_yaml::from_slice(&buf).ok()
    }

    pub fn parse_run(&self) -> Option<Run> {
        let path = self.general.splits.clone()?;
        let file = BufReader::new(File::open(&path).ok()?);
        let mut run = composite::parse(file, Some(path), true).ok()?.run;
        run.fix_splits();
        Some(run)
    }

    pub fn parse_run_or_default(&self) -> Run {
        self.parse_run().unwrap_or_else(|| {
            let mut run = Run::new();
            run.set_game_name("Game");
            run.set_category_name("Category");
            run.push_segment(Segment::new("Time"));
            run
        })
    }

    pub fn is_game_time(&self) -> bool {
        self.general.timing_method == Some(TimingMethod::GameTime)
    }

    pub fn parse_layout(&self) -> Option<Layout> {
        let path = self.general.layout.as_ref()?;
        let mut file = BufReader::new(File::open(path).ok()?);
        if let Ok(settings) = LayoutSettings::from_json(&mut file) {
            return Some(Layout::from_settings(settings));
        }
        file.seek(SeekFrom::Start(0)).ok()?;
        layout::parser::parse(file).ok()
    }

    pub fn parse_layout_or_default(&self) -> Layout {
        self.parse_layout().unwrap_or_else(Layout::default_layout)
    }

    // pub fn set_splits_path(&mut self, path: PathBuf) {
    //     self.general.splits = Some(path);
    // }

    pub fn create_hotkey_system(&self, timer: SharedTimer) -> Option<HotkeySystem> {
        HotkeySystem::with_config(timer, self.hotkeys).ok()
    }

    pub fn configure_timer(&self, timer: &mut Timer) {
        if self.is_game_time() {
            timer.set_current_timing_method(TimingMethod::GameTime);
        }
        if let Some(comparison) = &self.general.comparison {
            timer.set_current_comparison(comparison).ok();
        }
    }

    pub fn save_splits(&self, timer: &Timer) {
        if let Some(path) = &self.general.splits {
            // FIXME: Don't ignore not being able to save.
            if let Ok(file) = File::create(path) {
                save_timer(timer, BufWriter::new(file)).ok();
            }
        }
    }

    pub fn setup_logging(&self) {
        if let Some(log) = &self.log {
            if let Ok(log_file) = fs::OpenOptions::new()
                .create(true)
                .write(true)
                .append(!log.clear)
                .truncate(log.clear)
                .open(&log.path)
            {
                fern::Dispatch::new()
                    .format(|out, message, record| {
                        out.finish(format_args!(
                            "{}[{}][{}] {}",
                            chrono::Local::now().format("[%Y-%m-%d][%H:%M:%S]"),
                            record.target(),
                            record.level(),
                            message
                        ))
                    })
                    .level(log.level.unwrap_or(log::LevelFilter::Warn))
                    .chain(log_file)
                    .apply()
                    .ok();

                #[cfg(not(debug_assertions))]
                {
                    std::panic::set_hook(Box::new(|panic_info| {
                        log::error!(target: "PANIC", "{}\n{:?}", panic_info, backtrace::Backtrace::new());
                    }));
                }
            }
        }
    }

    pub fn build_window(&self) -> Result<minifb::Window, minifb::Error> {
        let mut window = minifb::Window::new(
            "LiveSplit One",
            self.window.width,
            self.window.height,
            minifb::WindowOptions {
                // borderless: true,
                resize: true,
                topmost: self.window.always_on_top,
                // transparency: self.window.transparency,
                ..Default::default()
            },
        )?;

        window.limit_update_rate(Some(Duration::from_secs_f32(self.window.fps.recip())));

        Ok(window)
    }

    pub fn build_marker_client(&self) -> stream_markers::Client {
        stream_markers::Client::new(self.connections.twitch.as_deref())
    }

    pub fn maybe_load_auto_splitter(&self, runtime: &auto_splitting::Runtime) {
        if let Some(auto_splitter) = &self.general.auto_splitter {
            if let Err(e) = runtime.load_script(auto_splitter.clone()) {
                log::error!("Auto Splitter failed to load: {}", ErrorChain(&e));
            }
        }
    }
}

struct ErrorChain<'a>(&'a dyn std::error::Error);

impl fmt::Display for ErrorChain<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut error = self.0;
        fmt::Display::fmt(error, f)?;
        if error.source().is_some() {
            f.write_str("\nCaused by:\n")?;
        }
        while let Some(source) = error.source() {
            error = source;
            fmt::Display::fmt(error, f)?;
            f.write_str("\n")?;
        }
        Ok(())
    }
}
