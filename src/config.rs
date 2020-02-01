use {
    crate::stream_markers,
    image::{png::PNGDecoder, ImageDecoder},
    livesplit_core::{
        layout::{self, Layout, LayoutSettings},
        run::{parser::composite, saver::livesplit::save_timer},
        HotkeyConfig, HotkeySystem, Run, Segment, Timer, TimingMethod,
    },
    serde::Deserialize,
    std::{
        fs::{self, File},
        io::{BufReader, BufWriter, Seek, SeekFrom},
        path::{Path, PathBuf},
    },
    winit::{
        dpi::LogicalSize,
        window::{Icon, WindowBuilder},
    },
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
    width: u32,
    height: u32,
    always_on_top: bool,
    transparency: bool,
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

    pub fn set_splits_path(&mut self, path: PathBuf) {
        self.general.splits = Some(path);
    }

    pub fn configure_hotkeys(&self, hotkeys: &mut HotkeySystem) {
        hotkeys.set_config(self.hotkeys.clone()).ok();
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

    pub fn build_window(&self) -> WindowBuilder {
        let icon_reader = PNGDecoder::new(&include_bytes!("icon.png")[..]).unwrap();
        let (width, height) = icon_reader.dimensions();
        let icon_bytes = icon_reader.read_image().unwrap();

        let builder = WindowBuilder::new()
            .with_inner_size(LogicalSize {
                width: self.window.width,
                height: self.window.height,
            })
            .with_title("LiveSplit One")
            .with_window_icon(Some(
                Icon::from_rgba(icon_bytes, width as _, height as _).unwrap(),
            ))
            .with_resizable(true)
            .with_always_on_top(self.window.always_on_top)
            .with_transparent(self.window.transparency);

        builder
    }

    pub fn build_marker_client(&self) -> stream_markers::Client {
        stream_markers::Client::new(self.connections.twitch.as_ref().map(String::as_str))
    }
}
