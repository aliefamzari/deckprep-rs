use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::library::AudioTrack;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedQueue {
    pub version: u32,
    pub created_at: String,
    pub tracks: Vec<SavedQueueTrack>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedQueueTrack {
    pub name: String,
    pub path: PathBuf,
}

pub fn save_queue(folder: impl AsRef<Path>, tracks: &[AudioTrack]) -> Result<PathBuf, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock error: {error}"))?
        .as_secs();
    save_queue_named(folder, &format!("queue_{timestamp}"), tracks)
}

pub fn save_queue_named(
    folder: impl AsRef<Path>,
    name: &str,
    tracks: &[AudioTrack],
) -> Result<PathBuf, String> {
    let folder = folder.as_ref();
    fs::create_dir_all(folder)
        .map_err(|error| format!("failed to create {}: {error}", folder.display()))?;
    let clean_name: String = name
        .trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();
    if clean_name.is_empty() {
        return Err("queue name cannot be empty".to_owned());
    }
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock error: {error}"))?
        .as_secs();
    let path = folder.join(format!("{clean_name}.json"));
    let queue = SavedQueue {
        version: 1,
        created_at: timestamp.to_string(),
        tracks: tracks
            .iter()
            .map(|track| SavedQueueTrack {
                name: track.name.clone(),
                path: track.path.clone(),
            })
            .collect(),
    };
    let contents = serde_json::to_string_pretty(&queue)
        .map_err(|error| format!("failed to serialize queue: {error}"))?;
    fs::write(&path, format!("{contents}\n"))
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    Ok(path)
}

pub fn load_latest_queue(folder: impl AsRef<Path>) -> Result<Vec<PathBuf>, String> {
    let folder = folder.as_ref();
    let mut files: Vec<PathBuf> = fs::read_dir(folder)
        .map_err(|error| format!("failed to read {}: {error}", folder.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect();
    files.sort();
    let path = files
        .pop()
        .ok_or_else(|| "no saved queue found".to_owned())?;
    let contents = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let queue: SavedQueue = serde_json::from_str(&contents)
        .map_err(|error| format!("invalid queue {}: {error}", path.display()))?;
    Ok(queue.tracks.into_iter().map(|track| track.path).collect())
}

pub fn match_tracks(tracks: &[AudioTrack], saved_paths: &[PathBuf]) -> (Vec<bool>, usize) {
    let mut selected = vec![false; tracks.len()];
    let mut missing = 0;
    for saved_path in saved_paths {
        if let Some((index, _)) = tracks
            .iter()
            .enumerate()
            .find(|(_, track)| track.path == *saved_path)
        {
            selected[index] = true;
        } else {
            missing += 1;
        }
    }
    (selected, missing)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(name: &str) -> AudioTrack {
        AudioTrack {
            name: name.to_owned(),
            path: PathBuf::from(format!("tracks/{name}")),
            duration_seconds: 60,
            codec: "wav".to_owned(),
        }
    }

    #[test]
    fn matches_saved_paths_and_reports_missing_tracks() {
        let tracks = vec![track("one.wav"), track("two.wav")];
        let (selected, missing) = match_tracks(
            &tracks,
            &[
                PathBuf::from("tracks/two.wav"),
                PathBuf::from("tracks/lost.wav"),
            ],
        );
        assert_eq!(selected, vec![false, true]);
        assert_eq!(missing, 1);
    }

    #[test]
    fn sanitizes_named_queue_filename() {
        let root = std::env::temp_dir().join("deckprep-queue-name-test");
        let path = save_queue_named(&root, "Evening / Side A", &[track("one.wav")]).unwrap();
        assert_eq!(path.file_name().unwrap(), "Evening___Side_A.json");
        fs::remove_dir_all(root).unwrap();
    }
}
