use std::time::{Duration, Instant};

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
    queue::match_tracks,
};

const BACKGROUND: Color = Color::RGB(12, 15, 16);
const PANEL: Color = Color::RGB(27, 31, 29);
const PHOSPHOR: Color = Color::RGB(112, 224, 141);
const AMBER: Color = Color::RGB(242, 178, 74);
const CREAM: Color = Color::RGB(220, 218, 196);
const DIM: Color = Color::RGB(88, 101, 91);
const RED: Color = Color::RGB(211, 76, 65);
const VISIBLE_TRACKS: usize = 5;
const TRACK_NAME_WIDTH: usize = 14;

pub struct AppUi {
    tracks: Vec<AudioTrack>,
    cursor: usize,
    viewport_start: usize,
    selected: Vec<bool>,
    armed: bool,
    previewing: bool,
    preview_paused: bool,
    normalization_status: String,
    prep_requested: bool,
    focus_changed_at: Instant,
    library_error: Option<String>,
    selected_panel: bool,
    queue_view: bool,
}

pub struct ProfileEditor {
    pub profile: RuntimeProfile,
    field: usize,
}

pub struct QueueNameEditor {
    pub name: String,
}

impl QueueNameEditor {
    pub fn new() -> Self {
        Self {
            name: String::new(),
        }
    }

    pub fn handle_text_input(&mut self, value: &str) {
        self.name.push_str(value);
    }

    pub fn handle_key(&mut self, key: Keycode) -> Option<bool> {
        match key {
            Keycode::Backspace => {
                self.name.pop();
                None
            }
            Keycode::Return => Some(true),
            Keycode::Escape | Keycode::Q => Some(false),
            _ => None,
        }
    }

    pub fn render(&self, canvas: &mut Canvas<Window>) -> Result<(), String> {
        canvas.set_draw_color(BACKGROUND);
        canvas.clear();
        outline(canvas, Rect::new(25, 75, 270, 90), AMBER)?;
        fill(canvas, Rect::new(27, 77, 266, 86), PANEL)?;
        text(canvas, 43, 88, "SAVE QUEUE", 2, PHOSPHOR)?;
        text(canvas, 43, 108, "NAME", 1, CREAM)?;
        text(canvas, 43, 123, &format!("{}_", self.name), 1, AMBER)?;
        text(canvas, 43, 146, "ENTER SAVE   ESC CANCEL", 1, DIM)?;
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
            ("P", "PREVIEW"),
            ("X", "STOP"),
            ("N", "NORMALIZE"),
            ("CTRL-S/O", "QUEUE SAVE/LOAD"),
            ("ENTER", "PREPARE"),
            ("TAB / V", "SELECTED QUEUE"),
            ("W", "PROFILE EDITOR"),
        ],
        LegendContext::Review => &[
            ("UP/DOWN", "MOVE TRACK"),
            ("PGUP/PGDN", "JUMP"),
            ("P", "PREVIEW"),
            ("X", "STOP"),
            ("H/L", "SEEK 10 SEC"),
            ("ENTER", "COUNTDOWN"),
            ("ESC", "BACK"),
        ],
        LegendContext::ProfileEditor => &[
            ("UP/DOWN", "MOVE FIELD"),
            ("LEFT/RIGHT", "CHANGE VALUE"),
            ("TYPE", "EDIT TEXT"),
            ("BACKSPACE", "DELETE"),
            ("CTRL-S", "SAVE"),
            ("ESC", "CLOSE"),
        ],
    };
    for (index, (key, action)) in lines.iter().enumerate() {
        let y = 64 + index as i32 * 17;
        text(canvas, 42, y, key, 1, AMBER)?;
        text(canvas, 142, y, action, 1, CREAM)?;
    }
    text(canvas, 42, 193, "? / ESC CLOSE", 1, RED)?;
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
    ) -> Result<(), String> {
        canvas.set_draw_color(BACKGROUND);
        canvas.clear();
        outline(canvas, Rect::new(6, 6, 308, 228), DIM)?;
        fill(canvas, Rect::new(9, 9, 302, 25), PANEL)?;
        text(canvas, 14, 13, "NORMALIZATION COMPLETE", 2, PHOSPHOR)?;
        text(canvas, 14, 43, "REVIEW TRACKS BEFORE COUNTDOWN", 1, CREAM)?;
        text(
            canvas,
            210,
            43,
            &format!("{} TRACKS", self.tracks.len()),
            1,
            AMBER,
        )?;

        let visible_end = (self.viewport_start + 7).min(self.tracks.len());
        for (row, (name, duration, cache_hit)) in self.tracks[self.viewport_start..visible_end]
            .iter()
            .enumerate()
        {
            let index = self.viewport_start + row;
            let status = if *cache_hit { "CACHE" } else { "NEW" };
            let color = if *cache_hit { DIM } else { PHOSPHOR };
            if index == self.cursor {
                fill(
                    canvas,
                    Rect::new(11, 57 + row as i32 * 17, 298, 14),
                    Color::RGB(42, 48, 43),
                )?;
            }
            let display_name = visible_track_name(
                name,
                index == self.cursor,
                marquee_offset(self.focus_changed_at.elapsed(), name.chars().count()),
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
        text(canvas, 14, 215, "P PREVIEW", 1, AMBER)?;
        text(canvas, 104, 215, "X STOP", 1, RED)?;
        text(canvas, 160, 215, "ENTER NEXT", 1, AMBER)?;
        if !self.preview_status.is_empty() {
            text(canvas, 235, 215, &self.preview_status, 1, PHOSPHOR)?;
        }
        text(
            canvas,
            14,
            224,
            &format!(
                "NORMALIZATION {:?}  TARGET {:+.1}",
                profile.normalization, profile.target_lufs
            ),
            1,
            DIM,
        )?;
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

    pub fn handle_key(&mut self, key: Keycode) -> bool {
        match key {
            Keycode::Up | Keycode::K => self.field = self.field.saturating_sub(1),
            Keycode::Down | Keycode::J => self.field = (self.field + 1).min(12),
            Keycode::Left | Keycode::H => self.change_value(-1),
            Keycode::Right | Keycode::L => self.change_value(1),
            Keycode::Backspace => self.handle_backspace(),
            Keycode::Escape | Keycode::Q => return true,
            _ => {}
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
        text(canvas, 14, 43, "UP/DOWN FIELD  LEFT/RIGHT CHANGE", 1, CREAM)?;
        let fields = [
            format!("NAME            {}", self.profile.name),
            format!("TRACKS FOLDER   {}", self.profile.tracks_folder),
            format!("NORMALIZED DIR  {}", self.profile.normalized_folder),
            format!("AUDIO DEVICE    {}", self.profile.audio_device),
            format!(
                "CALIBRATION     {}",
                self.profile.counter_config.as_deref().unwrap_or("NONE")
            ),
            format!("SIDE MINUTES    {:>3}", self.profile.side_duration_minutes),
            format!("LEADER GAP      {:>3}S", self.profile.leader_gap_seconds),
            format!("TRACK GAP       {:>3}S", self.profile.track_gap_seconds),
            format!("TARGET LUFS    {:>+5.1}", self.profile.target_lufs),
            format!("COUNTER RATE   {:>5.1}", self.profile.counter_rate),
            format!("NORMALIZATION   {:?}", self.profile.normalization),
            format!("COUNTER MODE    {:?}", self.profile.counter_mode),
            format!(
                "FULLSCREEN      {}",
                if self.profile.fullscreen { "YES" } else { "NO" }
            ),
        ];
        let first_field = self.field.saturating_sub(5);
        for (row, field) in fields.iter().skip(first_field).take(8).enumerate() {
            let index = first_field + row;
            let color = if index == self.field { PHOSPHOR } else { CREAM };
            if index == self.field {
                fill(
                    canvas,
                    Rect::new(12, 61 + row as i32 * 17, 296, 14),
                    Color::RGB(42, 48, 43),
                )?;
            }
            text(canvas, 18, 65 + row as i32 * 17, field, 1, color)?;
        }
        fill(canvas, Rect::new(9, 211, 302, 20), PANEL)?;
        text(canvas, 14, 215, "CTRL-S SAVE", 1, AMBER)?;
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
    if direction > 0 {
        match value {
            NormalizationMethod::Peak => NormalizationMethod::Lufs,
            NormalizationMethod::Lufs => NormalizationMethod::Peak,
        }
    } else {
        match value {
            NormalizationMethod::Peak => NormalizationMethod::Lufs,
            NormalizationMethod::Lufs => NormalizationMethod::Peak,
        }
    }
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
            armed: false,
            previewing: false,
            preview_paused: false,
            normalization_status: String::new(),
            prep_requested: false,
            focus_changed_at: Instant::now(),
            library_error,
            selected_panel: false,
            queue_view: false,
        }
    }

    pub fn handle_key(&mut self, key: Keycode) {
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
                self.selected[self.cursor] = !self.selected[self.cursor];
                self.selected_panel = self.selected.iter().any(|is_selected| *is_selected);
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
        self.previewing = previewing;
        self.preview_paused = paused;
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
        self.tracks
            .iter()
            .zip(&self.selected)
            .filter(|(_, selected)| **selected)
            .map(|(track, _)| track.clone())
            .collect()
    }

    pub fn load_selection(&mut self, saved_paths: &[std::path::PathBuf]) -> usize {
        let (selected, missing) = match_tracks(&self.tracks, saved_paths);
        self.selected = selected;
        self.selected_panel = self.selected.iter().any(|is_selected| *is_selected);
        missing
    }

    pub fn set_normalization_status(&mut self, status: impl Into<String>) {
        self.normalization_status = status.into();
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
        if self.queue_view {
            return self.render_queue_workspace(canvas, profile);
        }
        canvas.set_draw_color(BACKGROUND);
        canvas.clear();

        outline(canvas, Rect::new(6, 6, 308, 228), DIM)?;
        fill(canvas, Rect::new(9, 9, 302, 25), PANEL)?;
        text(canvas, 14, 13, "DECKPREP", 2, PHOSPHOR)?;
        text(canvas, 121, 14, "TAPE RECORDING CONSOLE", 1, CREAM)?;
        text(canvas, 121, 24, "320X240 / 60HZ", 1, AMBER)?;

        fill(canvas, Rect::new(9, 37, 302, 13), Color::RGB(18, 21, 20))?;
        text(
            canvas,
            13,
            40,
            &format!("PROFILE: {}", profile.name),
            1,
            DIM,
        )?;
        text(canvas, 207, 40, "USB DAC: DEFAULT", 1, DIM)?;

        draw_tracks(
            canvas,
            &self.tracks,
            self.cursor,
            self.viewport_start,
            &self.selected,
            self.focus_changed_at.elapsed(),
            self.library_error.as_deref(),
        )?;
        if self.selected_panel {
            draw_selected_panel(
                canvas,
                &self.tracks,
                &self.selected,
                self.cursor,
                self.focus_changed_at.elapsed(),
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
        if !self.normalization_status.is_empty() {
            text(canvas, 14, 205, &self.normalization_status, 1, AMBER)?;
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
                &compact_track_name(&track.name, 10),
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
        draw_scrollbar_at(canvas, 151, 58, 2, self.tracks.len(), self.viewport_start)?;

        let selected_tracks: Vec<(usize, &AudioTrack)> = self
            .tracks
            .iter()
            .enumerate()
            .zip(&self.selected)
            .filter(|(_, selected)| **selected)
            .map(|((index, track), _)| (index, track))
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
                &compact_track_name(&track.name, 10),
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

        let total_seconds: u32 = selected_tracks
            .iter()
            .map(|(_, track)| track.duration_seconds)
            .sum();
        text(
            canvas,
            169,
            180,
            &format!("TOTAL {:02}:{:02}", total_seconds / 60, total_seconds % 60),
            1,
            PHOSPHOR,
        )?;
        text(
            canvas,
            169,
            190,
            &format!("SIDE {:02}:00", profile.side_duration_minutes),
            1,
            DIM,
        )?;
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
    ) -> Result<(), String> {
        canvas.set_draw_color(BACKGROUND);
        canvas.clear();
        outline(canvas, Rect::new(6, 6, 308, 228), DIM)?;
        fill(canvas, Rect::new(9, 9, 302, 25), PANEL)?;
        text(canvas, 14, 13, "DECKPREP", 2, PHOSPHOR)?;
        text(canvas, 121, 14, "DECK RECORDING MODE", 1, CREAM)?;
        text(canvas, 121, 24, "PVM 320X240 / 60HZ", 1, AMBER)?;

        text(canvas, 14, 43, phase, 1, AMBER)?;
        text(
            canvas,
            207,
            43,
            &format!("PROFILE {}", profile.name),
            1,
            DIM,
        )?;
        text(canvas, 14, 57, "TAPE COUNTER", 1, CREAM)?;
        draw_counter(canvas, 33, 70, counter)?;
        text(canvas, 14, 109, "LINEAR / MM:SS", 1, DIM)?;

        let total = total_seconds.max(1);
        let progress = elapsed_seconds.min(total);
        text(
            canvas,
            14,
            120,
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
        progress_bar(canvas, 14, 132, progress, total, 170)?;

        let current_label = current_track
            .and_then(|index| track_names.get(index))
            .map(|name| visible_track_name(name, false, 0))
            .unwrap_or_else(|| "STANDBY".to_owned());
        text(canvas, 14, 146, "NOW PLAYING", 1, AMBER)?;
        text(canvas, 14, 157, &current_label, 1, PHOSPHOR)?;

        fill(canvas, Rect::new(9, 171, 302, 36), PANEL)?;
        text(canvas, 14, 175, "SESSION TRACKS", 1, AMBER)?;
        for (index, name) in track_names.iter().take(3).enumerate() {
            let color = if current_track == Some(index) {
                PHOSPHOR
            } else {
                CREAM
            };
            let display_name = visible_track_name(name, false, 0);
            text(
                canvas,
                16,
                185 + index as i32 * 8,
                &format!("{:02} {}", index + 1, display_name),
                1,
                color,
            )?;
        }

        fill(canvas, Rect::new(9, 211, 302, 20), PANEL)?;
        text(canvas, 14, 215, "Q CANCEL", 1, RED)?;
        text(canvas, 100, 215, "L", 1, DIM)?;
        meter(canvas, 110, 216, meter_levels.0)?;
        text(canvas, 100, 224, "R", 1, DIM)?;
        meter(canvas, 110, 225, meter_levels.1)?;
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
) -> Result<(), String> {
    fill(canvas, Rect::new(9, 53, 174, 154), PANEL)?;
    text(canvas, 14, 58, "TRACK QUEUE", 1, AMBER)?;
    text(canvas, 109, 58, "SIDE A / C60", 1, DIM)?;
    canvas.set_draw_color(DIM);
    canvas.draw_line((14, 68), (177, 68))?;

    if tracks.is_empty() {
        text(canvas, 19, 86, "NO AUDIO TRACKS", 1, AMBER)?;
        text(canvas, 19, 100, "ADD FILES TO", 1, CREAM)?;
        text(canvas, 19, 110, "THE TRACKS FOLDER", 1, CREAM)?;
        if library_error.is_some() {
            text(canvas, 19, 130, "FOLDER UNAVAILABLE", 1, RED)?;
        }
        return Ok(());
    }

    let visible_end = (viewport_start + VISIBLE_TRACKS).min(tracks.len());
    for (row, track) in tracks[viewport_start..visible_end].iter().enumerate() {
        let index = viewport_start + row;
        let y = 75 + row as i32 * 23;
        if index == cursor {
            fill(
                canvas,
                Rect::new(12, y - 3, 168, 18),
                Color::RGB(42, 48, 43),
            )?;
        }
        let marker = if selected[index] { "[X]" } else { "[ ]" };
        let color = if index == cursor { PHOSPHOR } else { CREAM };
        text(canvas, 15, y, marker, 1, color)?;
        text(canvas, 38, y, &format!("{:02}", index + 1), 1, color)?;
        let display_name = visible_track_name(
            &track.name,
            index == cursor,
            marquee_offset(focus_elapsed, track.name.chars().count()),
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

    let selected_count = selected.iter().filter(|value| **value).count();
    let selected_seconds: u32 = tracks
        .iter()
        .zip(selected)
        .filter(|(_, is_selected)| **is_selected)
        .map(|(track, _)| track.duration_seconds)
        .sum();
    text(
        canvas,
        14,
        194,
        &format!("SELECTED {selected_count:02}"),
        1,
        PHOSPHOR,
    )?;
    text(
        canvas,
        113,
        194,
        &format!(
            "{:02}:{:02} / 30:00",
            selected_seconds / 60,
            selected_seconds % 60
        ),
        1,
        AMBER,
    )?;
    Ok(())
}

fn visible_track_name(name: &str, highlighted: bool, offset: usize) -> String {
    let characters: Vec<char> = name.chars().collect();
    if characters.len() <= TRACK_NAME_WIDTH {
        return characters.into_iter().collect();
    }
    if !highlighted {
        return characters[..TRACK_NAME_WIDTH - 1]
            .iter()
            .chain(std::iter::once(&'.'))
            .collect();
    }

    let padded: Vec<char> = format!("{name}   ").chars().collect();
    (0..TRACK_NAME_WIDTH)
        .map(|index| padded[(offset + index) % padded.len()])
        .collect()
}

fn marquee_offset(elapsed: Duration, name_length: usize) -> usize {
    if name_length <= TRACK_NAME_WIDTH || elapsed < Duration::from_millis(900) {
        return 0;
    }
    ((elapsed - Duration::from_millis(900)).as_millis() / 180) as usize
}

fn draw_scrollbar(
    canvas: &mut Canvas<Window>,
    track_count: usize,
    viewport_start: usize,
) -> Result<(), String> {
    if track_count <= VISIBLE_TRACKS {
        return Ok(());
    }
    draw_scrollbar_at(canvas, 176, 73, 2, track_count, viewport_start)
}

fn draw_scrollbar_at(
    canvas: &mut Canvas<Window>,
    x: i32,
    top: i32,
    width: u32,
    track_count: usize,
    viewport_start: usize,
) -> Result<(), String> {
    const HEIGHT: u32 = 109;
    if track_count <= VISIBLE_TRACKS {
        return Ok(());
    }
    fill(
        canvas,
        Rect::new(x, top, width, HEIGHT),
        Color::RGB(45, 52, 47),
    )?;
    let thumb_height = ((HEIGHT as usize * VISIBLE_TRACKS) / track_count).max(8) as u32;
    let max_start = track_count - VISIBLE_TRACKS;
    let travel = HEIGHT - thumb_height;
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
    fn clips_unfocused_long_names() {
        assert_eq!(
            visible_track_name("a-very-long-track-name.flac", false, 0),
            "a-very-long-t."
        );
    }

    #[test]
    fn highlighted_long_names_scroll() {
        assert_eq!(
            visible_track_name("abcdefghijklmnop", true, 3),
            "defghijklmnop "
        );
    }

    #[test]
    fn navigation_keeps_highlight_inside_viewport() {
        let mut ui = AppUi::new(tracks(12), None);
        for _ in 0..7 {
            ui.handle_key(Keycode::Down);
        }
        assert_eq!(ui.cursor, 7);
        assert_eq!(ui.viewport_start, 3);

        ui.handle_key(Keycode::Home);
        assert_eq!(ui.cursor, 0);
        assert_eq!(ui.viewport_start, 0);

        ui.handle_key(Keycode::End);
        assert_eq!(ui.cursor, 11);
        assert_eq!(ui.viewport_start, 7);
    }
}

fn draw_deck_monitor(
    canvas: &mut Canvas<Window>,
    armed: bool,
    previewing: bool,
    preview_paused: bool,
    meter_levels: Option<(usize, usize)>,
) -> Result<(), String> {
    fill(canvas, Rect::new(187, 53, 124, 154), PANEL)?;
    text(canvas, 194, 58, "OUTPUT LEVEL", 1, CREAM)?;
    text(canvas, 194, 73, "L", 1, DIM)?;
    meter(canvas, 203, 73, meter_levels.map_or(0, |levels| levels.0))?;
    text(canvas, 194, 86, "R", 1, DIM)?;
    meter(canvas, 203, 86, meter_levels.map_or(0, |levels| levels.1))?;
    text(canvas, 203, 97, "-40  -20  -6   0", 1, DIM)?;

    text(canvas, 194, 112, "TAPE DECK", 1, AMBER)?;
    draw_cassette(canvas, 196, 124)?;
    let (status, color) = if previewing && preview_paused {
        ("PREVIEW PAUSE", AMBER)
    } else if previewing {
        ("PREVIEW PLAY", PHOSPHOR)
    } else if armed {
        ("READY TO PREP", AMBER)
    } else {
        ("MONITOR IDLE", PHOSPHOR)
    };
    text(canvas, 211, 159, status, 1, color)?;
    Ok(())
}

fn draw_selected_panel(
    canvas: &mut Canvas<Window>,
    tracks: &[AudioTrack],
    selected: &[bool],
    cursor: usize,
    focus_elapsed: Duration,
) -> Result<(), String> {
    fill(canvas, Rect::new(187, 53, 124, 154), PANEL)?;
    text(canvas, 194, 58, "SELECTED", 1, AMBER)?;
    let selected_tracks: Vec<(usize, &AudioTrack)> = tracks
        .iter()
        .enumerate()
        .zip(selected)
        .filter(|(_, is_selected)| **is_selected)
        .map(|((index, track), _)| (index, track))
        .collect();
    if selected_tracks.is_empty() {
        text(canvas, 194, 82, "NONE", 1, DIM)?;
    } else {
        for (index, (source_index, track)) in selected_tracks.iter().take(6).enumerate() {
            let y = 75 + index as i32 * 17;
            let highlighted = *source_index == cursor;
            let name = compact_track_name_scrolling(
                &track.name,
                10,
                highlighted,
                marquee_offset(focus_elapsed, track.name.chars().count()),
            );
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
    let total_seconds: u32 = selected_tracks
        .iter()
        .map(|(_, track)| track.duration_seconds)
        .sum();
    text(
        canvas,
        194,
        184,
        &format!("TOTAL {:02}:{:02}", total_seconds / 60, total_seconds % 60),
        1,
        PHOSPHOR,
    )?;
    text(
        canvas,
        194,
        195,
        &format!("{} TRACKS", selected_tracks.len()),
        1,
        DIM,
    )?;
    Ok(())
}

fn compact_track_name(name: &str, width: usize) -> String {
    let characters: Vec<char> = name.chars().collect();
    if characters.len() <= width {
        return characters.into_iter().collect();
    }
    characters[..width.saturating_sub(1)]
        .iter()
        .chain(std::iter::once(&'.'))
        .collect()
}

fn compact_track_name_scrolling(
    name: &str,
    width: usize,
    highlighted: bool,
    offset: usize,
) -> String {
    let characters: Vec<char> = name.chars().collect();
    if characters.len() <= width {
        return characters.into_iter().collect();
    }
    if !highlighted {
        return compact_track_name(name, width);
    }
    let padded: Vec<char> = format!("{name}   ").chars().collect();
    (0..width)
        .map(|index| padded[(offset + index) % padded.len()])
        .collect()
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
            if index > 11 { RED } else { PHOSPHOR }
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
    for character in value.to_ascii_uppercase().chars() {
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
        '?' => [14, 17, 1, 2, 4, 0, 4],
        '|' => [4, 4, 4, 4, 4, 4, 4],
        '/' => [1, 1, 2, 4, 8, 16, 16],
        '[' => [14, 8, 8, 8, 8, 8, 14],
        ']' => [14, 2, 2, 2, 2, 2, 14],
        '>' => [16, 8, 4, 2, 4, 8, 16],
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
