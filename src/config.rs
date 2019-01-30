use {
    livesplit_core::{
        auto_splitting,
        layout::{self, Layout, LayoutSettings},
        run::{parser::composite, saver::livesplit::save_timer},
        Run, Segment, Timer, TimingMethod,
    },
    serde::Deserialize,
    std::{
        fs::{self, File},
        io::{BufReader, BufWriter, Seek, SeekFrom},
        path::{Path, PathBuf},
    },
};

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct Config {
    general: General,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
struct General {
    splits: Option<PathBuf>,
    layout: Option<PathBuf>,
    auto_splitter: Option<PathBuf>,
    timing_method: Option<TimingMethod>,
}

impl Config {
    pub fn parse(path: impl AsRef<Path>) -> Option<Config> {
        let buf = fs::read(path).ok()?;
        toml::from_slice(&buf).ok()
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

    pub fn maybe_load_auto_splitter(&self, runtime: &auto_splitting::Runtime) {
        if let Some(auto_splitter) = &self.general.auto_splitter {
            if let Ok(buf) = fs::read(auto_splitter) {
                runtime.load_script(buf).ok();
            }
        }
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

    pub fn save_splits(&self, timer: &Timer) {
        if let Some(path) = &self.general.splits {
            // TODO: Don't ignore not being able to save.
            if let Ok(file) = File::create(path) {
                save_timer(timer, BufWriter::new(file)).ok();
            }
        }
    }
}
