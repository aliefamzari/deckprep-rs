use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver, TryRecvError},
    },
    thread::{self, JoinHandle},
    time::UNIX_EPOCH,
};

use sha2::{Digest, Sha256};

use crate::{config::NormalizationMethod, library::AudioTrack};

const TARGET_PEAK_DB: f64 = -1.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NormalizationSettings {
    pub method: NormalizationMethod,
    pub target_lufs: f64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedTrack {
    pub source: AudioTrack,
    pub path: PathBuf,
    pub cache_hit: bool,
}

pub enum NormalizationEvent {
    Processing { index: usize, name: String },
    Prepared(NormalizedTrack),
    Finished(Result<(), String>),
}

pub struct NormalizationJob {
    receiver: Receiver<NormalizationEvent>,
    cancelled: Arc<AtomicBool>,
    worker: Option<JoinHandle<()>>,
}

impl NormalizationJob {
    pub fn start(
        tracks: Vec<AudioTrack>,
        output_folder: PathBuf,
        settings: NormalizationSettings,
    ) -> Result<Self, String> {
        let (sender, receiver) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let worker = thread::Builder::new()
            .name("normalization".to_owned())
            .spawn(move || {
                let result = (|| {
                    for (index, track) in tracks.iter().enumerate() {
                        if worker_cancelled.load(Ordering::Relaxed) {
                            return Err("PREPARATION CANCELLED".to_owned());
                        }
                        sender
                            .send(NormalizationEvent::Processing {
                                index,
                                name: track.name.clone(),
                            })
                            .map_err(|_| "PREPARATION CLOSED".to_owned())?;
                        let prepared = normalize_track(track, &output_folder, settings)?;
                        sender
                            .send(NormalizationEvent::Prepared(prepared))
                            .map_err(|_| "PREPARATION CLOSED".to_owned())?;
                    }
                    if worker_cancelled.load(Ordering::Relaxed) {
                        return Err("PREPARATION CANCELLED".to_owned());
                    }
                    Ok(())
                })();
                let _ = sender.send(NormalizationEvent::Finished(result));
            })
            .map_err(|error| format!("failed to start preparation: {error}"))?;
        Ok(Self {
            receiver,
            cancelled,
            worker: Some(worker),
        })
    }

    pub fn poll(&self) -> Result<NormalizationEvent, TryRecvError> {
        self.receiver.try_recv()
    }

    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }
}

impl Drop for NormalizationJob {
    fn drop(&mut self) {
        self.cancel();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

pub fn clean_stale_cache_files(output_folder: impl AsRef<Path>) -> Result<usize, String> {
    let folder = output_folder.as_ref();
    if !folder.exists() {
        return Ok(0);
    }
    let mut removed = 0;
    let entries = fs::read_dir(folder)
        .map_err(|error| format!("failed to read cache folder {}: {error}", folder.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if let Some(file_name) = path.file_name().and_then(|n| n.to_str()) {
            if file_name.starts_with('.') && file_name.ends_with(".tmp.wav") {
                if fs::remove_file(&path).is_ok() {
                    removed += 1;
                }
            }
        }
    }
    Ok(removed)
}

pub fn normalize_track(
    track: &AudioTrack,
    output_folder: impl AsRef<Path>,
    settings: NormalizationSettings,
) -> Result<NormalizedTrack, String> {
    normalize_track_with(track, output_folder.as_ref(), settings, "ffmpeg")
}

fn normalize_track_with(
    track: &AudioTrack,
    output_folder: &Path,
    settings: NormalizationSettings,
    ffmpeg: &str,
) -> Result<NormalizedTrack, String> {
    if settings.method == NormalizationMethod::None {
        return Ok(NormalizedTrack {
            source: track.clone(),
            path: track.path.clone(),
            cache_hit: false,
        });
    }
    fs::create_dir_all(output_folder)
        .map_err(|error| format!("failed to create {}: {error}", output_folder.display()))?;
    let output_path = output_folder.join(format!("{}.wav", cache_key(track, settings)));
    if output_path.is_file() {
        return Ok(NormalizedTrack {
            source: track.clone(),
            path: output_path,
            cache_hit: true,
        });
    }

    let filter = match settings.method {
        NormalizationMethod::None => unreachable!("original tracks bypass normalization"),
        NormalizationMethod::Peak => {
            let peak_db = measure_peak(track.path.as_path(), ffmpeg)?;
            format!("volume={:.3}dB", TARGET_PEAK_DB - peak_db)
        }
        NormalizationMethod::Lufs => {
            format!("loudnorm=I={:.2}:TP=-1.0:LRA=11", settings.target_lufs)
        }
    };
    let temporary_path = output_folder.join(format!(".{}.tmp.wav", cache_key(track, settings)));
    let output = Command::new(ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y", "-i"])
        .arg(&track.path)
        .args(["-af", &filter, "-vn", "-c:a", "pcm_s16le"])
        .arg(&temporary_path)
        .output()
        .map_err(|error| format!("failed to run ffmpeg: {error}"))?;
    if !output.status.success() {
        let _ = fs::remove_file(&temporary_path);
        return Err(format!("ffmpeg normalization failed for {}", track.name));
    }
    fs::rename(&temporary_path, &output_path).map_err(|error| {
        let _ = fs::remove_file(&temporary_path);
        format!("failed to finalize normalized track: {error}")
    })?;

    Ok(NormalizedTrack {
        source: track.clone(),
        path: output_path,
        cache_hit: false,
    })
}

fn measure_peak(path: &Path, ffmpeg: &str) -> Result<f64, String> {
    let output = Command::new(ffmpeg)
        .args(["-hide_banner", "-i"])
        .arg(path)
        .args(["-af", "volumedetect", "-f", "null", "-"])
        .output()
        .map_err(|error| format!("failed to measure peak: {error}"))?;
    let stderr = String::from_utf8_lossy(&output.stderr);
    let peak = stderr
        .lines()
        .find_map(|line| {
            line.split_once("max_volume:")
                .map(|(_, value)| value.trim())
        })
        .and_then(|value| value.strip_suffix(" dB"))
        .and_then(|value| value.parse::<f64>().ok())
        .ok_or_else(|| format!("ffmpeg did not report a peak for {}", path.display()))?;
    if peak.is_finite() {
        Ok(peak)
    } else {
        Err(format!("invalid peak for {}", path.display()))
    }
}

fn cache_key(track: &AudioTrack, settings: NormalizationSettings) -> String {
    let modified = fs::metadata(&track.path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| format!("{}:{}", duration.as_secs(), duration.subsec_nanos()))
        .unwrap_or_else(|| "unknown-time".to_owned());
    let descriptor = format!(
        "{}|{}|{}|{}|{}|{}",
        track.path.display(),
        track.duration_seconds,
        track.codec,
        fs::metadata(&track.path)
            .map(|metadata| metadata.len())
            .unwrap_or(0),
        modified,
        match settings.method {
            NormalizationMethod::None => "none".to_owned(),
            NormalizationMethod::Peak => "peak".to_owned(),
            NormalizationMethod::Lufs => format!("lufs:{:.3}", settings.target_lufs),
        }
    );
    let digest = Sha256::digest(descriptor.as_bytes());
    format!("{:x}", digest)
}

#[cfg(test)]
mod tests {
    use std::{fs, process::Command, time::SystemTime};

    use super::*;

    fn track(path: &Path) -> AudioTrack {
        AudioTrack {
            name: "tone.wav".to_owned(),
            path: path.to_owned(),
            duration_seconds: 2,
            codec: "pcm_s16le".to_owned(),
        }
    }

    #[test]
    fn none_preserves_original_and_skips_ffmpeg_and_cache() {
        let root = std::env::temp_dir().join(format!(
            "deckprep-none-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("original.flac");
        fs::write(&path, b"unchanged original audio").unwrap();
        let output = root.join("cache");
        let source = track(&path);
        let prepared = normalize_track_with(
            &source,
            &output,
            NormalizationSettings {
                method: NormalizationMethod::None,
                target_lufs: -14.0,
            },
            "ffmpeg-must-not-be-called",
        )
        .unwrap();
        assert_eq!(prepared.path, path);
        assert_eq!(prepared.source, source);
        assert!(!prepared.cache_hit);
        assert!(!output.exists());
        assert_eq!(fs::read(&path).unwrap(), b"unchanged original audio");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn background_job_reports_ordered_progress_for_original_tracks() {
        let sources = vec![
            track(Path::new("first.flac")),
            track(Path::new("second.mp3")),
        ];
        let job = NormalizationJob::start(
            sources.clone(),
            PathBuf::from("unused-cache"),
            NormalizationSettings {
                method: NormalizationMethod::None,
                target_lufs: -14.0,
            },
        )
        .unwrap();
        for (expected_index, source) in sources.iter().enumerate() {
            match job
                .receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap()
            {
                NormalizationEvent::Processing { index, name } => {
                    assert_eq!(index, expected_index);
                    assert_eq!(name, source.name);
                }
                _ => panic!("expected processing event"),
            }
            match job
                .receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap()
            {
                NormalizationEvent::Prepared(prepared) => assert_eq!(prepared.path, source.path),
                _ => panic!("expected prepared event"),
            }
        }
        assert!(matches!(
            job.receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap(),
            NormalizationEvent::Finished(Ok(()))
        ));
    }

    #[test]
    fn background_job_reports_normalization_errors() {
        let root = std::env::temp_dir().join(format!(
            "deckprep-job-error-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let job = NormalizationJob::start(
            vec![track(&root.join("missing.wav"))],
            root.clone(),
            NormalizationSettings {
                method: NormalizationMethod::Peak,
                target_lufs: -14.0,
            },
        )
        .unwrap();
        assert!(matches!(
            job.receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap(),
            NormalizationEvent::Processing { index: 0, .. }
        ));
        assert!(matches!(
            job.receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .unwrap(),
            NormalizationEvent::Finished(Err(_))
        ));
        drop(job);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cache_key_changes_with_normalization_settings() {
        let path = std::env::temp_dir().join(format!(
            "deckprep-normalize-{}.wav",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::write(&path, b"fixture").unwrap();
        let source = track(&path);
        let peak = cache_key(
            &source,
            NormalizationSettings {
                method: NormalizationMethod::Peak,
                target_lufs: -14.0,
            },
        );
        let lufs = cache_key(
            &source,
            NormalizationSettings {
                method: NormalizationMethod::Lufs,
                target_lufs: -14.0,
            },
        );
        let other_lufs = cache_key(
            &source,
            NormalizationSettings {
                method: NormalizationMethod::Lufs,
                target_lufs: -16.0,
            },
        );
        assert_ne!(peak, lufs);
        assert_ne!(lufs, other_lufs);
        assert_eq!(lufs.len(), 64);
        fs::remove_file(path).unwrap();
    }

    #[test]
    fn ffmpeg_normalizes_and_reuses_cache() {
        let root = std::env::temp_dir().join(format!(
            "deckprep-normalize-e2e-{}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let source_path = root.join("source.wav");
        let output_folder = root.join("normalized");
        fs::create_dir_all(&root).unwrap();
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
            .arg(&source_path)
            .status()
            .unwrap();
        assert!(generated.success());

        let source = AudioTrack {
            name: "source.wav".to_owned(),
            path: source_path.clone(),
            duration_seconds: 1,
            codec: "pcm_s16le".to_owned(),
        };
        let settings = NormalizationSettings {
            method: NormalizationMethod::Peak,
            target_lufs: -14.0,
        };
        let first = normalize_track(&source, &output_folder, settings).unwrap();
        assert!(!first.cache_hit);
        assert!(first.path.is_file());
        let second = normalize_track(&source, &output_folder, settings).unwrap();
        assert!(second.cache_hit);
        assert_eq!(first.path, second.path);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cleans_stale_temporary_cache_files() {
        let folder = std::env::temp_dir().join(format!(
            "deckprep-stale-test-{}",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&folder).unwrap();

        // Create a normal cached wav and two interrupted temporary files
        let normal = folder.join("hash123.wav");
        let stale1 = folder.join(".hash456.tmp.wav");
        let stale2 = folder.join(".hash789.tmp.wav");
        let unrelated = folder.join("notes.txt");

        fs::write(&normal, b"cached audio").unwrap();
        fs::write(&stale1, b"interrupted").unwrap();
        fs::write(&stale2, b"interrupted").unwrap();
        fs::write(&unrelated, b"do not delete").unwrap();

        let removed = clean_stale_cache_files(&folder).unwrap();
        assert_eq!(removed, 2);
        assert!(!stale1.exists());
        assert!(!stale2.exists());
        assert!(normal.exists());
        assert!(unrelated.exists());

        fs::remove_dir_all(folder).unwrap();
    }
}
