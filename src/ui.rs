use std::{
    sync::OnceLock,
    time::{Duration, Instant},
};

use sdl2::{
    keyboard::Keycode,
    pixels::Color,
    rect::Rect,
    render::{BlendMode, Canvas},
    video::Window,
};

use crate::{
    config::{CounterMode, NormalizationMethod, RuntimeProfile},
    library::AudioTrack,
    queue::{match_tracks_ordered, next_queue_version, queue_exists},
    timing::SessionPlan,
    tone::TestTone,
};

const BACKGROUND: Color = Color::RGB(12, 15, 16);
const PANEL: Color = Color::RGB(27, 31, 29);
const PHOSPHOR: Color = Color::RGB(112, 224, 141);
const AMBER: Color = Color::RGB(242, 178, 74);
const CREAM: Color = Color::RGB(220, 218, 196);
const DIM: Color = Color::RGB(88, 101, 91);
const RED: Color = Color::RGB(211, 76, 65);
const VISIBLE_TRACKS: usize = 7;
const TRACK_NAME_WIDTH: usize = 14;
pub const STATUS_TIMEOUT: Duration = Duration::from_secs(3);

pub struct AppUi {
    tracks: Vec<AudioTrack>,
    cursor: usize,
    viewport_start: usize,
    selected: Vec<bool>,
    selection_order: Vec<usize>,
    armed: bool,
    previewing: bool,
    preview_paused: bool,
    normalization_status: String,
    status_updated_at: Option<Instant>,
    test_tone_preview: Option<(std::path::PathBuf, String)>,
    test_tone_status: Option<String>,
    capacity_warning_started: Option<Instant>,
    prep_requested: bool,
    focus_changed_at: Instant,
    library_error: Option<String>,
    selected_panel: bool,
    queue_view: bool,
    queue_name: Option<String>,
}

pub struct ProfileEditor {
    pub profile: RuntimeProfile,
    field: usize,
}

pub struct QueueNameEditor {
    pub name: String,
    pub confirming_overwrite: bool,
    folder: std::path::PathBuf,
}

pub struct ProfileBrowser {
    pub paths: Vec<std::path::PathBuf>,
    cursor: usize,
    viewport_start: usize,
}

pub struct TrackFolderBrowser {
    folder: std::path::PathBuf,
    directories: Vec<std::path::PathBuf>,
    cursor: usize,
    viewport_start: usize,
    error: Option<String>,
}

impl TrackFolderBrowser {
    pub fn new(path: &str) -> Result<Self, String> {
        let mut folder = std::env::current_dir()
            .map_err(|error| error.to_string())?
            .join(path);
        while !folder.is_dir() {
            if !folder.pop() {
                return Err("NO ACCESSIBLE START FOLDER".to_owned());
            }
        }
        let mut browser = Self {
            folder: folder.clone(),
            directories: Vec::new(),
            cursor: 0,
            viewport_start: 0,
            error: None,
        };
        browser.open(folder);
        Ok(browser)
    }

    fn open(&mut self, folder: std::path::PathBuf) {
        let result = (|| {
            let mut directories = Vec::new();
            for entry in std::fs::read_dir(&folder)? {
                let entry = entry?;
                if entry.path().is_dir() {
                    directories.push(entry.path());
                }
            }
            directories.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
            Ok::<_, std::io::Error>(directories)
        })();
        match result {
            Ok(directories) => {
                self.folder = folder;
                self.directories = directories;
                self.cursor = 0;
                self.viewport_start = 0;
                self.error = None;
            }
            Err(error) => self.error = Some(error.to_string()),
        }
    }

    pub fn selected_folder(&self) -> &std::path::Path {
        &self.folder
    }

    pub fn handle_key(&mut self, key: Keycode) -> Option<bool> {
        match key {
            Keycode::Escape | Keycode::Q => return Some(false),
            Keycode::Space if self.error.is_none() => return Some(true),
            Keycode::Return | Keycode::Right => {
                if let Some(folder) = self.directories.get(self.cursor).cloned() {
                    self.open(folder);
                }
            }
            Keycode::Backspace | Keycode::Left => {
                if let Some(parent) = self.folder.parent() {
                    self.open(parent.to_path_buf());
                }
            }
            Keycode::Up | Keycode::K => self.cursor = self.cursor.saturating_sub(1),
            Keycode::Down | Keycode::J => {
                self.cursor = (self.cursor + 1).min(self.directories.len().saturating_sub(1));
            }
            Keycode::PageUp => self.cursor = self.cursor.saturating_sub(7),
            Keycode::PageDown => {
                self.cursor = (self.cursor + 7).min(self.directories.len().saturating_sub(1));
            }
            Keycode::Home => self.cursor = 0,
            Keycode::End => self.cursor = self.directories.len().saturating_sub(1),
            _ => {}
        }
        if self.cursor < self.viewport_start {
            self.viewport_start = self.cursor;
        }
        if self.cursor >= self.viewport_start + 7 {
            self.viewport_start = self.cursor + 1 - 7;
        }
        None
    }

    pub fn render(&self, canvas: &mut Canvas<Window>) -> Result<(), String> {
        canvas.set_draw_color(BACKGROUND);
        canvas.clear();
        outline(canvas, Rect::new(6, 6, 308, 228), DIM)?;
        text(canvas, 14, 14, "TRACKS FOLDER", 2, PHOSPHOR)?;
        text(
            canvas,
            14,
            40,
            &auto_scroll_text(&self.folder.to_string_lossy(), 48),
            1,
            AMBER,
        )?;
        if self.directories.is_empty() {
            text(canvas, 14, 65, "NO SUBFOLDERS", 1, DIM)?;
        }
        for (row, folder) in self
            .directories
            .iter()
            .skip(self.viewport_start)
            .take(7)
            .enumerate()
        {
            let selected = self.viewport_start + row == self.cursor;
            let row_y = 62 + row as i32 * 17;
            if selected {
                fill(canvas, Rect::new(12, row_y - 3, 296, 14), PANEL)?;
            }
            let name = format!(
                "[DIR] {}",
                folder.file_name().unwrap_or_default().to_string_lossy()
            );
            text(
                canvas,
                14,
                row_y,
                &auto_scroll_text(&name, 48),
                1,
                if selected { PHOSPHOR } else { CREAM },
            )?;
        }
        if let Some(error) = &self.error {
            text(canvas, 14, 186, &auto_scroll_text(error, 48), 1, RED)?;
        }
        text(canvas, 14, 202, "ENTER OPEN  BKSP UP  ESC CANCEL", 1, DIM)?;
        text(canvas, 14, 218, "SPACE USE CURRENT FOLDER", 1, AMBER)?;
        Ok(())
    }
}

impl ProfileBrowser {
    pub fn new(paths: Vec<std::path::PathBuf>) -> Self {
        Self {
            paths,
            cursor: 0,
            viewport_start: 0,
        }
    }

    pub fn handle_key(&mut self, key: Keycode) -> Option<bool> {
        match key {
            Keycode::Up | Keycode::K => self.cursor = self.cursor.saturating_sub(1),
            Keycode::Down | Keycode::J if !self.paths.is_empty() => {
                self.cursor = (self.cursor + 1).min(self.paths.len() - 1)
            }
            Keycode::PageUp => self.cursor = self.cursor.saturating_sub(7),
            Keycode::PageDown if !self.paths.is_empty() => {
                self.cursor = (self.cursor + 7).min(self.paths.len() - 1)
            }
            Keycode::Home => self.cursor = 0,
            Keycode::End if !self.paths.is_empty() => self.cursor = self.paths.len() - 1,
            Keycode::Return => return Some(true),
            Keycode::Escape | Keycode::Q => return Some(false),
            _ => {}
        }
        if self.cursor < self.viewport_start {
            self.viewport_start = self.cursor;
        }
        if self.cursor >= self.viewport_start + 7 {
            self.viewport_start = self.cursor + 1 - 7;
        }
        None
    }

    pub fn selected_path(&self) -> Option<&std::path::Path> {
        self.paths.get(self.cursor).map(std::path::PathBuf::as_path)
    }

    pub fn render(&self, canvas: &mut Canvas<Window>) -> Result<(), String> {
        canvas.set_draw_color(BACKGROUND);
        canvas.clear();
        outline(canvas, Rect::new(18, 20, 284, 200), AMBER)?;
        fill(canvas, Rect::new(20, 22, 280, 196), PANEL)?;
        text(canvas, 32, 32, "LOAD PROFILE", 2, PHOSPHOR)?;
        if self.paths.is_empty() {
            text(canvas, 38, 74, "NO SAVED PROFILES", 1, DIM)?;
        }
        for (row, path) in self
            .paths
            .iter()
            .skip(self.viewport_start)
            .take(7)
            .enumerate()
        {
            let index = self.viewport_start + row;
            let y = 60 + row as i32 * 17;
            if index == self.cursor {
                fill(
                    canvas,
                    Rect::new(28, y - 3, 264, 14),
                    Color::RGB(42, 48, 43),
                )?;
            }
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("PROFILE.JSON");
            text(
                canvas,
                34,
                y,
                &auto_scroll_text(name, 35),
                1,
                if index == self.cursor {
                    PHOSPHOR
                } else {
                    CREAM
                },
            )?;
        }
        text(canvas, 32, 190, "ENTER LOAD   ESC CANCEL", 1, DIM)?;
        Ok(())
    }
}

pub struct QueueBrowser {
    pub paths: Vec<std::path::PathBuf>,
    cursor: usize,
    viewport_start: usize,
}

impl QueueBrowser {
    pub fn new(paths: Vec<std::path::PathBuf>) -> Self {
        Self {
            paths,
            cursor: 0,
            viewport_start: 0,
        }
    }

    pub fn handle_key(&mut self, key: Keycode) -> Option<bool> {
        match key {
            Keycode::Up | Keycode::K => self.cursor = self.cursor.saturating_sub(1),
            Keycode::Down | Keycode::J if !self.paths.is_empty() => {
                self.cursor = (self.cursor + 1).min(self.paths.len() - 1)
            }
            Keycode::PageUp => self.cursor = self.cursor.saturating_sub(7),
            Keycode::PageDown if !self.paths.is_empty() => {
                self.cursor = (self.cursor + 7).min(self.paths.len() - 1)
            }
            Keycode::Home => self.cursor = 0,
            Keycode::End if !self.paths.is_empty() => self.cursor = self.paths.len() - 1,
            Keycode::Return => return Some(true),
            Keycode::Escape | Keycode::Q => return Some(false),
            _ => {}
        }
        if self.cursor < self.viewport_start {
            self.viewport_start = self.cursor;
        }
        if self.cursor >= self.viewport_start + 7 {
            self.viewport_start = self.cursor + 1 - 7;
        }
        None
    }

    pub fn selected_path(&self) -> Option<&std::path::Path> {
        self.paths.get(self.cursor).map(std::path::PathBuf::as_path)
    }

    pub fn render(&self, canvas: &mut Canvas<Window>) -> Result<(), String> {
        canvas.set_draw_color(BACKGROUND);
        canvas.clear();
        outline(canvas, Rect::new(18, 20, 284, 200), AMBER)?;
        fill(canvas, Rect::new(20, 22, 280, 196), PANEL)?;
        text(canvas, 32, 32, "LOAD SAVED QUEUE", 2, PHOSPHOR)?;
        if self.paths.is_empty() {
            text(canvas, 38, 74, "NO SAVED QUEUES", 1, DIM)?;
        }
        for (row, path) in self
            .paths
            .iter()
            .skip(self.viewport_start)
            .take(7)
            .enumerate()
        {
            let index = self.viewport_start + row;
            let y = 60 + row as i32 * 17;
            if index == self.cursor {
                fill(
                    canvas,
                    Rect::new(28, y - 3, 264, 14),
                    Color::RGB(42, 48, 43),
                )?;
            }
            let name = path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or("QUEUE");
            text(
                canvas,
                34,
                y,
                &auto_scroll_text(name, 35),
                1,
                if index == self.cursor {
                    PHOSPHOR
                } else {
                    CREAM
                },
            )?;
        }
        text(canvas, 32, 190, "ENTER LOAD   ESC CANCEL", 1, DIM)?;
        Ok(())
    }
}

impl QueueNameEditor {
    pub fn new(folder: impl Into<std::path::PathBuf>) -> Self {
        Self {
            name: String::new(),
            confirming_overwrite: false,
            folder: folder.into(),
        }
    }

    pub fn handle_text_input(&mut self, value: &str) {
        if self.confirming_overwrite {
            self.confirming_overwrite = false;
        }
        self.name.push_str(value);
    }

    pub fn handle_key(&mut self, key: Keycode) -> Option<bool> {
        if self.confirming_overwrite {
            match key {
                Keycode::Return => Some(true),
                Keycode::V => {
                    self.name = next_queue_version(&self.folder, &self.name);
                    Some(true)
                }
                Keycode::Escape => {
                    self.confirming_overwrite = false;
                    None
                }
                Keycode::Backspace => {
                    self.confirming_overwrite = false;
                    self.name.pop();
                    None
                }
                _ => None,
            }
        } else {
            match key {
                Keycode::Backspace => {
                    self.name.pop();
                    None
                }
                Keycode::Return => {
                    if self.name.trim().is_empty() {
                        return None;
                    }
                    if queue_exists(&self.folder, &self.name) {
                        self.confirming_overwrite = true;
                        None
                    } else {
                        Some(true)
                    }
                }
                Keycode::Escape | Keycode::Q => Some(false),
                _ => None,
            }
        }
    }

    pub fn render(&self, canvas: &mut Canvas<Window>) -> Result<(), String> {
        canvas.set_draw_color(BACKGROUND);
        canvas.clear();
        outline(canvas, Rect::new(20, 68, 280, 102), AMBER)?;
        fill(canvas, Rect::new(22, 70, 276, 98), PANEL)?;
        if self.confirming_overwrite {
            text(canvas, 36, 78, "FILE ALREADY EXISTS!", 1, RED)?;
            text(
                canvas,
                36,
                94,
                &auto_scroll_text(&format!("{}.json", self.name), 38),
                1,
                PHOSPHOR,
            )?;
            text(canvas, 36, 114, "OVERWRITE OR CREATE VERSION?", 1, CREAM)?;
            text(
                canvas,
                36,
                142,
                "ENT OVERWRITE  V VERSION  ESC NO",
                1,
                AMBER,
            )?;
        } else {
            text(canvas, 38, 80, "SAVE QUEUE", 2, PHOSPHOR)?;
            text(canvas, 38, 104, "NAME", 1, CREAM)?;
            text(
                canvas,
                38,
                120,
                &auto_scroll_text(&format!("{}_", self.name), 42),
                1,
                AMBER,
            )?;
            text(canvas, 38, 144, "ENTER SAVE   ESC CANCEL", 1, DIM)?;
        }
        Ok(())
    }
}

pub struct NormalizationReview {
    pub tracks: Vec<(String, u32, bool)>,
    pub total_seconds: u32,
    pub capacity_seconds: u32,
    pub preview_status: String,
    pub cursor: usize,
    pub viewport_start: usize,
    pub focus_changed_at: Instant,
}

pub struct NormalizationProgress {
    pub total: usize,
    pub completed: usize,
    pub current_index: usize,
    pub current_track: String,
    pub cache_hits: usize,
    pub method: NormalizationMethod,
    pub cancelling: bool,
    pub started_at: Instant,
}

impl NormalizationProgress {
    pub fn render(&self, canvas: &mut Canvas<Window>) -> Result<(), String> {
        canvas.set_draw_color(BACKGROUND);
        canvas.clear();
        outline(canvas, Rect::new(6, 6, 308, 228), DIM)?;
        text(canvas, 14, 14, "PREPARING TRACKS", 2, PHOSPHOR)?;
        let stage = if self.cancelling {
            "CANCELLING AFTER CURRENT TRACK"
        } else {
            match self.method {
                NormalizationMethod::None => "USING ORIGINAL FILES",
                NormalizationMethod::Peak => "CACHE / PEAK ANALYSIS / NORMALIZE",
                NormalizationMethod::Lufs => "CACHE / LUFS NORMALIZATION",
            }
        };
        text(canvas, 14, 48, stage, 1, AMBER)?;
        text(
            canvas,
            14,
            72,
            &format!("TRACK {} OF {}", self.current_index + 1, self.total),
            1,
            CREAM,
        )?;
        text(
            canvas,
            14,
            92,
            &auto_scroll_text(&self.current_track, 48),
            1,
            PHOSPHOR,
        )?;
        outline(canvas, Rect::new(14, 118, 292, 14), DIM)?;
        let width = (288 * self.completed / self.total.max(1)) as u32;
        if width > 0 {
            fill(canvas, Rect::new(16, 120, width, 10), PHOSPHOR)?;
        }
        text(
            canvas,
            14,
            146,
            &format!(
                "READY {}/{}  CACHE {}",
                self.completed, self.total, self.cache_hits
            ),
            1,
            CREAM,
        )?;
        let pulse = if (self.started_at.elapsed().as_millis() / 350) % 2 == 0 {
            "WORKING"
        } else {
            "WORKING ."
        };
        text(canvas, 14, 170, pulse, 1, DIM)?;
        text(canvas, 14, 215, "ESC CANCEL", 1, RED)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegendContext {
    Main,
    Review,
    ProfileEditor,
}

pub fn render_legend(canvas: &mut Canvas<Window>, context: LegendContext) -> Result<(), String> {
    canvas.set_blend_mode(BlendMode::Blend);
    fill(canvas, Rect::new(0, 0, 320, 240), Color::RGBA(0, 0, 0, 190))?;
    outline(canvas, Rect::new(24, 25, 272, 190), AMBER)?;
    fill(canvas, Rect::new(26, 27, 268, 186), PANEL)?;
    text(canvas, 40, 36, "CONTROL LEGEND", 2, PHOSPHOR)?;
    let lines: &[(&str, &str)] = match context {
        LegendContext::Main => &[
            ("UP/DOWN", "MOVE TRACK"),
            ("SPACE", "SELECT"),
            ("C", "CLEAR SELECTION"),
            ("1-5", "CALIBRATION TONES"),
            ("P", "PREVIEW"),
            ("X", "STOP"),
            ("N", "NORMALIZE"),
            ("CTRL-S/O", "QUEUE SAVE/BROWSE"),
            ("L", "LOAD PROFILE"),
            ("ENTER", "PREPARE"),
            ("TAB / V", "SELECTED QUEUE"),
            ("W / B", "EDITOR / FOLDER"),
        ],
        LegendContext::Review => &[
            ("UP/DOWN", "MOVE TRACK"),
            ("PGUP/PGDN", "JUMP"),
            ("1-5", "CALIBRATION TONES"),
            ("P", "PREVIEW"),
            ("X", "STOP"),
            ("H/L", "SEEK 10 SEC"),
            ("ENTER", "COUNTDOWN"),
            ("ESC", "BACK"),
        ],
        LegendContext::ProfileEditor => &[
            ("UP/DOWN", "NAVIGATE FIELDS"),
            ("[TXT] FIELDS", "TYPE TEXT / BKSP"),
            ("[OPT] FIELDS", "LEFT/RIGHT CHANGE"),
            ("F2", "BROWSE TRACK FOLDER"),
            ("CTRL-S", "SAVE PROFILE"),
            ("ESC", "CLOSE EDITOR"),
        ],
    };
    let spacing = if lines.len() > 10 {
        11
    } else if lines.len() > 7 {
        13
    } else {
        17
    };
    let start_y = if lines.len() > 10 {
        52
    } else if lines.len() > 7 {
        56
    } else {
        62
    };
    for (index, (key, action)) in lines.iter().enumerate() {
        let y = start_y + index as i32 * spacing;
        text(canvas, 42, y, key, 1, AMBER)?;
        text(canvas, 142, y, action, 1, CREAM)?;
    }
    text(canvas, 42, 195, "? / ESC CLOSE", 1, RED)?;
    Ok(())
}

impl NormalizationReview {
    pub fn handle_key(&mut self, key: Keycode) {
        if self.tracks.is_empty() {
            return;
        }
        let previous_cursor = self.cursor;
        match key {
            Keycode::Up | Keycode::K => self.cursor = self.cursor.saturating_sub(1),
            Keycode::Down | Keycode::J => {
                self.cursor = (self.cursor + 1).min(self.tracks.len() - 1)
            }
            Keycode::PageUp => self.cursor = self.cursor.saturating_sub(7),
            Keycode::PageDown => self.cursor = (self.cursor + 7).min(self.tracks.len() - 1),
            Keycode::Home => self.cursor = 0,
            Keycode::End => self.cursor = self.tracks.len() - 1,
            _ => {}
        }
        if self.cursor != previous_cursor {
            self.focus_changed_at = Instant::now();
            if self.cursor < self.viewport_start {
                self.viewport_start = self.cursor;
            } else if self.cursor >= self.viewport_start + 7 {
                self.viewport_start = self.cursor + 1 - 7;
            }
        }
    }

    pub fn current_index(&self) -> usize {
        self.cursor
    }

    pub fn render(
        &self,
        canvas: &mut Canvas<Window>,
        profile: &RuntimeProfile,
        meter_levels: Option<(usize, usize)>,
        test_tone_status: Option<&str>,
    ) -> Result<(), String> {
        canvas.set_draw_color(BACKGROUND);
        canvas.clear();
        outline(canvas, Rect::new(6, 6, 308, 228), DIM)?;
        fill(canvas, Rect::new(9, 9, 302, 25), PANEL)?;
        text(
            canvas,
            14,
            13,
            if profile.normalization == NormalizationMethod::None {
                "ORIGINAL TRACKS READY"
            } else {
                "NORMALIZATION COMPLETE"
            },
            2,
            PHOSPHOR,
        )?;
        if let Some(status) = test_tone_status {
            text(canvas, 14, 43, &auto_scroll_text(status, 48), 1, AMBER)?;
        } else {
            text(canvas, 14, 43, "REVIEW TRACKS BEFORE COUNTDOWN", 1, CREAM)?;
            text(
                canvas,
                210,
                43,
                &format!("{} TRACKS", self.tracks.len()),
                1,
                AMBER,
            )?;
        }

        let visible_end = (self.viewport_start + 7).min(self.tracks.len());
        for (row, (name, duration, cache_hit)) in self.tracks[self.viewport_start..visible_end]
            .iter()
            .enumerate()
        {
            let index = self.viewport_start + row;
            let status = if profile.normalization == NormalizationMethod::None {
                "ORIG"
            } else if *cache_hit {
                "CACHE"
            } else {
                "NEW"
            };
            let color = if *cache_hit { DIM } else { PHOSPHOR };
            if index == self.cursor {
                fill(
                    canvas,
                    Rect::new(11, 57 + row as i32 * 17, 298, 14),
                    Color::RGB(42, 48, 43),
                )?;
            }
            let display_name = track_name_text(
                name,
                22,
                index == self.cursor,
                self.focus_changed_at.elapsed(),
            );
            text(
                canvas,
                14,
                60 + row as i32 * 17,
                &format!("{:02} {}", index + 1, display_name),
                1,
                CREAM,
            )?;
            text(
                canvas,
                173,
                60 + row as i32 * 17,
                &format!("{:02}:{:02}", duration / 60, duration % 60),
                1,
                CREAM,
            )?;
            text(canvas, 230, 60 + row as i32 * 17, status, 1, color)?;
        }
        draw_scrollbar(canvas, self.tracks.len(), self.viewport_start)?;

        fill(canvas, Rect::new(9, 181, 302, 26), PANEL)?;
        text(
            canvas,
            14,
            185,
            &format!(
                "TOTAL {:02}:{:02}",
                self.total_seconds / 60,
                self.total_seconds % 60
            ),
            1,
            CREAM,
        )?;
        text(
            canvas,
            130,
            185,
            &format!(
                "SIDE {:02}:{:02}",
                self.capacity_seconds / 60,
                self.capacity_seconds % 60
            ),
            1,
            DIM,
        )?;
        let remaining = self.capacity_seconds as i64 - self.total_seconds as i64;
        let capacity_color = if remaining >= 0 { PHOSPHOR } else { RED };
        text(
            canvas,
            14,
            195,
            &format!("REMAINING {:+}S", remaining),
            1,
            capacity_color,
        )?;
        text(canvas, 203, 183, "L", 1, DIM)?;
        meter(canvas, 212, 182, meter_levels.map_or(0, |levels| levels.0))?;
        text(canvas, 203, 193, "R", 1, DIM)?;
        meter(canvas, 212, 192, meter_levels.map_or(0, |levels| levels.1))?;
        text(canvas, 212, 202, "-40  -20  -6   0", 1, DIM)?;
        text(canvas, 14, 215, "P PREVIEW", 1, AMBER)?;
        text(canvas, 104, 215, "X STOP", 1, RED)?;
        text(canvas, 160, 215, "ENTER NEXT", 1, AMBER)?;
        if !self.preview_status.is_empty() {
            text(
                canvas,
                235,
                215,
                &auto_scroll_text(&self.preview_status, 12),
                1,
                PHOSPHOR,
            )?;
        }
        let normalization_label = if profile.normalization == NormalizationMethod::None {
            "NORMALIZATION NONE / ORIGINAL FILES".to_owned()
        } else {
            format!(
                "NORMALIZATION {:?}  TARGET {:+.1}",
                profile.normalization, profile.target_lufs
            )
        };
        text(canvas, 14, 224, &normalization_label, 1, DIM)?;
        text(canvas, 274, 224, "? HELP", 1, AMBER)?;
        Ok(())
    }
}

impl ProfileEditor {
    pub fn new(profile: &RuntimeProfile) -> Self {
        Self {
            profile: profile.clone(),
            field: 0,
        }
    }

    pub fn is_text_field(&self) -> bool {
        self.field < 5
    }

    pub fn handle_key(&mut self, key: Keycode) -> bool {
        if self.is_text_field() {
            match key {
                Keycode::Up => self.field = self.field.saturating_sub(1),
                Keycode::Down => self.field = (self.field + 1).min(12),
                Keycode::Backspace => self.handle_backspace(),
                Keycode::Escape => return true,
                _ => {}
            }
        } else {
            match key {
                Keycode::Up | Keycode::K => self.field = self.field.saturating_sub(1),
                Keycode::Down | Keycode::J => self.field = (self.field + 1).min(12),
                Keycode::Left | Keycode::H => self.change_value(-1),
                Keycode::Right | Keycode::L => self.change_value(1),
                Keycode::Backspace => self.handle_backspace(),
                Keycode::Escape | Keycode::Q => return true,
                _ => {}
            }
        }
        false
    }

    fn change_value(&mut self, direction: i32) {
        match self.field {
            5 => {
                self.profile.side_duration_minutes =
                    step_u32(self.profile.side_duration_minutes, direction, 5, 120)
            }
            6 => {
                self.profile.leader_gap_seconds =
                    step_u32(self.profile.leader_gap_seconds, direction, 0, 60)
            }
            7 => {
                self.profile.track_gap_seconds =
                    step_u32(self.profile.track_gap_seconds, direction, 0, 30)
            }
            8 => {
                self.profile.target_lufs =
                    (self.profile.target_lufs + f64::from(direction)).clamp(-30.0, -1.0)
            }
            9 => {
                self.profile.counter_rate =
                    (self.profile.counter_rate + f64::from(direction) * 0.1).max(0.1)
            }
            10 => {
                self.profile.normalization =
                    cycle_normalization(self.profile.normalization, direction)
            }
            11 => self.profile.counter_mode = cycle_counter(self.profile.counter_mode, direction),
            12 => self.profile.fullscreen = !self.profile.fullscreen,
            _ => {}
        }
    }

    pub fn handle_text_input(&mut self, value: &str) {
        let target = match self.field {
            0 => &mut self.profile.name,
            1 => &mut self.profile.tracks_folder,
            2 => &mut self.profile.normalized_folder,
            3 => &mut self.profile.audio_device,
            4 => self.profile.counter_config.get_or_insert_with(String::new),
            _ => return,
        };
        target.push_str(value);
    }

    pub fn handle_backspace(&mut self) {
        let target = match self.field {
            0 => &mut self.profile.name,
            1 => &mut self.profile.tracks_folder,
            2 => &mut self.profile.normalized_folder,
            3 => &mut self.profile.audio_device,
            4 => self.profile.counter_config.get_or_insert_with(String::new),
            _ => return,
        };
        target.pop();
    }

    pub fn render(&self, canvas: &mut Canvas<Window>) -> Result<(), String> {
        canvas.set_draw_color(BACKGROUND);
        canvas.clear();
        outline(canvas, Rect::new(6, 6, 308, 228), DIM)?;
        fill(canvas, Rect::new(9, 9, 302, 25), PANEL)?;
        text(canvas, 14, 13, "PROFILE EDITOR", 2, PHOSPHOR)?;

        if self.field == 1 {
            text(canvas, 14, 43, "[TXT] TYPE PATH  F2 BROWSE", 1, AMBER)?;
        } else if self.is_text_field() {
            text(canvas, 14, 43, "[TXT] TYPE TO EDIT  BKSP ERASE", 1, AMBER)?;
        } else {
            text(
                canvas,
                14,
                43,
                "[OPT] LEFT/RIGHT SELECT OPTION",
                1,
                PHOSPHOR,
            )?;
        }

        let fields: [(&str, &str, String, bool); 13] = [
            ("[TXT]", "NAME", self.profile.name.clone(), true),
            (
                "[TXT]",
                "TRACKS FOLDER",
                self.profile.tracks_folder.clone(),
                true,
            ),
            (
                "[TXT]",
                "NORMALIZED DIR",
                self.profile.normalized_folder.clone(),
                true,
            ),
            (
                "[TXT]",
                "AUDIO DEVICE",
                self.profile.audio_device.clone(),
                true,
            ),
            (
                "[TXT]",
                "CALIBRATION",
                self.profile.counter_config.clone().unwrap_or_default(),
                true,
            ),
            (
                "[OPT]",
                "SIDE MINUTES",
                format!("< {:>3} >", self.profile.side_duration_minutes),
                false,
            ),
            (
                "[OPT]",
                "LEADER GAP",
                format!("< {:>3}S >", self.profile.leader_gap_seconds),
                false,
            ),
            (
                "[OPT]",
                "TRACK GAP",
                format!("< {:>3}S >", self.profile.track_gap_seconds),
                false,
            ),
            (
                "[OPT]",
                "TARGET LUFS",
                format!("< {:>+5.1} >", self.profile.target_lufs),
                false,
            ),
            (
                "[OPT]",
                "COUNTER RATE",
                format!("< {:>5.1} >", self.profile.counter_rate),
                false,
            ),
            (
                "[OPT]",
                "NORMALIZATION",
                format!("< {:?} >", self.profile.normalization),
                false,
            ),
            (
                "[OPT]",
                "COUNTER MODE",
                format!("< {:?} >", self.profile.counter_mode),
                false,
            ),
            (
                "[OPT]",
                "FULLSCREEN",
                format!(
                    "< {:^3} >",
                    if self.profile.fullscreen { "YES" } else { "NO" }
                ),
                false,
            ),
        ];

        let first_field = self.field.saturating_sub(5);
        for (row, (tag, label, value, is_text)) in
            fields.iter().skip(first_field).take(8).enumerate()
        {
            let index = first_field + row;
            let highlighted = index == self.field;
            let row_y = 65 + row as i32 * 17;

            if highlighted {
                fill(
                    canvas,
                    Rect::new(12, row_y - 4, 296, 15),
                    Color::RGB(42, 48, 43),
                )?;
            }

            let tag_color = if *is_text {
                if highlighted { AMBER } else { DIM }
            } else {
                if highlighted { PHOSPHOR } else { DIM }
            };
            text(canvas, 16, row_y, tag, 1, tag_color)?;

            let label_color = if highlighted { PHOSPHOR } else { CREAM };
            text(canvas, 52, row_y, label, 1, label_color)?;

            let val_color = if highlighted {
                if *is_text { CREAM } else { AMBER }
            } else {
                CREAM
            };

            let display_val = if *is_text && highlighted {
                auto_scroll_text(&format!("{value}_"), 26)
            } else if *is_text && value.is_empty() {
                "(none)".to_owned()
            } else {
                auto_scroll_text(value, 26)
            };

            text(canvas, 154, row_y, &display_val, 1, val_color)?;
        }

        fill(canvas, Rect::new(9, 211, 302, 20), PANEL)?;
        text(canvas, 14, 215, "CTRL-S SAVE", 1, AMBER)?;
        text(canvas, 96, 225, "F2 BROWSE TRACKS", 1, PHOSPHOR)?;
        text(canvas, 150, 215, "ESC CLOSE", 1, RED)?;
        text(canvas, 258, 215, "?", 1, AMBER)?;
        Ok(())
    }
}

fn step_u32(value: u32, direction: i32, minimum: u32, maximum: u32) -> u32 {
    value
        .saturating_add_signed(direction)
        .clamp(minimum, maximum)
}

fn cycle_normalization(value: NormalizationMethod, direction: i32) -> NormalizationMethod {
    let modes = [
        NormalizationMethod::Peak,
        NormalizationMethod::Lufs,
        NormalizationMethod::None,
    ];
    let index = modes.iter().position(|mode| *mode == value).unwrap_or(0) as i32;
    modes[(index + direction).rem_euclid(modes.len() as i32) as usize]
}

fn cycle_counter(value: CounterMode, direction: i32) -> CounterMode {
    let modes = [
        CounterMode::Linear,
        CounterMode::Static,
        CounterMode::Auto,
        CounterMode::Manual,
    ];
    let index = modes.iter().position(|mode| *mode == value).unwrap_or(0) as i32;
    modes[(index + direction).rem_euclid(modes.len() as i32) as usize]
}

impl AppUi {
    pub fn new(tracks: Vec<AudioTrack>, library_error: Option<String>) -> Self {
        let selected = vec![false; tracks.len()];
        Self {
            tracks,
            cursor: 0,
            viewport_start: 0,
            selected,
            selection_order: Vec::new(),
            armed: false,
            previewing: false,
            preview_paused: false,
            normalization_status: String::new(),
            status_updated_at: None,
            test_tone_preview: None,
            test_tone_status: None,
            capacity_warning_started: None,
            prep_requested: false,
            focus_changed_at: Instant::now(),
            library_error,
            selected_panel: false,
            queue_view: false,
            queue_name: None,
        }
    }

    pub fn handle_key(&mut self, key: Keycode, profile: &RuntimeProfile) {
        if self.tracks.is_empty() {
            return;
        }
        let previous_cursor = self.cursor;
        match key {
            Keycode::Up | Keycode::K => {
                self.cursor = self.cursor.saturating_sub(1);
            }
            Keycode::Down | Keycode::J => {
                self.cursor = (self.cursor + 1).min(self.selected.len() - 1);
            }
            Keycode::PageUp => self.cursor = self.cursor.saturating_sub(VISIBLE_TRACKS),
            Keycode::PageDown => {
                self.cursor = (self.cursor + VISIBLE_TRACKS).min(self.tracks.len() - 1);
            }
            Keycode::Home => self.cursor = 0,
            Keycode::End => self.cursor = self.tracks.len() - 1,
            Keycode::Space => {
                if self.selected[self.cursor] {
                    self.queue_name = None;
                    self.capacity_warning_started = None;
                    self.selected[self.cursor] = false;
                    self.selection_order.retain(|index| *index != self.cursor);
                    self.selected_panel = self.selected.iter().any(|is_selected| *is_selected);
                } else if let Some(track) = self.tracks.get(self.cursor) {
                    let mut candidate_tracks = self.selected_tracks();
                    candidate_tracks.push(track.clone());
                    let plan = SessionPlan::from_audio_tracks(profile, &candidate_tracks);
                    if plan.fits() {
                        self.queue_name = None;
                        self.capacity_warning_started = None;
                        self.selected[self.cursor] = true;
                        self.selection_order.push(self.cursor);
                        self.selected_panel = true;
                    } else {
                        self.capacity_warning_started = Some(Instant::now());
                    }
                }
            }
            Keycode::C => {
                self.clear_selection();
            }
            Keycode::Return => {
                self.armed = !self.armed;
                self.prep_requested = true;
            }
            Keycode::Tab | Keycode::V => self.queue_view = !self.queue_view,
            _ => {}
        }
        if self.cursor != previous_cursor {
            self.focus_changed_at = Instant::now();
            self.keep_cursor_visible();
        }
    }

    pub fn current_track(&self) -> Option<&AudioTrack> {
        self.tracks.get(self.cursor)
    }

    pub fn set_previewing(&mut self, previewing: bool, paused: bool) {
        if !previewing {
            self.test_tone_preview = None;
            self.test_tone_status = None;
        }
        self.previewing = previewing;
        self.preview_paused = paused;
    }

    pub fn set_test_tone_preview(&mut self, path: &std::path::Path, tone: TestTone) {
        self.set_track_preview(path, &format!("TEST TONE {}", tone.label()));
    }

    pub fn set_track_preview(&mut self, path: &std::path::Path, name: &str) {
        self.test_tone_preview = Some((path.to_path_buf(), name.to_owned()));
        self.sync_test_tone_preview(Some(path), false);
    }

    pub fn sync_test_tone_preview(&mut self, current_path: Option<&std::path::Path>, paused: bool) {
        if let Some((path, label)) = &self.test_tone_preview {
            if current_path == Some(path.as_path()) {
                self.test_tone_status = Some(format!(
                    "{} {}",
                    if paused { "PAUSED" } else { "PLAYING" },
                    label
                ));
            } else {
                self.test_tone_preview = None;
                self.test_tone_status = None;
            }
        }
    }

    pub fn test_tone_status(&self) -> Option<&str> {
        self.test_tone_status.as_deref()
    }

    pub fn set_selected_panel(&mut self, visible: bool) {
        self.selected_panel = visible;
    }

    pub fn queue_view(&self) -> bool {
        self.queue_view
    }

    pub fn previewing(&self) -> bool {
        self.previewing
    }

    pub fn selected_tracks(&self) -> Vec<AudioTrack> {
        self.selection_order
            .iter()
            .filter_map(|index| self.tracks.get(*index))
            .cloned()
            .collect()
    }

    pub fn reset_library(&mut self, tracks: Vec<AudioTrack>, error: Option<String>) {
        self.test_tone_preview = None;
        self.test_tone_status = None;
        self.queue_name = None;
        self.capacity_warning_started = None;
        self.tracks = tracks;
        self.cursor = 0;
        self.viewport_start = 0;
        self.selected = vec![false; self.tracks.len()];
        self.selection_order.clear();
        self.selected_panel = false;
        self.armed = false;
        self.previewing = false;
        self.preview_paused = false;
        self.library_error = error;
        self.focus_changed_at = Instant::now();
    }

    pub fn clear_selection(&mut self) {
        self.queue_name = None;
        self.capacity_warning_started = None;
        if self.selected.iter().any(|&is_selected| is_selected) {
            self.selected.fill(false);
            self.selection_order.clear();
            self.selected_panel = false;
            self.armed = false;
        }
    }

    pub fn load_selection(&mut self, saved_paths: &[std::path::PathBuf]) -> usize {
        self.queue_name = None;
        self.capacity_warning_started = None;
        let (selected, order, missing) = match_tracks_ordered(&self.tracks, saved_paths);
        self.selected = selected;
        self.selection_order = order;
        self.selected_panel = self.selected.iter().any(|is_selected| *is_selected);
        missing
    }

    pub fn set_queue_path(&mut self, path: &std::path::Path) {
        self.queue_name = path
            .file_stem()
            .map(|name| name.to_string_lossy().into_owned());
    }

    fn recording_session_label(&self, profile: &RuntimeProfile) -> String {
        format!(
            "[{}+{}]",
            profile.name,
            self.queue_name.as_deref().unwrap_or("Unsaved")
        )
    }

    pub fn set_normalization_status(&mut self, status: impl Into<String>) {
        self.normalization_status = status.into();
        self.status_updated_at = Some(Instant::now());
    }

    pub fn current_status(&self) -> Option<&str> {
        if let Some(status) = self.test_tone_status() {
            return Some(status);
        }
        if let Some(timestamp) = self.status_updated_at {
            if timestamp.elapsed() < STATUS_TIMEOUT && !self.normalization_status.is_empty() {
                return Some(&self.normalization_status);
            }
        }
        None
    }

    pub fn clear_normalization_status(&mut self) {
        self.normalization_status.clear();
        self.status_updated_at = None;
    }

    pub fn take_prep_request(&mut self) -> bool {
        std::mem::take(&mut self.prep_requested)
    }

    fn keep_cursor_visible(&mut self) {
        if self.cursor < self.viewport_start {
            self.viewport_start = self.cursor;
        } else if self.cursor >= self.viewport_start + VISIBLE_TRACKS {
            self.viewport_start = self.cursor + 1 - VISIBLE_TRACKS;
        }
    }

    pub fn render(
        &self,
        canvas: &mut Canvas<Window>,
        profile: &RuntimeProfile,
        meter_levels: Option<(usize, usize)>,
    ) -> Result<(), String> {
        let test_tone_active = self.test_tone_preview.is_some();
        if self.queue_view && !test_tone_active {
            return self.render_queue_workspace(canvas, profile);
        }
        canvas.set_draw_color(BACKGROUND);
        canvas.clear();

        outline(canvas, Rect::new(6, 6, 308, 228), DIM)?;
        fill(canvas, Rect::new(9, 9, 302, 25), PANEL)?;
        text(canvas, 14, 13, "DECKPREP", 2, PHOSPHOR)?;
        text(canvas, 121, 14, "TAPE RECORDING CONSOLE", 1, CREAM)?;
        text(canvas, 121, 24, "PROFILE:", 1, AMBER)?;
        text(
            canvas,
            169,
            24,
            &auto_scroll_text(&profile.name, 9),
            1,
            AMBER,
        )?;
        text(canvas, 227, 24, "DAC:", 1, DIM)?;
        text(
            canvas,
            251,
            24,
            &auto_scroll_text(&profile.audio_device, 9),
            1,
            DIM,
        )?;

        draw_tracks(
            canvas,
            &self.tracks,
            self.cursor,
            self.viewport_start,
            &self.selected,
            self.focus_changed_at.elapsed(),
            self.library_error.as_deref(),
            profile,
        )?;
        let capacity_warning =
            capacity_warning_phase(self.capacity_warning_started.map(|start| start.elapsed()));
        if !test_tone_active && (self.selected_panel || capacity_warning.is_some()) {
            draw_selected_panel(
                canvas,
                &self.tracks,
                &self.selection_order,
                self.cursor,
                self.focus_changed_at.elapsed(),
                profile,
                capacity_warning,
            )?;
        } else {
            draw_deck_monitor(
                canvas,
                self.armed,
                self.previewing,
                self.preview_paused,
                meter_levels,
            )?;
        }

        fill(canvas, Rect::new(9, 211, 302, 20), PANEL)?;
        tiny_text(canvas, 18, 218, "NAV ^V", Color::RGB(72, 202, 220))?;
        tiny_text(canvas, 60, 218, "SEL SPC", PHOSPHOR)?;
        tiny_text(canvas, 108, 218, "PLAY P", AMBER)?;
        tiny_text(canvas, 151, 218, "STOP X", RED)?;
        tiny_text(canvas, 195, 218, "QUEUE TAB", Color::RGB(92, 148, 224))?;
        tiny_text(canvas, 249, 218, "PREP ENT", Color::RGB(220, 104, 190))?;
        tiny_text(canvas, 290, 218, "?", Color::RGB(246, 208, 82))?;
        if let Some(status) = self.current_status() {
            text(canvas, 14, 205, &auto_scroll_text(status, 48), 1, AMBER)?;
        }

        Ok(())
    }

    fn render_queue_workspace(
        &self,
        canvas: &mut Canvas<Window>,
        profile: &RuntimeProfile,
    ) -> Result<(), String> {
        canvas.set_draw_color(BACKGROUND);
        canvas.clear();
        outline(canvas, Rect::new(6, 6, 308, 228), DIM)?;
        fill(canvas, Rect::new(9, 9, 302, 25), PANEL)?;
        text(canvas, 14, 13, "TRACK / QUEUE WORKSPACE", 2, PHOSPHOR)?;
        text(canvas, 14, 43, "TRACK LIBRARY", 1, AMBER)?;
        text(canvas, 169, 43, "SELECTED QUEUE", 1, AMBER)?;
        fill(canvas, Rect::new(9, 51, 148, 151), PANEL)?;
        fill(canvas, Rect::new(164, 51, 147, 151), PANEL)?;

        let visible_end = (self.viewport_start + 7).min(self.tracks.len());
        for (row, track) in self.tracks[self.viewport_start..visible_end]
            .iter()
            .enumerate()
        {
            let index = self.viewport_start + row;
            let y = 60 + row as i32 * 17;
            let highlighted = index == self.cursor;
            if highlighted {
                fill(
                    canvas,
                    Rect::new(11, y - 3, 144, 14),
                    Color::RGB(42, 48, 43),
                )?;
            }
            let color = if highlighted { PHOSPHOR } else { CREAM };
            text(
                canvas,
                14,
                y,
                if self.selected[index] { "[X]" } else { "[ ]" },
                1,
                color,
            )?;
            text(canvas, 34, y, &format!("{:02}", index + 1), 1, color)?;
            text(
                canvas,
                50,
                y,
                &track_name_text(
                    &track.name,
                    10,
                    highlighted,
                    self.focus_changed_at.elapsed(),
                ),
                1,
                color,
            )?;
            text(
                canvas,
                116,
                y,
                &format!(
                    "{:02}:{:02}",
                    track.duration_seconds / 60,
                    track.duration_seconds % 60
                ),
                1,
                DIM,
            )?;
        }
        draw_scrollbar_at(
            canvas,
            151,
            58,
            135,
            2,
            self.tracks.len(),
            self.viewport_start,
            7,
        )?;

        let selected_tracks: Vec<(usize, &AudioTrack)> = self
            .selection_order
            .iter()
            .filter_map(|source_index| {
                self.tracks
                    .get(*source_index)
                    .map(|track| (*source_index, track))
            })
            .collect();
        for (row, (source_index, track)) in selected_tracks.iter().take(7).enumerate() {
            let y = 60 + row as i32 * 17;
            let highlighted = *source_index == self.cursor;
            if highlighted {
                fill(
                    canvas,
                    Rect::new(166, y - 3, 142, 14),
                    Color::RGB(42, 48, 43),
                )?;
            }
            let color = if highlighted { PHOSPHOR } else { CREAM };
            text(canvas, 169, y, &format!("{:02}", row + 1), 1, color)?;
            text(
                canvas,
                185,
                y,
                &track_name_text(
                    &track.name,
                    10,
                    highlighted,
                    self.focus_changed_at.elapsed(),
                ),
                1,
                color,
            )?;
            text(
                canvas,
                251,
                y,
                &format!(
                    "{:02}:{:02}",
                    track.duration_seconds / 60,
                    track.duration_seconds % 60
                ),
                1,
                DIM,
            )?;
        }

        let selected_audio: Vec<AudioTrack> = selected_tracks
            .iter()
            .map(|(_, track)| (*track).clone())
            .collect();
        let plan = SessionPlan::from_audio_tracks(profile, &selected_audio);
        let total_seconds = plan.total_seconds();
        let total_color = if plan.fits() { PHOSPHOR } else { RED };
        text(
            canvas,
            169,
            180,
            &format!("TOTAL {:02}:{:02}", total_seconds / 60, total_seconds % 60),
            1,
            total_color,
        )?;
        draw_capacity_limit(
            canvas,
            169,
            190,
            profile,
            capacity_warning_phase(self.capacity_warning_started.map(|start| start.elapsed())),
        )?;
        if let Some(status) = self.current_status() {
            text(canvas, 14, 197, &auto_scroll_text(status, 48), 1, AMBER)?;
        }
        fill(canvas, Rect::new(9, 207, 302, 24), PANEL)?;
        tiny_text(canvas, 16, 216, "TAB CLOSE", AMBER)?;
        tiny_text(canvas, 82, 216, "SPACE SELECT", CREAM)?;
        tiny_text(canvas, 153, 216, "CTRL-S SAVE", PHOSPHOR)?;
        tiny_text(canvas, 232, 216, "CTRL-O LOAD", AMBER)?;
        Ok(())
    }

    pub fn render_recording(
        &self,
        canvas: &mut Canvas<Window>,
        profile: &RuntimeProfile,
        phase: &str,
        track_names: &[String],
        current_track: Option<usize>,
        elapsed_seconds: u32,
        total_seconds: u32,
        counter: u32,
        meter_levels: (usize, usize),
        marquee_elapsed: Duration,
        counter_running: bool,
    ) -> Result<(), String> {
        canvas.set_draw_color(BACKGROUND);
        canvas.clear();
        outline(canvas, Rect::new(6, 6, 308, 228), DIM)?;
        fill(canvas, Rect::new(9, 9, 302, 25), PANEL)?;
        text(canvas, 14, 13, "DECKPREP", 2, PHOSPHOR)?;
        text(canvas, 121, 14, "RECORDING MODE", 1, CREAM)?;
        text(canvas, 121, 24, "SESSION:", 1, AMBER)?;
        text(
            canvas,
            175,
            24,
            &auto_scroll_text(&self.recording_session_label(profile), 22),
            1,
            AMBER,
        )?;
        text(canvas, 211, 14, "DAC:", 1, DIM)?;
        text(
            canvas,
            241,
            14,
            &auto_scroll_text(&profile.audio_device, 11),
            1,
            DIM,
        )?;

        text(canvas, 14, 43, &auto_scroll_text(phase, 48), 1, AMBER)?;
        let total = total_seconds.max(1);
        let progress = elapsed_seconds.min(total);
        let remaining = total.saturating_sub(progress);

        text(canvas, 14, 57, "TAPE COUNTER", 1, CREAM)?;
        draw_counter(canvas, 22, 70, counter)?;
        draw_tape_reels(
            canvas,
            184,
            55,
            marquee_elapsed,
            counter_running,
            progress,
            total,
        )?;

        text(
            canvas,
            14,
            112,
            &format!(
                "TIME {:02}:{:02} / {:02}:{:02}",
                progress / 60,
                progress % 60,
                total / 60,
                total % 60
            ),
            1,
            CREAM,
        )?;
        text(
            canvas,
            205,
            112,
            &format!("LEFT {:02}:{:02}", remaining / 60, remaining % 60),
            1,
            AMBER,
        )?;
        progress_bar(canvas, 14, 124, progress, total, 292)?;

        let current_label = current_track
            .and_then(|index| track_names.get(index))
            .map(|name| scrolling_text(name, 45, marquee_elapsed))
            .unwrap_or_else(|| "STANDBY".to_owned());
        text(canvas, 14, 137, "NOW PLAYING", 1, AMBER)?;
        text(canvas, 14, 148, &current_label, 1, PHOSPHOR)?;

        fill(canvas, Rect::new(9, 163, 302, 68), PANEL)?;
        text(canvas, 14, 166, "SESSION TRACKS", 1, AMBER)?;
        text(canvas, 194, 166, "OUTPUT", 1, AMBER)?;
        let queue_start = current_track
            .unwrap_or(0)
            .saturating_sub(2)
            .min(track_names.len().saturating_sub(6));
        for (row, (index, name)) in track_names
            .iter()
            .enumerate()
            .skip(queue_start)
            .take(6)
            .enumerate()
        {
            let color = if current_track == Some(index) {
                PHOSPHOR
            } else {
                CREAM
            };
            let display_name =
                track_name_text(name, 23, current_track == Some(index), marquee_elapsed);
            if current_track == Some(index) {
                fill(
                    canvas,
                    Rect::new(12, 174 + row as i32 * 9, 164, 9),
                    Color::RGB(42, 48, 43),
                )?;
            }
            text(
                canvas,
                16,
                175 + row as i32 * 9,
                &format!("{:02} {}", index + 1, display_name),
                1,
                color,
            )?;
        }
        text(canvas, 194, 175, "L", 1, DIM)?;
        meter(canvas, 205, 174, meter_levels.0)?;
        text(canvas, 194, 187, "R", 1, DIM)?;
        meter(canvas, 205, 186, meter_levels.1)?;
        text(canvas, 205, 197, "-40  -20  -6   0", 1, DIM)?;
        Ok(())
    }
}

fn tiny_text(
    canvas: &mut Canvas<Window>,
    x: i32,
    y: i32,
    value: &str,
    color: Color,
) -> Result<(), String> {
    let mut cursor = x;
    for character in value.to_ascii_uppercase().chars() {
        for (row, bits) in tiny_glyph(character).iter().enumerate() {
            for column in 0..3 {
                if bits & (1 << (2 - column)) != 0 {
                    fill(
                        canvas,
                        Rect::new(cursor + column, y + row as i32, 1, 1),
                        color,
                    )?;
                }
            }
        }
        cursor += 4;
    }
    Ok(())
}

fn tiny_glyph(character: char) -> [u8; 5] {
    match character {
        'A' => [2, 5, 7, 5, 5],
        'B' => [6, 5, 6, 5, 6],
        'C' => [3, 4, 4, 4, 3],
        'E' => [7, 4, 6, 4, 7],
        'H' => [5, 5, 7, 5, 5],
        'L' => [4, 4, 4, 4, 7],
        'M' => [5, 7, 7, 5, 5],
        'N' => [5, 7, 7, 7, 5],
        'O' => [2, 5, 5, 5, 2],
        'P' => [6, 5, 6, 4, 4],
        'Q' => [2, 5, 5, 3, 1],
        'R' => [6, 5, 6, 5, 5],
        'S' => [7, 4, 2, 1, 7],
        'T' => [7, 2, 2, 2, 2],
        'U' => [5, 5, 5, 5, 2],
        'V' => [5, 5, 5, 5, 2],
        'Y' => [5, 5, 2, 2, 2],
        '?' => [6, 1, 2, 0, 2],
        _ => [0; 5],
    }
}

fn draw_counter(canvas: &mut Canvas<Window>, x: i32, y: i32, value: u32) -> Result<(), String> {
    let digits = format!("{:04}", value.min(9999));
    for (index, digit) in digits.bytes().enumerate() {
        draw_digit(canvas, x + index as i32 * 27, y, digit - b'0', PHOSPHOR)?;
    }
    Ok(())
}

fn draw_tape_reels(
    canvas: &mut Canvas<Window>,
    x: i32,
    y: i32,
    elapsed: Duration,
    rotating: bool,
    progress: u32,
    total: u32,
) -> Result<(), String> {
    // Standard audio cassette aspect ratio: 100mm x 64mm (approx 1.56 : 1)
    // Scaled for pixel display: 84px wide x 54px high (~1.55 : 1)
    const W: u32 = 84;
    const H: u32 = 54;
    let cassette_bg = Color::RGB(22, 26, 24);
    let cassette_label = Color::RGB(34, 40, 36);
    let label_border = Color::RGB(55, 65, 58);
    let tape_pack_color = Color::RGB(70, 50, 42);
    let tape_path_color = Color::RGBA(125, 90, 70, 160);
    let window_bg = Color::RGB(10, 12, 11);

    // Cassette shell body & outer bevel
    fill(canvas, Rect::new(x, y, W, H), cassette_bg)?;
    outline(canvas, Rect::new(x, y, W, H), Color::RGB(52, 62, 56))?;
    outline(
        canvas,
        Rect::new(x + 1, y + 1, W - 2, H - 2),
        Color::RGB(16, 20, 18),
    )?;

    // Four corner assembly screws
    let screw_color = Color::RGB(90, 105, 95);
    fill(canvas, Rect::new(x + 2, y + 2, 2, 2), screw_color)?;
    fill(
        canvas,
        Rect::new(x + (W as i32) - 4, y + 2, 2, 2),
        screw_color,
    )?;
    fill(
        canvas,
        Rect::new(x + 2, y + (H as i32) - 4, 2, 2),
        screw_color,
    )?;
    fill(
        canvas,
        Rect::new(x + (W as i32) - 4, y + (H as i32) - 4, 2, 2),
        screw_color,
    )?;

    // Center label sticker
    fill(canvas, Rect::new(x + 6, y + 4, W - 12, 37), cassette_label)?;
    outline(canvas, Rect::new(x + 6, y + 4, W - 12, 37), label_border)?;
    // Retro accent line on the label
    fill(canvas, Rect::new(x + 8, y + 6, W - 16, 1), AMBER)?;

    // Transparent inspection window
    let win_x = x + 11;
    let win_y = y + 10;
    let win_w = 62;
    let win_h = 29;
    fill(canvas, Rect::new(win_x, win_y, win_w, win_h), window_bg)?;
    outline(canvas, Rect::new(win_x, win_y, win_w, win_h), DIM)?;

    // Center reel hubs (standard spacing: ~42.5mm on 100mm cassette -> ratio ~0.425 * 58px)
    let left_center = (win_x + 16, win_y + 14);
    let right_center = (win_x + 46, win_y + 14);

    // Center guide tick marks between reels
    canvas.set_draw_color(Color::RGB(40, 50, 44));
    canvas.draw_line((win_x + 31, win_y + 3), (win_x + 31, win_y + 26))?;
    canvas.draw_line((win_x + 29, win_y + 14), (win_x + 33, win_y + 14))?;

    // Dynamic tape pack radius based on playback progress (0.0 to 1.0)
    let ratio = if total > 0 {
        f64::from(progress) / f64::from(total)
    } else {
        0.0
    }
    .clamp(0.0, 1.0);

    // Reel hub outer radius is 5px.
    // Full spool radius: 14px (increased by 3px from 11px)
    // Empty spool radius: 5px (bare hub core)
    let left_pack_r = 14.0 - ratio * 9.0;
    let right_pack_r = 5.0 + ratio * 9.0;

    // Angular rotation: differential speed
    let left_speed = 2.0 + (ratio * 1.5);
    let right_speed = 3.5 - (ratio * 1.5);
    let elapsed_sec = elapsed.as_secs_f64();
    let left_angle = if rotating {
        -elapsed_sec * left_speed
    } else {
        0.0
    };
    let right_angle = if rotating {
        elapsed_sec * right_speed
    } else {
        0.0
    };
    let roller_angle = if rotating { elapsed_sec * 9.5 } else { 0.0 };

    // Draw tape spools (wound magnetic tape packs with dynamic shimmering light reflection)
    draw_tape_pack(
        canvas,
        left_center.0,
        left_center.1,
        left_pack_r,
        left_angle,
        tape_pack_color,
    )?;
    draw_tape_pack(
        canvas,
        right_center.0,
        right_center.1,
        right_pack_r,
        -right_angle,
        tape_pack_color,
    )?;

    // Lower trapezoid tape path / head opening area (cassette chin)
    let trap_x = x + 10;
    let trap_y = y + 41;
    let trap_w = W - 20; // 64px
    let trap_h = 11;
    fill(
        canvas,
        Rect::new(trap_x, trap_y, trap_w, trap_h),
        Color::RGB(16, 19, 17),
    )?;
    outline(
        canvas,
        Rect::new(trap_x, trap_y, trap_w, trap_h),
        Color::RGB(48, 58, 52),
    )?;

    // Roller guide positions placed right at the outer lower corners near the shell frame
    let left_roller = (x + 6, y + 47);
    let right_roller = (x + (W as i32) - 7, y + 47);

    // Left and right capstan drive pass-through holes
    fill(
        canvas,
        Rect::new(trap_x + 9, trap_y + 4, 3, 3),
        Color::RGB(10, 12, 11),
    )?;
    fill(
        canvas,
        Rect::new(trap_x + (trap_w as i32) - 12, trap_y + 4, 3, 3),
        Color::RGB(10, 12, 11),
    )?;

    // Central magnetic head shield & felt pressure pad
    let head_center_x = trap_x + (trap_w as i32) / 2;
    fill(
        canvas,
        Rect::new(head_center_x - 5, trap_y + 4, 10, 5),
        Color::RGB(140, 110, 60), // bronze head shield
    )?;
    fill(
        canvas,
        Rect::new(head_center_x - 3, trap_y + 6, 6, 2),
        Color::RGB(200, 170, 110), // felt pad
    )?;

    // 1. Diagonal tape unwinding from supply pack down to left roller guide (1px single line)
    canvas.set_blend_mode(BlendMode::Blend);
    let left_tangent_x = (left_center.0 as f64 - left_pack_r).round() as i32;
    canvas.set_draw_color(tape_path_color);
    canvas.draw_line(
        (left_tangent_x, left_center.1),
        (left_roller.0 - 2, left_roller.1),
    )?;

    // 2. Diagonal tape winding from right roller guide up to take-up pack (1px single line)
    let right_tangent_x = (right_center.0 as f64 + right_pack_r).round() as i32;
    canvas.draw_line(
        (right_roller.0 + 2, right_roller.1),
        (right_tangent_x, right_center.1),
    )?;

    // 3. Horizontal magnetic tape running across the lower head opening between the rollers (1px line)
    canvas.draw_line(
        (left_roller.0 + 3, trap_y + 7),
        (right_roller.0 - 3, trap_y + 7),
    )?;
    canvas.set_blend_mode(BlendMode::None);

    // Draw the left and right roller guides with spinning flutes and center pins
    draw_roller_guide(canvas, left_roller.0, left_roller.1, roller_angle)?;
    draw_roller_guide(canvas, right_roller.0, right_roller.1, -roller_angle)?;

    // Vertical tape guide pins next to the head opening
    fill(
        canvas,
        Rect::new(head_center_x - 7, trap_y + 4, 1, 4),
        Color::RGB(90, 100, 95),
    )?;
    fill(
        canvas,
        Rect::new(head_center_x + 6, trap_y + 4, 1, 4),
        Color::RGB(90, 100, 95),
    )?;

    // Draw cassette reel hubs
    draw_reel_hub(canvas, left_center.0, left_center.1, left_angle, PHOSPHOR)?;
    draw_reel_hub(canvas, right_center.0, right_center.1, -right_angle, CREAM)?;

    Ok(())
}

fn draw_roller_guide(
    canvas: &mut Canvas<Window>,
    center_x: i32,
    center_y: i32,
    angle: f64,
) -> Result<(), String> {
    // 5px circular outer wheel with rounded rim (radius 2 circle with corner softening)
    // Outer flanged rim ring:
    let rim_color = Color::RGB(130, 140, 130);
    let wheel_color = Color::RGB(220, 225, 215);

    // Row dy = -2: 3px wide (-1..=1)
    fill(
        canvas,
        Rect::new(center_x - 1, center_y - 2, 3, 1),
        rim_color,
    )?;
    // Row dy = -1: 5px wide (-2..=2), rim on ends
    fill(
        canvas,
        Rect::new(center_x - 2, center_y - 1, 1, 1),
        rim_color,
    )?;
    fill(
        canvas,
        Rect::new(center_x - 1, center_y - 1, 3, 1),
        wheel_color,
    )?;
    fill(
        canvas,
        Rect::new(center_x + 2, center_y - 1, 1, 1),
        rim_color,
    )?;
    // Row dy = 0: 5px wide (-2..=2), rim on ends
    fill(canvas, Rect::new(center_x - 2, center_y, 1, 1), rim_color)?;
    fill(canvas, Rect::new(center_x - 1, center_y, 3, 1), wheel_color)?;
    fill(canvas, Rect::new(center_x + 2, center_y, 1, 1), rim_color)?;
    // Row dy = 1: 5px wide (-2..=2), rim on ends
    fill(
        canvas,
        Rect::new(center_x - 2, center_y + 1, 1, 1),
        rim_color,
    )?;
    fill(
        canvas,
        Rect::new(center_x - 1, center_y + 1, 3, 1),
        wheel_color,
    )?;
    fill(
        canvas,
        Rect::new(center_x + 2, center_y + 1, 1, 1),
        rim_color,
    )?;
    // Row dy = 2: 3px wide (-1..=1)
    fill(
        canvas,
        Rect::new(center_x - 1, center_y + 2, 3, 1),
        rim_color,
    )?;

    // Rotating flute / notch on the roller
    let notch_x = center_x + (angle.cos() * 1.4).round() as i32;
    let notch_y = center_y + (angle.sin() * 1.4).round() as i32;
    fill(
        canvas,
        Rect::new(notch_x, notch_y, 1, 1),
        Color::RGB(70, 80, 72),
    )?;

    // Central brass/steel pivot pin (center dot)
    fill(
        canvas,
        Rect::new(center_x, center_y, 1, 1),
        Color::RGB(40, 35, 30),
    )?;

    Ok(())
}

fn draw_circle_filled(
    canvas: &mut Canvas<Window>,
    center_x: i32,
    center_y: i32,
    radius: i32,
    color: Color,
) -> Result<(), String> {
    canvas.set_draw_color(color);
    let r2 = radius * radius;
    for dy in -radius..=radius {
        let max_dx = ((r2 - dy * dy) as f64).sqrt() as i32;
        canvas.draw_line(
            (center_x - max_dx, center_y + dy),
            (center_x + max_dx, center_y + dy),
        )?;
    }
    Ok(())
}

fn draw_tape_pack(
    canvas: &mut Canvas<Window>,
    center_x: i32,
    center_y: i32,
    radius: f64,
    angle: f64,
    pack_color: Color,
) -> Result<(), String> {
    let r = radius.round() as i32;
    draw_circle_filled(canvas, center_x, center_y, r, pack_color)?;

    // If tape is wound onto the spool beyond the core hub (r > 5), render dynamic reflection & winding texture
    if radius > 5.5 {
        let r_sq = (radius * radius) as i32;

        // Subtle specular pulse in brightness as the tape moves through the light beam
        let pulse = ((angle * 4.0).sin() * 18.0) as i32;
        let gleam_r = (142 + pulse).clamp(110, 175) as u8;
        let gleam_g = (112 + pulse * 3 / 4).clamp(85, 140) as u8;
        let gleam_b = (95 + pulse / 2).clamp(70, 118) as u8;
        let gleam_color = Color::RGB(gleam_r, gleam_g, gleam_b);

        // 1. Upper specular highlight crescent (anchored overhead, subtly undulating with movement)
        let gleam_shift = ((angle * 3.0).cos() * 1.0).round() as i32;
        for dy in -r..=-2 {
            let dy_f = dy as f64;
            let max_dx = ((radius * radius - dy_f * dy_f).max(0.0)).sqrt() as i32;
            for dx in -max_dx..=max_dx {
                let dist_sq = dx * dx + dy * dy;
                if dist_sq > 25 && dist_sq <= r_sq {
                    // Upper-left specular gleam
                    if dx < 0 && dy < -3 && dx > dy - 2 + gleam_shift && dx < dy + 8 + gleam_shift {
                        fill(
                            canvas,
                            Rect::new(center_x + dx, center_y + dy, 1, 1),
                            gleam_color,
                        )?;
                    } else if dy <= -(r / 2) && dx.abs() <= (r / 2) {
                        // Soft upper sheen arc
                        fill(
                            canvas,
                            Rect::new(center_x + dx, center_y + dy, 1, 1),
                            Color::RGB(98, 74, 62),
                        )?;
                    }
                }
            }
        }

        // 2. Anisotropic reflection streak (anchored at ~135°, shimmering as tape ribbons rotate)
        if radius >= 8.0 {
            let inner_r = 5.5;
            let outer_r = radius - 1.0;
            // Shimmering angular wobble in sync with spool rotation
            let shimmer = (angle * 3.0).sin() * 0.12;
            let streaks: [(f64, Color); 2] = [
                (2.356_f64 + shimmer, Color::RGB(118, 90, 76)), // ~135 deg highlight
                (5.498_f64 + shimmer * 0.7, Color::RGB(88, 64, 52)), // ~315 deg softer counter-sheen
            ];
            for (streak_angle, col) in streaks {
                let cos_a = streak_angle.cos();
                let sin_a = streak_angle.sin();
                let start = (
                    (center_x as f64 + cos_a * inner_r).round() as i32,
                    (center_y as f64 + sin_a * inner_r).round() as i32,
                );
                let end = (
                    (center_x as f64 + cos_a * outer_r).round() as i32,
                    (center_y as f64 + sin_a * outer_r).round() as i32,
                );
                canvas.set_draw_color(col);
                canvas.draw_line(start, end)?;
            }

            // Traveling glint along the main highlight streak showing tape layers gliding through
            for step in 0..2 {
                let phase = (angle * 0.8 + f64::from(step) * 0.5).rem_euclid(1.0);
                let glint_dist = inner_r + phase * (outer_r - inner_r);
                let main_angle = 2.356_f64 + shimmer;
                let gx = (center_x as f64 + main_angle.cos() * glint_dist).round() as i32;
                let gy = (center_y as f64 + main_angle.sin() * glint_dist).round() as i32;
                fill(canvas, Rect::new(gx, gy, 1, 1), Color::RGB(165, 138, 118))?;
            }
        }

        // 3. Concentric groove ring showing tight layers of wound magnetic tape
        if radius >= 9.0 {
            let ring_r = (radius * 0.68).round() as i32;
            let r2 = ring_r * ring_r;
            for dy in -ring_r..=ring_r {
                let dx = ((r2 - dy * dy) as f64).sqrt().round() as i32;
                if dx > 1 {
                    fill(
                        canvas,
                        Rect::new(center_x + dx, center_y + dy, 1, 1),
                        Color::RGB(52, 36, 30),
                    )?;
                    fill(
                        canvas,
                        Rect::new(center_x - dx, center_y + dy, 1, 1),
                        Color::RGB(52, 36, 30),
                    )?;
                }
            }
        }

        // 4. Subtle top-rim edge highlight
        let top_y = center_y - r;
        fill(
            canvas,
            Rect::new(center_x - 2, top_y + 1, 5, 1),
            Color::RGB(110, 84, 70),
        )?;
    }

    Ok(())
}

fn draw_reel_hub(
    canvas: &mut Canvas<Window>,
    center_x: i32,
    center_y: i32,
    angle: f64,
    color: Color,
) -> Result<(), String> {
    // Hub core ring (radius 5)
    for dy in -5_i32..=5 {
        for dx in -5_i32..=5 {
            let dist = dx * dx + dy * dy;
            if dist <= 25 && dist >= 7 {
                fill(canvas, Rect::new(center_x + dx, center_y + dy, 1, 1), color)?;
            }
        }
    }
    // 3 rotating punch holes in the hub ring for distinct spinning visibility
    for i in 0..3 {
        let hole_angle = angle + f64::from(i) * std::f64::consts::TAU / 3.0 + 0.52;
        let hx = (center_x as f64 + hole_angle.cos() * 3.6).round() as i32;
        let hy = (center_y as f64 + hole_angle.sin() * 3.6).round() as i32;
        fill(canvas, Rect::new(hx, hy, 1, 1), Color::RGB(10, 12, 11))?;
    }
    // Center spindle hole
    fill(
        canvas,
        Rect::new(center_x - 1, center_y - 1, 3, 3),
        Color::RGB(10, 12, 11),
    )?;

    // Drive splines / teeth protruding into the center hole
    canvas.set_draw_color(Color::RGB(25, 30, 27));
    for spoke in 0..6 {
        let spoke_angle = angle + f64::from(spoke) * std::f64::consts::TAU / 6.0;
        let inner = 1.0;
        let outer = 4.0;
        let start = (
            center_x + (spoke_angle.cos() * inner).round() as i32,
            center_y + (spoke_angle.sin() * inner).round() as i32,
        );
        let end = (
            center_x + (spoke_angle.cos() * outer).round() as i32,
            center_y + (spoke_angle.sin() * outer).round() as i32,
        );
        canvas.draw_line(start, end)?;
    }
    Ok(())
}

fn progress_bar(
    canvas: &mut Canvas<Window>,
    x: i32,
    y: i32,
    value: u32,
    total: u32,
    width: i32,
) -> Result<(), String> {
    outline(canvas, Rect::new(x, y, width as u32, 8), DIM)?;
    let filled = width * value.min(total) as i32 / total.max(1) as i32;
    if filled > 0 {
        fill(canvas, Rect::new(x + 1, y + 1, filled as u32, 6), PHOSPHOR)?;
    }
    Ok(())
}

fn draw_tracks(
    canvas: &mut Canvas<Window>,
    tracks: &[AudioTrack],
    cursor: usize,
    viewport_start: usize,
    selected: &[bool],
    focus_elapsed: Duration,
    library_error: Option<&str>,
    profile: &RuntimeProfile,
) -> Result<(), String> {
    fill(canvas, Rect::new(9, 37, 174, 170), PANEL)?;
    text(canvas, 14, 42, "TRACK QUEUE", 1, AMBER)?;
    text(
        canvas,
        103,
        42,
        &format!("SIDE {:02}:00", profile.side_duration_minutes),
        1,
        DIM,
    )?;
    canvas.set_draw_color(DIM);
    canvas.draw_line((14, 52), (177, 52))?;

    if tracks.is_empty() {
        text(canvas, 19, 76, "NO AUDIO TRACKS", 1, AMBER)?;
        text(canvas, 19, 90, "ADD FILES TO", 1, CREAM)?;
        text(canvas, 19, 100, "THE TRACKS FOLDER", 1, CREAM)?;
        text(canvas, 19, 140, "B BROWSE FOLDERS", 1, PHOSPHOR)?;
        if library_error.is_some() {
            text(canvas, 19, 120, "FOLDER UNAVAILABLE", 1, RED)?;
        }
        return Ok(());
    }

    let visible_end = (viewport_start + VISIBLE_TRACKS).min(tracks.len());
    for (row, track) in tracks[viewport_start..visible_end].iter().enumerate() {
        let index = viewport_start + row;
        let y = 57 + row as i32 * 20;
        if index == cursor {
            fill(
                canvas,
                Rect::new(12, y - 3, 168, 16),
                Color::RGB(42, 48, 43),
            )?;
        }
        let marker = if selected[index] { "[X]" } else { "[ ]" };
        let color = if index == cursor { PHOSPHOR } else { CREAM };
        text(canvas, 15, y, marker, 1, color)?;
        text(canvas, 38, y, &format!("{:02}", index + 1), 1, color)?;
        let display_name = track_name_text(
            &track.name,
            TRACK_NAME_WIDTH,
            index == cursor,
            focus_elapsed,
        );
        text(canvas, 56, y, &display_name, 1, color)?;
        text(
            canvas,
            144,
            y,
            &format!(
                "{:02}:{:02}",
                track.duration_seconds / 60,
                track.duration_seconds % 60
            ),
            1,
            DIM,
        )?;
        if index == cursor {
            text(canvas, 178, y, ">", 1, AMBER)?;
        }
    }

    draw_scrollbar(canvas, tracks.len(), viewport_start)?;
    Ok(())
}

fn auto_scroll_text(value: &str, width: usize) -> String {
    static START: OnceLock<Instant> = OnceLock::new();
    scrolling_text(value, width, START.get_or_init(Instant::now).elapsed())
}

fn scrolling_text(value: &str, width: usize, elapsed: Duration) -> String {
    if width == 0 {
        return String::new();
    }
    let characters: Vec<char> = value.chars().collect();
    if characters.len() <= width {
        return characters.into_iter().collect();
    }
    if elapsed < Duration::from_millis(900) {
        return characters[..width.saturating_sub(1)]
            .iter()
            .chain(std::iter::once(&'.'))
            .collect();
    }
    let padded: Vec<char> = format!("{value}    ").chars().collect();
    let offset = ((elapsed - Duration::from_millis(900)).as_millis() / 160) as usize;
    (0..width)
        .map(|index| padded[(offset + index) % padded.len()])
        .collect()
}

fn track_name_text(name: &str, width: usize, highlighted: bool, elapsed: Duration) -> String {
    if highlighted {
        scrolling_text(name, width, elapsed)
    } else {
        clipped_text(name, width)
    }
}

fn clipped_text(value: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let characters: Vec<char> = value.chars().collect();
    if characters.len() <= width {
        return characters.into_iter().collect();
    }
    characters[..width - 1]
        .iter()
        .chain(std::iter::once(&'.'))
        .collect()
}

fn draw_scrollbar(
    canvas: &mut Canvas<Window>,
    track_count: usize,
    viewport_start: usize,
) -> Result<(), String> {
    if track_count <= VISIBLE_TRACKS {
        return Ok(());
    }
    draw_scrollbar_at(
        canvas,
        176,
        56,
        144,
        2,
        track_count,
        viewport_start,
        VISIBLE_TRACKS,
    )
}

fn draw_scrollbar_at(
    canvas: &mut Canvas<Window>,
    x: i32,
    top: i32,
    height: u32,
    width: u32,
    track_count: usize,
    viewport_start: usize,
    visible_count: usize,
) -> Result<(), String> {
    if track_count <= visible_count {
        return Ok(());
    }
    fill(
        canvas,
        Rect::new(x, top, width, height),
        Color::RGB(45, 52, 47),
    )?;
    let thumb_height = ((height as usize * visible_count) / track_count).max(8) as u32;
    let max_start = track_count - visible_count;
    let travel = height - thumb_height;
    let thumb_y = top + (travel as usize * viewport_start / max_start) as i32;
    fill(
        canvas,
        Rect::new(x - 1, thumb_y, width + 2, thumb_height),
        AMBER,
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn tracks(count: usize) -> Vec<AudioTrack> {
        (0..count)
            .map(|index| AudioTrack {
                name: format!("track-{index}.wav"),
                path: PathBuf::from(format!("track-{index}.wav")),
                duration_seconds: 60,
                codec: "pcm_s16le".to_owned(),
            })
            .collect()
    }

    #[test]
    fn long_variable_text_scrolls_after_initial_pause() {
        assert_eq!(
            scrolling_text("abcdefghijklmnop", 8, Duration::ZERO),
            "abcdefg."
        );
        assert_eq!(
            scrolling_text("abcdefghijklmnop", 8, Duration::from_millis(1060)),
            "bcdefghi"
        );
    }

    #[test]
    fn short_variable_text_is_not_scrolled() {
        assert_eq!(scrolling_text("short", 8, Duration::from_secs(10)), "short");
    }

    #[test]
    fn recording_header_combines_profile_and_queue_names() {
        let mut profile = RuntimeProfile::default();
        profile.name = "My Deck".to_owned();
        let mut ui = AppUi::new(tracks(2), None);
        assert_eq!(ui.recording_session_label(&profile), "[My Deck+Unsaved]");
        ui.set_queue_path(std::path::Path::new("queues/Evening_Mix.json"));
        assert_eq!(
            ui.recording_session_label(&profile),
            "[My Deck+Evening_Mix]"
        );
        ui.handle_key(Keycode::Down, &profile);
        assert_eq!(
            ui.recording_session_label(&profile),
            "[My Deck+Evening_Mix]"
        );
        ui.set_queue_path(std::path::Path::new("queues/Evening_Mix_2.json"));
        assert_eq!(
            ui.recording_session_label(&profile),
            "[My Deck+Evening_Mix_2]"
        );
    }

    #[test]
    fn changing_selection_or_library_clears_saved_queue_identity() {
        let profile = RuntimeProfile::default();
        let path = std::path::Path::new("queues/Mix.json");
        let mut ui = AppUi::new(tracks(2), None);
        ui.set_queue_path(path);
        ui.handle_key(Keycode::Space, &profile);
        assert!(ui.queue_name.is_none());
        ui.set_queue_path(path);
        ui.handle_key(Keycode::Space, &profile);
        assert!(ui.queue_name.is_none());
        ui.set_queue_path(path);
        ui.clear_selection();
        assert!(ui.queue_name.is_none());
        ui.set_queue_path(path);
        ui.load_selection(&[]);
        assert!(ui.queue_name.is_none());
        ui.set_queue_path(path);
        ui.reset_library(tracks(1), None);
        assert!(ui.queue_name.is_none());
    }

    #[test]
    fn only_highlighted_track_names_scroll() {
        let name = "abcdefghijklmnop";
        assert_eq!(
            track_name_text(name, 8, false, Duration::from_secs(10)),
            "abcdefg."
        );
        assert_eq!(
            track_name_text(name, 8, true, Duration::from_millis(1060)),
            "bcdefghi"
        );
    }

    #[test]
    fn navigation_keeps_highlight_inside_viewport() {
        let profile = RuntimeProfile::default();
        let mut ui = AppUi::new(tracks(12), None);
        for _ in 0..7 {
            ui.handle_key(Keycode::Down, &profile);
        }
        assert_eq!(ui.cursor, 7);
        assert_eq!(ui.viewport_start, 1);

        ui.handle_key(Keycode::Home, &profile);
        assert_eq!(ui.cursor, 0);
        assert_eq!(ui.viewport_start, 0);

        ui.handle_key(Keycode::End, &profile);
        assert_eq!(ui.cursor, 11);
        assert_eq!(ui.viewport_start, 5);
    }

    #[test]
    fn track_selection_enforces_side_capacity() {
        let mut profile = RuntimeProfile::default();
        // 3 minutes capacity = 180 seconds. Leader = 10s, gap = 5s.
        profile.side_duration_minutes = 3;
        profile.leader_gap_seconds = 10;
        profile.track_gap_seconds = 5;

        // Tracks are 60 seconds each.
        // Track 0: 10 + 60 = 70s <= 180s. Fits!
        // Track 1: 10 + 60 + 60 + 5 = 135s <= 180s. Fits!
        // Track 2: 10 + 60 + 60 + 60 + 10 = 200s > 180s. Exceeds capacity!
        let mut ui = AppUi::new(tracks(5), None);

        // Select track 0
        ui.handle_key(Keycode::Space, &profile);
        assert_eq!(ui.selected_tracks().len(), 1);
        assert!(ui.selected[0]);

        // Move to track 1 and select
        ui.handle_key(Keycode::Down, &profile);
        ui.handle_key(Keycode::Space, &profile);
        assert_eq!(ui.selected_tracks().len(), 2);
        assert!(ui.selected[1]);

        // Move to track 2 and attempt to select - should be blocked
        ui.handle_key(Keycode::Down, &profile);
        ui.handle_key(Keycode::Space, &profile);
        assert_eq!(ui.selected_tracks().len(), 2);
        assert!(!ui.selected[2]);
        assert_eq!(ui.current_status(), None);
        assert!(ui.capacity_warning_started.is_some());

        // Deselect track 1
        ui.handle_key(Keycode::Up, &profile);
        ui.handle_key(Keycode::Space, &profile);
        assert_eq!(ui.selected_tracks().len(), 1);
        assert!(!ui.selected[1]);
        assert!(ui.capacity_warning_started.is_none());

        // Now track 2 can be selected
        ui.handle_key(Keycode::Down, &profile);
        ui.handle_key(Keycode::Space, &profile);
        assert_eq!(ui.selected_tracks().len(), 2);
        assert!(ui.selected[2]);
    }

    #[test]
    fn capacity_warning_blinks_and_expires() {
        assert_eq!(capacity_warning_phase(None), None);
        assert_eq!(capacity_warning_phase(Some(Duration::ZERO)), Some(true));
        assert_eq!(
            capacity_warning_phase(Some(Duration::from_millis(349))),
            Some(true)
        );
        assert_eq!(
            capacity_warning_phase(Some(Duration::from_millis(350))),
            Some(false)
        );
        assert_eq!(
            capacity_warning_phase(Some(Duration::from_millis(700))),
            Some(true)
        );
        assert!(capacity_warning_phase(Some(STATUS_TIMEOUT - Duration::from_millis(1))).is_some());
        assert_eq!(capacity_warning_phase(Some(STATUS_TIMEOUT)), None);
    }

    #[test]
    fn first_oversized_track_warns_without_replacing_status_and_resets_cleanly() {
        let mut profile = RuntimeProfile::default();
        profile.side_duration_minutes = 1;
        profile.leader_gap_seconds = 10;
        let mut ui = AppUi::new(tracks(1), None);
        ui.set_normalization_status("PROFILE SAVED");
        ui.handle_key(Keycode::Space, &profile);
        assert!(ui.selected_tracks().is_empty());
        assert!(ui.capacity_warning_started.is_some());
        assert_eq!(ui.current_status(), Some("PROFILE SAVED"));
        ui.clear_selection();
        assert!(ui.capacity_warning_started.is_none());
        ui.handle_key(Keycode::Space, &profile);
        ui.reset_library(tracks(1), None);
        assert!(ui.capacity_warning_started.is_none());
        ui.handle_key(Keycode::Space, &profile);
        ui.load_selection(&[]);
        assert!(ui.capacity_warning_started.is_none());
    }

    #[test]
    fn c_shortcut_clears_track_selection() {
        let profile = RuntimeProfile::default();
        let mut ui = AppUi::new(tracks(5), None);

        // Select first two tracks
        ui.handle_key(Keycode::Space, &profile);
        ui.handle_key(Keycode::Down, &profile);
        ui.handle_key(Keycode::Space, &profile);
        assert_eq!(ui.selected_tracks().len(), 2);
        assert!(ui.selected_panel);

        // Press C to clear selection
        ui.handle_key(Keycode::C, &profile);
        assert_eq!(ui.selected_tracks().len(), 0);
        assert!(!ui.selected[0]);
        assert!(!ui.selected[1]);
        assert!(!ui.selected_panel);

        // Pressing C again on empty selection is a safe no-op
        ui.handle_key(Keycode::C, &profile);
        assert_eq!(ui.selected_tracks().len(), 0);
    }

    #[test]
    fn status_notification_expires_after_timeout() {
        let mut ui = AppUi::new(tracks(2), None);
        assert_eq!(ui.current_status(), None);

        ui.set_normalization_status("PROFILE SAVED test.json");
        assert_eq!(ui.current_status(), Some("PROFILE SAVED test.json"));

        // Simulate time passage past STATUS_TIMEOUT
        ui.status_updated_at = Some(Instant::now() - Duration::from_secs(4));
        assert_eq!(ui.current_status(), None);

        // Clear status explicitly
        ui.set_normalization_status("ANOTHER STATUS");
        assert_eq!(ui.current_status(), Some("ANOTHER STATUS"));
        ui.clear_normalization_status();
        assert_eq!(ui.current_status(), None);
    }

    #[test]
    fn test_tone_status_stays_visible_past_notification_timeout() {
        let mut ui = AppUi::new(tracks(1), None);
        let path = std::path::Path::new("tones/tone_400hz.wav");
        ui.set_test_tone_preview(path, TestTone::Hz400);
        ui.set_normalization_status("OTHER NOTICE");
        ui.status_updated_at = Some(Instant::now() - Duration::from_secs(4));
        assert_eq!(ui.current_status(), Some("PLAYING TEST TONE 400Hz (Level)"));
        ui.clear_normalization_status();
        assert_eq!(ui.current_status(), Some("PLAYING TEST TONE 400Hz (Level)"));
        ui.sync_test_tone_preview(Some(path), true);
        assert_eq!(ui.current_status(), Some("PAUSED TEST TONE 400Hz (Level)"));
        ui.sync_test_tone_preview(Some(path), false);
        assert_eq!(ui.current_status(), Some("PLAYING TEST TONE 400Hz (Level)"));
        ui.sync_test_tone_preview(None, false);
        assert_eq!(ui.current_status(), None);
    }

    #[test]
    fn track_preview_status_tracks_playback_until_stop() {
        let mut ui = AppUi::new(tracks(2), None);
        let path = std::path::Path::new("cache/normalized.wav");
        ui.set_track_preview(path, "My Track.flac");
        ui.set_normalization_status("OTHER NOTICE");
        ui.status_updated_at = Some(Instant::now() - Duration::from_secs(4));
        assert_eq!(ui.current_status(), Some("PLAYING My Track.flac"));
        ui.clear_normalization_status();
        assert_eq!(ui.current_status(), Some("PLAYING My Track.flac"));
        ui.sync_test_tone_preview(Some(path), true);
        assert_eq!(ui.current_status(), Some("PAUSED My Track.flac"));
        ui.sync_test_tone_preview(Some(path), false);
        ui.handle_key(Keycode::Down, &RuntimeProfile::default());
        assert_eq!(ui.current_status(), Some("PLAYING My Track.flac"));
        ui.sync_test_tone_preview(None, false);
        assert_eq!(ui.current_status(), None);
    }

    #[test]
    fn track_preview_status_updates_on_switch_and_clears_on_exit() {
        let mut ui = AppUi::new(tracks(2), None);
        let first_path = std::path::Path::new("first.flac");
        let next_path = std::path::Path::new("second.wav");
        ui.set_track_preview(first_path, "First.flac");
        ui.set_track_preview(next_path, "Second.wav");
        ui.sync_test_tone_preview(Some(next_path), false);
        assert_eq!(ui.current_status(), Some("PLAYING Second.wav"));
        ui.set_previewing(false, false);
        assert_eq!(ui.current_status(), None);
        ui.set_track_preview(first_path, "First.flac");
        ui.set_test_tone_preview(next_path, TestTone::Hz400);
        assert_eq!(ui.current_status(), Some("PLAYING TEST TONE 400Hz (Level)"));
    }

    #[test]
    #[ignore = "requires SDL video; run with SDL_VIDEODRIVER=dummy"]
    fn preview_meters_override_queue_panels_and_restore_previous_view() {
        let sdl = sdl2::init().unwrap();
        let video = sdl.video().unwrap();
        let window = video
            .window("meter regression", 320, 240)
            .hidden()
            .build()
            .unwrap();
        let mut canvas = window.into_canvas().software().build().unwrap();
        let profile = RuntimeProfile::default();
        for (queue_view, test_tone) in [(false, false), (true, false), (false, true), (true, true)]
        {
            let mut ui = AppUi::new(tracks(1), None);
            ui.handle_key(Keycode::Space, &profile);
            ui.queue_view = queue_view;
            ui.render(&mut canvas, &profile, Some((8, 8))).unwrap();
            let previous_pixel = canvas
                .read_pixels(
                    Rect::new(204, 72, 1, 1),
                    sdl2::pixels::PixelFormatEnum::RGB24,
                )
                .unwrap();
            assert_ne!(previous_pixel, vec![PHOSPHOR.r, PHOSPHOR.g, PHOSPHOR.b]);

            if test_tone {
                ui.set_test_tone_preview(std::path::Path::new("tone.wav"), TestTone::Hz400);
            } else {
                ui.set_track_preview(std::path::Path::new("track.flac"), "Track.flac");
            }
            ui.set_previewing(true, false);
            ui.render(&mut canvas, &profile, Some((8, 8))).unwrap();
            let meter_pixel = canvas
                .read_pixels(
                    Rect::new(204, 72, 1, 1),
                    sdl2::pixels::PixelFormatEnum::RGB24,
                )
                .unwrap();
            assert_eq!(meter_pixel, vec![PHOSPHOR.r, PHOSPHOR.g, PHOSPHOR.b]);
            assert_eq!(ui.queue_view, queue_view);
            assert!(ui.selected_panel);
            assert_eq!(ui.selected_tracks().len(), 1);

            ui.set_previewing(false, false);
            ui.render(&mut canvas, &profile, None).unwrap();
            let restored_pixel = canvas
                .read_pixels(
                    Rect::new(204, 72, 1, 1),
                    sdl2::pixels::PixelFormatEnum::RGB24,
                )
                .unwrap();
            assert_eq!(restored_pixel, previous_pixel);
        }
    }

    #[test]
    fn test_tone_status_clears_on_track_switch_stop_and_library_reset() {
        let mut ui = AppUi::new(tracks(1), None);
        let path = std::path::Path::new("tones/tone_400hz.wav");
        ui.set_test_tone_preview(path, TestTone::Hz400);
        ui.sync_test_tone_preview(Some(std::path::Path::new("music.flac")), false);
        assert_eq!(ui.test_tone_status(), None);
        ui.set_test_tone_preview(path, TestTone::Hz400);
        ui.set_previewing(false, false);
        assert_eq!(ui.test_tone_status(), None);
        ui.set_test_tone_preview(path, TestTone::Hz400);
        ui.reset_library(tracks(1), None);
        assert_eq!(ui.test_tone_status(), None);
        ui.set_test_tone_preview(path, TestTone::Hz400);
        let next_path = std::path::Path::new("tones/tone_1000hz.wav");
        ui.set_test_tone_preview(next_path, TestTone::Hz1000);
        ui.sync_test_tone_preview(Some(next_path), false);
        assert_eq!(ui.current_status(), Some("PLAYING TEST TONE 1kHz (Ref)"));
    }

    fn folder_browser_fixture() -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("deckprep-folders-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn track_folder_browser_navigates_selects_and_cancels() {
        let root = folder_browser_fixture();
        let child = root.join("Music Tracks");
        std::fs::create_dir(&child).unwrap();
        std::fs::write(root.join("song.wav"), b"audio").unwrap();
        let mut browser = TrackFolderBrowser::new(root.to_str().unwrap()).unwrap();
        assert_eq!(browser.directories, vec![child.clone()]);
        assert_eq!(browser.handle_key(Keycode::Return), None);
        assert_eq!(browser.selected_folder(), child);
        assert_eq!(browser.handle_key(Keycode::Space), Some(true));
        assert_eq!(browser.handle_key(Keycode::Return), None);
        assert_eq!(browser.selected_folder(), child);
        browser.handle_key(Keycode::Backspace);
        assert_eq!(browser.selected_folder(), root);
        assert_eq!(browser.handle_key(Keycode::Escape), Some(false));
        assert_eq!(browser.handle_key(Keycode::Q), Some(false));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn track_folder_browser_recovers_missing_paths_and_failed_navigation() {
        let root = folder_browser_fixture();
        let missing = root.join("missing").join("tracks");
        let mut browser = TrackFolderBrowser::new(missing.to_str().unwrap()).unwrap();
        assert_eq!(browser.selected_folder(), root);
        assert_eq!(browser.handle_key(Keycode::Space), Some(true));
        let child = root.join("removed");
        std::fs::create_dir(&child).unwrap();
        browser.open(root.clone());
        std::fs::remove_dir(&child).unwrap();
        browser.handle_key(Keycode::Return);
        assert_eq!(browser.selected_folder(), root);
        assert!(browser.error.is_some());
        assert_eq!(browser.handle_key(Keycode::Space), None);
        browser.open(root.clone());
        assert!(browser.error.is_none());
        assert_eq!(browser.handle_key(Keycode::Space), Some(true));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn track_folder_browser_scrolls_and_resets_on_folder_change() {
        let root = folder_browser_fixture();
        for index in 0..12 {
            std::fs::create_dir(root.join(format!("folder-{index:02}"))).unwrap();
        }
        let mut browser = TrackFolderBrowser::new(root.to_str().unwrap()).unwrap();
        browser.handle_key(Keycode::PageDown);
        assert_eq!((browser.cursor, browser.viewport_start), (7, 1));
        browser.handle_key(Keycode::End);
        assert_eq!((browser.cursor, browser.viewport_start), (11, 5));
        browser.handle_key(Keycode::Down);
        assert_eq!(browser.cursor, 11);
        browser.handle_key(Keycode::Home);
        assert_eq!((browser.cursor, browser.viewport_start), (0, 0));
        browser.handle_key(Keycode::Up);
        assert_eq!(browser.cursor, 0);
        browser.handle_key(Keycode::End);
        browser.handle_key(Keycode::Right);
        assert_eq!((browser.cursor, browser.viewport_start), (0, 0));
        assert!(browser.directories.is_empty());
        browser.handle_key(Keycode::PageDown);
        browser.handle_key(Keycode::End);
        assert_eq!(browser.cursor, 0);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn track_folder_browser_cannot_navigate_above_root() {
        let mut browser = TrackFolderBrowser {
            folder: PathBuf::from("/"),
            directories: Vec::new(),
            cursor: 0,
            viewport_start: 0,
            error: None,
        };
        browser.handle_key(Keycode::Left);
        assert_eq!(browser.selected_folder(), std::path::Path::new("/"));
    }

    #[test]
    fn profile_browser_navigation_and_selection() {
        let paths = vec![
            std::path::PathBuf::from("profiles/p1.json"),
            std::path::PathBuf::from("profiles/p2.json"),
        ];
        let mut browser = ProfileBrowser::new(paths.clone());
        assert_eq!(browser.selected_path(), Some(paths[0].as_path()));

        browser.handle_key(Keycode::Down);
        assert_eq!(browser.selected_path(), Some(paths[1].as_path()));

        assert_eq!(browser.handle_key(Keycode::Return), Some(true));
        assert_eq!(browser.handle_key(Keycode::Escape), Some(false));
    }

    #[test]
    fn profile_editor_distinguishes_text_and_option_fields() {
        let profile = RuntimeProfile::default();
        let mut editor = ProfileEditor::new(&profile);

        // Fields 0 to 4 are text fields
        for field_index in 0..5 {
            editor.field = field_index;
            assert!(editor.is_text_field());
        }

        // Fields 5 to 12 are option/numerical fields
        for field_index in 5..=12 {
            editor.field = field_index;
            assert!(!editor.is_text_field());
        }

        // Editing tracks_folder preserves exact lowercase path
        editor.field = 1;
        editor.profile.tracks_folder.clear();
        editor.handle_text_input("/home/pi/Music_Tracks");
        assert_eq!(editor.profile.tracks_folder, "/home/pi/Music_Tracks");

        // Backspace removes last character
        editor.handle_backspace();
        assert_eq!(editor.profile.tracks_folder, "/home/pi/Music_Track");

        // On text fields, typing keys like Q, J, K don't quit or move fields
        assert!(!editor.handle_key(Keycode::Q));
        assert!(!editor.handle_key(Keycode::J));
        assert!(!editor.handle_key(Keycode::K));
        assert_eq!(editor.field, 1);

        // Arrow keys navigate fields
        editor.handle_key(Keycode::Down);
        assert_eq!(editor.field, 2);
        editor.handle_key(Keycode::Up);
        assert_eq!(editor.field, 1);

        // On option fields, Left/Right change values and Q quits
        editor.field = 5; // side_duration_minutes (default 30)
        assert!(!editor.is_text_field());
        editor.handle_key(Keycode::Right);
        assert_eq!(editor.profile.side_duration_minutes, 31);
        editor.handle_key(Keycode::Left);
        assert_eq!(editor.profile.side_duration_minutes, 30);
        assert!(editor.handle_key(Keycode::Q));
    }

    #[test]
    fn normalization_options_cycle_in_both_directions() {
        assert_eq!(
            cycle_normalization(NormalizationMethod::Peak, 1),
            NormalizationMethod::Lufs
        );
        assert_eq!(
            cycle_normalization(NormalizationMethod::Lufs, 1),
            NormalizationMethod::None
        );
        assert_eq!(
            cycle_normalization(NormalizationMethod::None, 1),
            NormalizationMethod::Peak
        );
        assert_eq!(
            cycle_normalization(NormalizationMethod::Peak, -1),
            NormalizationMethod::None
        );
        assert_eq!(
            cycle_normalization(NormalizationMethod::None, -1),
            NormalizationMethod::Lufs
        );
        assert_eq!(
            cycle_normalization(NormalizationMethod::Lufs, -1),
            NormalizationMethod::Peak
        );
    }

    #[test]
    fn glyph_supports_lowercase_and_path_symbols() {
        for c in 'a'..='z' {
            let bitmap = glyph(c);
            assert!(bitmap.iter().any(|&b| b > 0), "glyph for '{c}' is empty");
        }
        for c in ['_', '<', '>', '/', '\\', '~', '(', ')', '+', '-', '.'] {
            let bitmap = glyph(c);
            assert!(bitmap.iter().any(|&b| b > 0), "glyph for '{c}' is empty");
        }
    }

    #[test]
    fn queue_name_editor_flow_and_overwrite_confirmation() {
        let root = std::env::temp_dir().join("deckprep-ui-queue-test");
        std::fs::create_dir_all(&root).unwrap();

        // 1. New name saves immediately
        let mut editor = QueueNameEditor::new(&root);
        editor.handle_text_input("side_a");
        assert_eq!(editor.name, "side_a");
        assert_eq!(editor.handle_key(Keycode::Return), Some(true));
        assert!(!editor.confirming_overwrite);

        // Pre-create side_a.json in root
        std::fs::write(root.join("side_a.json"), "{}\n").unwrap();

        // 2. Existing name enters overwrite confirmation
        let mut editor2 = QueueNameEditor::new(&root);
        editor2.handle_text_input("side_a");
        assert_eq!(editor2.handle_key(Keycode::Return), None);
        assert!(editor2.confirming_overwrite);

        // 3. User cancels overwrite prompt with Escape -> stays in editor
        assert_eq!(editor2.handle_key(Keycode::Escape), None);
        assert!(!editor2.confirming_overwrite);

        // 4. Pressing Enter again re-prompts
        assert_eq!(editor2.handle_key(Keycode::Return), None);
        assert!(editor2.confirming_overwrite);

        // 5. User confirms overwrite with Return
        assert_eq!(editor2.handle_key(Keycode::Return), Some(true));
        assert_eq!(editor2.name, "side_a");

        // 6. User chooses auto-versioning with V
        let mut editor3 = QueueNameEditor::new(&root);
        editor3.handle_text_input("side_a");
        assert_eq!(editor3.handle_key(Keycode::Return), None);
        assert!(editor3.confirming_overwrite);
        assert_eq!(editor3.handle_key(Keycode::V), Some(true));
        assert_eq!(editor3.name, "side_a_1");

        std::fs::remove_dir_all(root).unwrap();
    }
}

fn draw_deck_monitor(
    canvas: &mut Canvas<Window>,
    armed: bool,
    previewing: bool,
    preview_paused: bool,
    meter_levels: Option<(usize, usize)>,
) -> Result<(), String> {
    fill(canvas, Rect::new(187, 37, 124, 170), PANEL)?;
    text(canvas, 194, 42, "OUTPUT LEVEL", 1, CREAM)?;
    canvas.set_draw_color(DIM);
    canvas.draw_line((194, 52), (305, 52))?;
    text(canvas, 194, 58, "L", 1, DIM)?;
    meter(canvas, 203, 58, meter_levels.map_or(0, |levels| levels.0))?;
    text(canvas, 194, 71, "R", 1, DIM)?;
    meter(canvas, 203, 71, meter_levels.map_or(0, |levels| levels.1))?;
    text(canvas, 203, 82, "-40  -20  -6   0", 1, DIM)?;

    text(canvas, 194, 100, "TAPE DECK", 1, AMBER)?;
    draw_cassette(canvas, 196, 114)?;
    let (status, color) = if previewing && preview_paused {
        ("PREVIEW PAUSE", AMBER)
    } else if previewing {
        ("PREVIEW PLAY", PHOSPHOR)
    } else if armed {
        ("READY TO PREP", AMBER)
    } else {
        ("MONITOR IDLE", PHOSPHOR)
    };
    text(canvas, 211, 153, status, 1, color)?;
    Ok(())
}

fn draw_selected_panel(
    canvas: &mut Canvas<Window>,
    tracks: &[AudioTrack],
    selection_order: &[usize],
    cursor: usize,
    focus_elapsed: Duration,
    profile: &RuntimeProfile,
    capacity_warning: Option<bool>,
) -> Result<(), String> {
    fill(canvas, Rect::new(187, 37, 124, 170), PANEL)?;
    text(canvas, 194, 42, "SELECTED", 1, AMBER)?;
    canvas.set_draw_color(DIM);
    canvas.draw_line((194, 52), (305, 52))?;
    let selected_tracks: Vec<(usize, &AudioTrack)> = selection_order
        .iter()
        .filter_map(|&source_index| tracks.get(source_index).map(|track| (source_index, track)))
        .collect();
    if selected_tracks.is_empty() {
        text(canvas, 194, 66, "NONE", 1, DIM)?;
    } else {
        for (index, (source_index, track)) in selected_tracks.iter().take(6).enumerate() {
            let y = 57 + index as i32 * 17;
            let highlighted = *source_index == cursor;
            let name = track_name_text(&track.name, 10, highlighted, focus_elapsed);
            if highlighted {
                fill(
                    canvas,
                    Rect::new(190, y - 3, 118, 14),
                    Color::RGB(42, 48, 43),
                )?;
            }
            let text_color = if highlighted { PHOSPHOR } else { CREAM };
            text(canvas, 193, y, &format!("{:02}", index + 1), 1, text_color)?;
            text(canvas, 209, y, &name, 1, text_color)?;
            text(
                canvas,
                272,
                y,
                "|",
                1,
                if highlighted { AMBER } else { DIM },
            )?;
            text(
                canvas,
                280,
                y,
                &format!(
                    "{:02}:{:02}",
                    track.duration_seconds / 60,
                    track.duration_seconds % 60
                ),
                1,
                if highlighted { AMBER } else { DIM },
            )?;
        }
    }
    let selected_audio: Vec<AudioTrack> = selected_tracks
        .iter()
        .map(|(_, track)| (*track).clone())
        .collect();
    let plan = SessionPlan::from_audio_tracks(profile, &selected_audio);
    let total_seconds = plan.total_seconds();
    let total_color = if plan.fits() { PHOSPHOR } else { RED };
    text(
        canvas,
        194,
        169,
        &format!("TOTAL {:02}:{:02}", total_seconds / 60, total_seconds % 60),
        1,
        total_color,
    )?;
    draw_capacity_limit(canvas, 194, 181, profile, capacity_warning)?;
    text(
        canvas,
        194,
        193,
        &format!("{} TRACKS", selected_tracks.len()),
        1,
        DIM,
    )?;
    Ok(())
}

fn capacity_warning_phase(elapsed: Option<Duration>) -> Option<bool> {
    elapsed
        .filter(|elapsed| *elapsed < STATUS_TIMEOUT)
        .map(|elapsed| (elapsed.as_millis() / 350) % 2 == 0)
}

fn draw_capacity_limit(
    canvas: &mut Canvas<Window>,
    x: i32,
    y: i32,
    profile: &RuntimeProfile,
    warning: Option<bool>,
) -> Result<(), String> {
    let label = format!("MAX {:02}:00", profile.side_duration_minutes);
    if warning.is_some() {
        let text_width = (label.chars().count() as u32 * 6).saturating_sub(1);
        fill(
            canvas,
            Rect::new(x - 3, y - 2, text_width + 6, 11),
            Color::RGB(180, 60, 52),
        )?;
    }
    if warning != Some(false) {
        text(
            canvas,
            x,
            y,
            &label,
            1,
            if warning.is_some() { CREAM } else { DIM },
        )?;
    }
    Ok(())
}

fn draw_cassette(canvas: &mut Canvas<Window>, x: i32, y: i32) -> Result<(), String> {
    outline(canvas, Rect::new(x, y, 106, 29), DIM)?;
    outline(canvas, Rect::new(x + 8, y + 5, 90, 13), CREAM)?;
    outline(canvas, Rect::new(x + 22, y + 8, 8, 8), AMBER)?;
    outline(canvas, Rect::new(x + 76, y + 8, 8, 8), AMBER)?;
    canvas.set_draw_color(DIM);
    canvas.draw_line((x + 37, y + 12), (x + 69, y + 12))?;
    canvas.draw_line((x + 14, y + 24), (x + 92, y + 24))?;
    Ok(())
}

fn meter(canvas: &mut Canvas<Window>, x: i32, y: i32, level: usize) -> Result<(), String> {
    for index in 0..14 {
        let color = if index < level {
            if index >= 12 {
                RED
            } else if index >= 9 {
                AMBER
            } else {
                PHOSPHOR
            }
        } else {
            Color::RGB(48, 55, 50)
        };
        fill(canvas, Rect::new(x + index as i32 * 7, y, 5, 7), color)?;
    }
    Ok(())
}

fn draw_digit(
    canvas: &mut Canvas<Window>,
    x: i32,
    y: i32,
    digit: u8,
    color: Color,
) -> Result<(), String> {
    const SEGMENTS: [[bool; 7]; 10] = [
        [true, true, true, false, true, true, true],
        [false, false, true, false, false, true, false],
        [true, false, true, true, true, false, true],
        [true, false, true, true, false, true, true],
        [false, true, true, true, false, true, false],
        [true, true, false, true, false, true, true],
        [true, true, false, true, true, true, true],
        [true, false, true, false, false, true, false],
        [true, true, true, true, true, true, true],
        [true, true, true, true, false, true, true],
    ];
    let positions = [
        Rect::new(x + 3, y, 14, 3),
        Rect::new(x, y + 3, 3, 12),
        Rect::new(x + 17, y + 3, 3, 12),
        Rect::new(x + 3, y + 15, 14, 3),
        Rect::new(x, y + 18, 3, 12),
        Rect::new(x + 17, y + 18, 3, 12),
        Rect::new(x + 3, y + 30, 14, 3),
    ];
    for (enabled, rect) in SEGMENTS[digit as usize].iter().zip(positions) {
        fill(
            canvas,
            rect,
            if *enabled {
                color
            } else {
                Color::RGB(35, 48, 39)
            },
        )?;
    }
    Ok(())
}

fn text(
    canvas: &mut Canvas<Window>,
    x: i32,
    y: i32,
    value: &str,
    scale: u32,
    color: Color,
) -> Result<(), String> {
    let mut cursor = x;
    for character in value.chars() {
        for (row, bits) in glyph(character).iter().enumerate() {
            for column in 0..5 {
                if bits & (1 << (4 - column)) != 0 {
                    fill(
                        canvas,
                        Rect::new(
                            cursor + column * scale as i32,
                            y + row as i32 * scale as i32,
                            scale,
                            scale,
                        ),
                        color,
                    )?;
                }
            }
        }
        cursor += 6 * scale as i32;
    }
    Ok(())
}

fn glyph(character: char) -> [u8; 7] {
    match character {
        'A' => [14, 17, 17, 31, 17, 17, 17],
        'B' => [30, 17, 17, 30, 17, 17, 30],
        'C' => [14, 17, 16, 16, 16, 17, 14],
        'D' => [30, 17, 17, 17, 17, 17, 30],
        'E' => [31, 16, 16, 30, 16, 16, 31],
        'F' => [31, 16, 16, 30, 16, 16, 16],
        'G' => [14, 17, 16, 23, 17, 17, 15],
        'H' => [17, 17, 17, 31, 17, 17, 17],
        'I' => [14, 4, 4, 4, 4, 4, 14],
        'J' => [7, 2, 2, 2, 18, 18, 12],
        'K' => [17, 18, 20, 24, 20, 18, 17],
        'L' => [16, 16, 16, 16, 16, 16, 31],
        'M' => [17, 27, 21, 21, 17, 17, 17],
        'N' => [17, 25, 21, 19, 17, 17, 17],
        'O' => [14, 17, 17, 17, 17, 17, 14],
        'P' => [30, 17, 17, 30, 16, 16, 16],
        'Q' => [14, 17, 17, 17, 21, 18, 13],
        'R' => [30, 17, 17, 30, 20, 18, 17],
        'S' => [15, 16, 16, 14, 1, 1, 30],
        'T' => [31, 4, 4, 4, 4, 4, 4],
        'U' => [17, 17, 17, 17, 17, 17, 14],
        'V' => [17, 17, 17, 17, 17, 10, 4],
        'W' => [17, 17, 17, 21, 21, 21, 10],
        'X' => [17, 17, 10, 4, 10, 17, 17],
        'Y' => [17, 17, 10, 4, 4, 4, 4],
        'Z' => [31, 1, 2, 4, 8, 16, 31],
        'a' => [0, 0, 14, 1, 15, 17, 15],
        'b' => [16, 16, 22, 25, 17, 17, 30],
        'c' => [0, 0, 14, 17, 16, 17, 14],
        'd' => [1, 1, 13, 19, 17, 17, 15],
        'e' => [0, 0, 14, 17, 31, 16, 14],
        'f' => [6, 9, 8, 28, 8, 8, 8],
        'g' => [0, 0, 15, 17, 15, 1, 14],
        'h' => [16, 16, 22, 25, 17, 17, 17],
        'i' => [4, 0, 12, 4, 4, 4, 14],
        'j' => [2, 0, 6, 2, 2, 18, 12],
        'k' => [16, 16, 18, 20, 24, 20, 18],
        'l' => [12, 4, 4, 4, 4, 4, 14],
        'm' => [0, 0, 26, 21, 21, 17, 17],
        'n' => [0, 0, 22, 25, 17, 17, 17],
        'o' => [0, 0, 14, 17, 17, 17, 14],
        'p' => [0, 0, 30, 17, 30, 16, 16],
        'q' => [0, 0, 15, 17, 15, 1, 1],
        'r' => [0, 0, 22, 25, 16, 16, 16],
        's' => [0, 0, 15, 16, 14, 1, 30],
        't' => [8, 8, 28, 8, 8, 9, 6],
        'u' => [0, 0, 17, 17, 17, 19, 13],
        'v' => [0, 0, 17, 17, 17, 10, 4],
        'w' => [0, 0, 17, 17, 21, 21, 10],
        'x' => [0, 0, 17, 10, 4, 10, 17],
        'y' => [0, 0, 17, 17, 15, 1, 14],
        'z' => [0, 0, 31, 2, 4, 8, 31],
        '0' => [14, 17, 19, 21, 25, 17, 14],
        '1' => [4, 12, 4, 4, 4, 4, 14],
        '2' => [14, 17, 1, 2, 4, 8, 31],
        '3' => [30, 1, 1, 14, 1, 1, 30],
        '4' => [2, 6, 10, 18, 31, 2, 2],
        '5' => [31, 16, 16, 30, 1, 1, 30],
        '6' => [14, 16, 16, 30, 17, 17, 14],
        '7' => [31, 1, 2, 4, 8, 8, 8],
        '8' => [14, 17, 17, 14, 17, 17, 14],
        '9' => [14, 17, 17, 15, 1, 1, 14],
        ':' => [0, 4, 4, 0, 4, 4, 0],
        '-' => [0, 0, 0, 31, 0, 0, 0],
        '_' => [0, 0, 0, 0, 0, 0, 31],
        '?' => [14, 17, 1, 2, 4, 0, 4],
        '|' => [4, 4, 4, 4, 4, 4, 4],
        '/' => [1, 1, 2, 4, 8, 16, 16],
        '\\' => [16, 16, 8, 4, 2, 1, 1],
        '[' => [14, 8, 8, 8, 8, 8, 14],
        ']' => [14, 2, 2, 2, 2, 2, 14],
        '<' => [2, 4, 8, 16, 8, 4, 2],
        '>' => [16, 8, 4, 2, 4, 8, 16],
        '(' => [2, 4, 8, 8, 8, 4, 2],
        ')' => [8, 4, 2, 2, 2, 4, 8],
        '+' => [0, 4, 4, 31, 4, 4, 0],
        '~' => [0, 13, 18, 0, 0, 0, 0],
        '=' => [0, 0, 31, 0, 31, 0, 0],
        ',' => [0, 0, 0, 0, 0, 12, 4],
        '.' => [0, 0, 0, 0, 0, 12, 12],
        _ => [0; 7],
    }
}

fn fill(canvas: &mut Canvas<Window>, rect: Rect, color: Color) -> Result<(), String> {
    canvas.set_draw_color(color);
    canvas.fill_rect(rect)
}

fn outline(canvas: &mut Canvas<Window>, rect: Rect, color: Color) -> Result<(), String> {
    canvas.set_draw_color(color);
    canvas.draw_rect(rect)
}
