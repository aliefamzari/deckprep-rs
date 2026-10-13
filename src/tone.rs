use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestTone {
    Hz400,
    Hz1000,
    Hz10000,
    Hz15000,
    Sweep,
}

impl TestTone {
    pub fn from_key_number(num: u8) -> Option<Self> {
        match num {
            1 => Some(Self::Hz400),
            2 => Some(Self::Hz1000),
            3 => Some(Self::Hz10000),
            4 => Some(Self::Hz15000),
            5 => Some(Self::Sweep),
            _ => None,
        }
    }

    pub fn frequency_hz(&self) -> Option<u32> {
        match self {
            Self::Hz400 => Some(400),
            Self::Hz1000 => Some(1000),
            Self::Hz10000 => Some(10000),
            Self::Hz15000 => Some(15000),
            Self::Sweep => None,
        }
    }

    pub fn duration_seconds(&self) -> u32 {
        match self {
            Self::Sweep => 30,
            _ => 120,
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Hz400 => "400Hz (Level)",
            Self::Hz1000 => "1kHz (Ref)",
            Self::Hz10000 => "10kHz (Bias)",
            Self::Hz15000 => "15kHz (Azimuth)",
            Self::Sweep => "20Hz-20kHz Sweep",
        }
    }

    pub fn filename(&self) -> &'static str {
        match self {
            Self::Hz400 => "tone_400hz.wav",
            Self::Hz1000 => "tone_1000hz.wav",
            Self::Hz10000 => "tone_10000hz.wav",
            Self::Hz15000 => "tone_15000hz.wav",
            Self::Sweep => "tone_sweep_20_20000hz.wav",
        }
    }
}

pub fn generate_test_tone(
    tone: TestTone,
    output_folder: impl AsRef<Path>,
) -> Result<PathBuf, String> {
    generate_test_tone_with(tone, output_folder.as_ref(), "ffmpeg")
}

fn generate_test_tone_with(
    tone: TestTone,
    output_folder: &Path,
    ffmpeg: &str,
) -> Result<PathBuf, String> {
    fs::create_dir_all(output_folder).map_err(|error| {
        format!(
            "failed to create tone folder {}: {error}",
            output_folder.display()
        )
    })?;
    let target_path = output_folder.join(tone.filename());
    if target_path.is_file() {
        return Ok(target_path);
    }

    let temporary_path = output_folder.join(format!(".{}.tmp.wav", tone.filename()));
    let duration = tone.duration_seconds();

    let mut command = Command::new(ffmpeg);
    command.args(["-hide_banner", "-loglevel", "error", "-y", "-f", "lavfi"]);

    match tone {
        TestTone::Sweep => {
            let sweep_filter = format!(
                "aevalsrc='0.8*sin(2*PI*20*(exp(t*log(1000)/{d})-1)/(log(1000)/{d}))|0.8*sin(2*PI*20*(exp(t*log(1000)/{d})-1)/(log(1000)/{d}))':s=44100:d={d}",
                d = duration
            );
            command.args(["-i", &sweep_filter]);
        }
        _ => {
            let freq = tone.frequency_hz().unwrap();
            let sine_filter = format!("sine=frequency={freq}:duration={duration}");
            command.args(["-i", &sine_filter, "-ac", "2", "-ar", "44100"]);
        }
    }

    command.args(["-c:a", "pcm_s16le"]).arg(&temporary_path);

    let output = command
        .output()
        .map_err(|error| format!("failed to generate test tone: {error}"))?;
    if !output.status.success() {
        let _ = fs::remove_file(&temporary_path);
        return Err(format!("ffmpeg failed to generate {}", tone.label()));
    }

    fs::rename(&temporary_path, &target_path).map_err(|error| {
        let _ = fs::remove_file(&temporary_path);
        format!("failed to finalize tone: {error}")
    })?;

    Ok(target_path)
}

#[cfg(test)]
mod tests {
    use std::time::SystemTime;

    use super::*;

    #[test]
    fn test_tone_mappings_and_metadata() {
        assert_eq!(TestTone::from_key_number(1), Some(TestTone::Hz400));
        assert_eq!(TestTone::from_key_number(2), Some(TestTone::Hz1000));
        assert_eq!(TestTone::from_key_number(3), Some(TestTone::Hz10000));
        assert_eq!(TestTone::from_key_number(4), Some(TestTone::Hz15000));
        assert_eq!(TestTone::from_key_number(5), Some(TestTone::Sweep));
        assert_eq!(TestTone::from_key_number(6), None);

        assert_eq!(TestTone::Hz400.frequency_hz(), Some(400));
        assert_eq!(TestTone::Sweep.frequency_hz(), None);
        assert_eq!(TestTone::Hz400.duration_seconds(), 120);
        assert_eq!(TestTone::Sweep.duration_seconds(), 30);
    }

    #[test]
    fn generates_and_caches_test_tone() {
        let root = std::env::temp_dir().join(format!(
            "deckprep-tone-test-{}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();

        let path = generate_test_tone(TestTone::Hz400, &root).unwrap();
        assert!(path.is_file());
        assert_eq!(path.file_name().unwrap(), "tone_400hz.wav");

        // Calling again reuses the cached file
        let path2 = generate_test_tone(TestTone::Hz400, &root).unwrap();
        assert_eq!(path, path2);

        fs::remove_dir_all(root).unwrap();
    }
}
