use std::{path::Path, process::Command};

const SAMPLE_RATE: usize = 44_100;
const CHUNK_SAMPLES: usize = SAMPLE_RATE / 20;

#[derive(Debug, Clone)]
pub struct StereoMeter {
    levels: Vec<(usize, usize)>,
}

impl StereoMeter {
    pub fn analyze(path: impl AsRef<Path>) -> Result<Self, String> {
        let output = Command::new("ffmpeg")
            .args(["-hide_banner", "-loglevel", "error", "-i"])
            .arg(path.as_ref())
            .args(["-f", "s16le", "-ac", "2", "-ar", "44100", "-"])
            .output()
            .map_err(|error| format!("failed to analyze audio: {error}"))?;
        if !output.status.success() {
            return Err(format!(
                "ffmpeg meter analysis failed for {}",
                path.as_ref().display()
            ));
        }

        let mut levels = Vec::new();
        let bytes_per_frame = 4;
        let chunk_bytes = CHUNK_SAMPLES * bytes_per_frame;
        for chunk in output.stdout.chunks(chunk_bytes) {
            let mut sum_left = 0.0_f64;
            let mut sum_right = 0.0_f64;
            let mut frames = 0;
            for frame in chunk.chunks_exact(bytes_per_frame) {
                let left = i16::from_le_bytes([frame[0], frame[1]]) as f64 / 32768.0;
                let right = i16::from_le_bytes([frame[2], frame[3]]) as f64 / 32768.0;
                sum_left += left * left;
                sum_right += right * right;
                frames += 1;
            }
            if frames > 0 {
                levels.push((
                    rms_to_segments(sum_left / frames as f64),
                    rms_to_segments(sum_right / frames as f64),
                ));
            }
        }
        Ok(Self { levels })
    }

    pub fn level_at(&self, position_seconds: f64) -> (usize, usize) {
        if self.levels.is_empty() {
            return (0, 0);
        }
        let index = (position_seconds.max(0.0) * 20.0) as usize;
        self.levels[index.min(self.levels.len() - 1)]
    }
}

fn rms_to_segments(rms: f64) -> usize {
    if rms <= 0.000_001 {
        return 0;
    }
    let db = 20.0 * rms.sqrt().log10();
    (((db + 60.0) / 60.0) * 14.0).clamp(0.0, 14.0) as usize
}

#[cfg(test)]
mod tests {
    use std::{fs, process::Command, time::SystemTime};

    use super::*;

    #[test]
    fn analyzes_generated_tone() {
        let root = std::env::temp_dir().join(format!(
            "deckprep-meter-{}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("tone.wav");
        let generated = Command::new("ffmpeg")
            .args([
                "-hide_banner",
                "-loglevel",
                "error",
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=0.2",
                "-c:a",
                "pcm_s16le",
            ])
            .arg(&source)
            .status()
            .unwrap();
        assert!(generated.success());
        let meter = StereoMeter::analyze(&source).unwrap();
        assert!(meter.level_at(0.1).0 > 0);
        assert_eq!(meter.level_at(0.1).0, meter.level_at(0.1).1);
        fs::remove_dir_all(root).unwrap();
    }
}
