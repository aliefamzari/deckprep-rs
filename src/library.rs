use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use serde::Deserialize;

const SUPPORTED_EXTENSIONS: &[&str] = &["mp3", "wav", "flac", "webm", "m4a", "aac", "ogg"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioTrack {
    pub name: String,
    pub path: PathBuf,
    pub duration_seconds: u32,
    pub codec: String,
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    format: Option<ProbeFormat>,
    #[serde(default)]
    streams: Vec<ProbeStream>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
}

pub fn scan_tracks(folder: impl AsRef<Path>) -> Result<Vec<AudioTrack>, String> {
    scan_tracks_with(folder.as_ref(), "ffprobe")
}

fn scan_tracks_with(folder: &Path, ffprobe: &str) -> Result<Vec<AudioTrack>, String> {
    if !folder.is_dir() {
        return Err(format!("track folder not found: {}", folder.display()));
    }

    let mut paths: Vec<PathBuf> = fs::read_dir(folder)
        .map_err(|error| format!("failed to read {}: {error}", folder.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_supported(path))
        .collect();
    paths.sort_by_key(|path| {
        path.file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_lowercase()
    });

    Ok(paths
        .into_iter()
        .filter_map(|path| probe_track(&path, ffprobe).ok())
        .collect())
}

fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            SUPPORTED_EXTENSIONS
                .iter()
                .any(|supported| extension.eq_ignore_ascii_case(supported))
        })
}

fn probe_track(path: &Path, ffprobe: &str) -> Result<AudioTrack, String> {
    let output = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=codec_type,codec_name",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .map_err(|error| format!("failed to run ffprobe: {error}"))?;
    if !output.status.success() {
        return Err(format!("ffprobe could not read {}", path.display()));
    }

    let probe: ProbeOutput = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("invalid ffprobe output: {error}"))?;
    let duration = probe
        .format
        .and_then(|format| format.duration)
        .and_then(|duration| duration.parse::<f64>().ok())
        .filter(|duration| duration.is_finite() && *duration >= 0.0)
        .ok_or_else(|| format!("missing duration for {}", path.display()))?;
    let codec = probe
        .streams
        .into_iter()
        .find(|stream| stream.codec_type.as_deref() == Some("audio"))
        .and_then(|stream| stream.codec_name)
        .unwrap_or_else(|| "unknown".to_owned());

    Ok(AudioTrack {
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        path: path.to_path_buf(),
        duration_seconds: duration.round() as u32,
        codec,
    })
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;

    #[test]
    fn scans_supported_files_in_name_order() {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("deckprep-library-{unique}"));
        let tracks = root.join("tracks");
        fs::create_dir_all(&tracks).unwrap();
        fs::write(tracks.join("B Song.FLAC"), []).unwrap();
        fs::write(tracks.join("a-long-track-name.mp3"), []).unwrap();
        fs::write(tracks.join("notes.txt"), []).unwrap();

        let probe = root.join("ffprobe-test");
        fs::write(
            &probe,
            "#!/bin/sh\nprintf '%s' '{\"format\":{\"duration\":\"125.6\"},\"streams\":[{\"codec_type\":\"audio\",\"codec_name\":\"flac\"}]}'\n",
        )
        .unwrap();
        let mut permissions = fs::metadata(&probe).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&probe, permissions).unwrap();

        let result = scan_tracks_with(&tracks, probe.to_str().unwrap()).unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].name, "a-long-track-name.mp3");
        assert_eq!(result[0].duration_seconds, 126);
        assert_eq!(result[1].name, "B Song.FLAC");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_a_missing_folder() {
        let error =
            scan_tracks_with(Path::new("folder-that-does-not-exist"), "ffprobe").unwrap_err();
        assert!(error.starts_with("track folder not found:"));
    }
}
