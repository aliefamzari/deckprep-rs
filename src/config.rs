use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

pub const PVM_WIDTH: u32 = 320;
pub const PVM_HEIGHT: u32 = 240;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DisplayBackend {
    DesktopWindow,
    DrmKms,
    GpioScartPlaceholder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NormalizationMethod {
    Peak,
    Lufs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CounterMode {
    Linear,
    Static,
    Auto,
    Manual,
}

impl Default for CounterMode {
    fn default() -> Self {
        Self::Linear
    }
}

impl Default for NormalizationMethod {
    fn default() -> Self {
        Self::Lufs
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeProfile {
    pub name: String,
    pub display_backend: DisplayBackend,
    pub logical_width: u32,
    pub logical_height: u32,
    pub window_scale: u32,
    pub fullscreen: bool,
    pub keyboard_enabled: bool,
    pub audio_backend: String,
    pub audio_device: String,
    pub tracks_folder: String,
    #[serde(default)]
    pub normalization: NormalizationMethod,
    #[serde(default = "default_target_lufs")]
    pub target_lufs: f64,
    #[serde(default = "default_normalized_folder")]
    pub normalized_folder: String,
    #[serde(default = "default_side_duration_minutes")]
    pub side_duration_minutes: u32,
    #[serde(default = "default_leader_gap_seconds")]
    pub leader_gap_seconds: u32,
    #[serde(default = "default_track_gap_seconds")]
    pub track_gap_seconds: u32,
    #[serde(default)]
    pub counter_mode: CounterMode,
    #[serde(default = "default_counter_rate")]
    pub counter_rate: f64,
    #[serde(default)]
    pub counter_config: Option<String>,
}

fn default_target_lufs() -> f64 {
    -14.0
}

fn default_normalized_folder() -> String {
    "normalized".to_owned()
}

fn default_side_duration_minutes() -> u32 {
    30
}

fn default_leader_gap_seconds() -> u32 {
    10
}

fn default_track_gap_seconds() -> u32 {
    5
}

fn default_counter_rate() -> f64 {
    1.0
}

impl Default for RuntimeProfile {
    fn default() -> Self {
        Self {
            name: "desktop-dev".to_owned(),
            display_backend: DisplayBackend::DesktopWindow,
            logical_width: PVM_WIDTH,
            logical_height: PVM_HEIGHT,
            window_scale: 3,
            fullscreen: false,
            keyboard_enabled: true,
            audio_backend: "sdl".to_owned(),
            audio_device: "default".to_owned(),
            tracks_folder: "tracks".to_owned(),
            normalization: NormalizationMethod::default(),
            target_lufs: default_target_lufs(),
            normalized_folder: default_normalized_folder(),
            side_duration_minutes: default_side_duration_minutes(),
            leader_gap_seconds: default_leader_gap_seconds(),
            track_gap_seconds: default_track_gap_seconds(),
            counter_mode: CounterMode::default(),
            counter_rate: default_counter_rate(),
            counter_config: None,
        }
    }
}

impl RuntimeProfile {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, String> {
        let path = path.as_ref();
        let contents = fs::read_to_string(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let profile: Self = serde_json::from_str(&contents)
            .map_err(|error| format!("invalid profile {}: {error}", path.display()))?;
        profile.validate()?;
        Ok(profile)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.logical_width != PVM_WIDTH || self.logical_height != PVM_HEIGHT {
            return Err(format!(
                "logical resolution must be {PVM_WIDTH}x{PVM_HEIGHT}"
            ));
        }
        if self.window_scale == 0 {
            return Err("window scale must be at least 1".to_owned());
        }
        if !self.keyboard_enabled {
            return Err("the initial release requires keyboard input".to_owned());
        }
        Ok(())
    }

    pub fn save_snapshot(&self, folder: impl AsRef<Path>) -> Result<PathBuf, String> {
        self.validate()?;
        let folder = folder.as_ref();
        fs::create_dir_all(folder)
            .map_err(|error| format!("failed to create {}: {error}", folder.display()))?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("system clock error: {error}"))?
            .as_secs();
        let stem: String = self
            .name
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() {
                    character.to_ascii_lowercase()
                } else {
                    '-'
                }
            })
            .collect();
        let stem = stem.trim_matches('-');
        let filename = format!(
            "{}-{}.json",
            if stem.is_empty() { "deck" } else { stem },
            timestamp
        );
        let path = folder.join(filename);
        let temporary_path = folder.join(format!(
            ".{}.tmp",
            path.file_name().unwrap().to_string_lossy()
        ));
        let contents = serde_json::to_string_pretty(self)
            .map_err(|error| format!("failed to serialize profile: {error}"))?;
        let result = (|| -> Result<(), String> {
            let mut file = File::create(&temporary_path)
                .map_err(|error| format!("failed to create profile: {error}"))?;
            file.write_all(contents.as_bytes())
                .and_then(|_| file.write_all(b"\n"))
                .and_then(|_| file.sync_all())
                .map_err(|error| format!("failed to write profile: {error}"))?;
            fs::rename(&temporary_path, &path)
                .map_err(|error| format!("failed to finalize profile: {error}"))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary_path);
        }
        result.map(|()| path)
    }

    pub fn list_profiles(folder: impl AsRef<Path>) -> Result<Vec<PathBuf>, String> {
        let folder = folder.as_ref();
        if !folder.exists() {
            return Ok(Vec::new());
        }
        let mut files: Vec<PathBuf> = fs::read_dir(folder)
            .map_err(|error| format!("failed to read {}: {error}", folder.display()))?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
            .filter(|path| {
                path.file_name()
                    .and_then(|name| name.to_str())
                    .map_or(false, |name| !name.starts_with('.'))
            })
            .collect();
        files.sort();
        Ok(files)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_profile() -> RuntimeProfile {
        RuntimeProfile::default()
    }

    #[test]
    fn accepts_pvm_resolution() {
        assert_eq!(valid_profile().validate(), Ok(()));
    }

    #[test]
    fn rejects_non_pvm_resolution() {
        let mut profile = valid_profile();
        profile.logical_width = 640;
        assert_eq!(
            profile.validate(),
            Err("logical resolution must be 320x240".to_owned())
        );
    }

    #[test]
    fn saves_reusable_profile_snapshot() {
        let profile = valid_profile();
        let folder = std::env::temp_dir().join("deckprep-profile-test");
        let path = profile.save_snapshot(&folder).unwrap();
        let loaded = RuntimeProfile::load(&path).unwrap();
        assert_eq!(loaded, profile);
        assert!(
            !path
                .with_file_name(format!(
                    ".{}.tmp",
                    path.file_name().unwrap().to_string_lossy()
                ))
                .exists()
        );
        let listed = RuntimeProfile::list_profiles(&folder).unwrap();
        assert_eq!(listed, vec![path]);
        fs::remove_dir_all(folder).unwrap();
    }
}
