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

pub fn sanitize_queue_name(name: &str) -> String {
    name.trim()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

pub fn queue_exists(folder: impl AsRef<Path>, name: &str) -> bool {
    let clean_name = sanitize_queue_name(name);
    if clean_name.is_empty() {
        return false;
    }
    folder.as_ref().join(format!("{clean_name}.json")).exists()
}

pub fn next_queue_version(folder: impl AsRef<Path>, name: &str) -> String {
    let clean_name = sanitize_queue_name(name);
    let base = if clean_name.is_empty() {
        "queue".to_owned()
    } else {
        clean_name
    };
    let folder = folder.as_ref();
    let mut version = 1;
    loop {
        let candidate = format!("{base}_{version}");
        if !folder.join(format!("{candidate}.json")).exists() {
            return candidate;
        }
        version += 1;
    }
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
    let clean_name = sanitize_queue_name(name);
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

pub fn list_queues(folder: impl AsRef<Path>) -> Result<Vec<PathBuf>, String> {
    let folder = folder.as_ref();
    let mut files: Vec<PathBuf> = fs::read_dir(folder)
        .map_err(|error| format!("failed to read {}: {error}", folder.display()))?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
        .collect();
    files.sort();
    Ok(files)
}

pub fn load_queue(path: impl AsRef<Path>) -> Result<Vec<PathBuf>, String> {
    let path = path.as_ref();
    let contents = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let queue: SavedQueue = serde_json::from_str(&contents)
        .map_err(|error| format!("invalid queue {}: {error}", path.display()))?;
    Ok(queue.tracks.into_iter().map(|track| track.path).collect())
}

pub fn match_tracks(tracks: &[AudioTrack], saved_paths: &[PathBuf]) -> (Vec<bool>, usize) {
    let (selected, _, missing) = match_tracks_ordered(tracks, saved_paths);
    (selected, missing)
}

pub fn match_tracks_ordered(
    tracks: &[AudioTrack],
    saved_paths: &[PathBuf],
) -> (Vec<bool>, Vec<usize>, usize) {
    let mut selected = vec![false; tracks.len()];
    let mut order = Vec::new();
    let mut missing = 0;
    for saved_path in saved_paths {
        // First try exact path match
        let found = tracks
            .iter()
            .enumerate()
            .find(|(_, track)| track.path == *saved_path)
            // If no exact match, fallback to matching by filename for relocated paths
            .or_else(|| {
                saved_path.file_name().and_then(|saved_file_name| {
                    tracks
                        .iter()
                        .enumerate()
                        .find(|(_, track)| track.path.file_name() == Some(saved_file_name))
                })
            });

        if let Some((index, _)) = found {
            selected[index] = true;
            order.push(index);
        } else {
            missing += 1;
        }
    }
    (selected, order, missing)
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
        let (selected, order, missing) = match_tracks_ordered(
            &tracks,
            &[
                PathBuf::from("tracks/two.wav"),
                PathBuf::from("tracks/lost.wav"),
            ],
        );
        assert_eq!(selected, vec![false, true]);
        assert_eq!(order, vec![1]);
        assert_eq!(missing, 1);
    }

    #[test]
    fn sanitizes_named_queue_filename() {
        let root = std::env::temp_dir().join("deckprep-queue-name-test");
        let path = save_queue_named(&root, "Evening / Side A", &[track("one.wav")]).unwrap();
        assert_eq!(path.file_name().unwrap(), "Evening___Side_A.json");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn matches_relocated_paths_by_filename() {
        let tracks = vec![track("alpha.wav"), track("beta.flac")];
        // Saved paths came from a different directory (e.g. /home/pi/Music)
        let saved_paths = vec![
            PathBuf::from("/home/pi/Music/beta.flac"),
            PathBuf::from("/media/usb/alpha.wav"),
            PathBuf::from("/nowhere/nonexistent.wav"),
        ];
        let (selected, order, missing) = match_tracks_ordered(&tracks, &saved_paths);
        assert_eq!(selected, vec![true, true]);
        assert_eq!(order, vec![1, 0]);
        assert_eq!(missing, 1);
    }

    #[test]
    fn preserves_duplicate_tracks_in_queue_order() {
        let tracks = vec![track("intro.wav"), track("body.wav")];
        let saved_paths = vec![
            PathBuf::from("tracks/intro.wav"),
            PathBuf::from("tracks/body.wav"),
            PathBuf::from("tracks/intro.wav"),
        ];
        let (selected, order, missing) = match_tracks_ordered(&tracks, &saved_paths);
        assert_eq!(selected, vec![true, true]);
        assert_eq!(order, vec![0, 1, 0]);
        assert_eq!(missing, 0);
    }

    #[test]
    fn checks_queue_exists_and_auto_versions() {
        let root = std::env::temp_dir().join("deckprep-queue-version-test");
        fs::create_dir_all(&root).unwrap();

        assert!(!queue_exists(&root, "mixtape"));
        save_queue_named(&root, "mixtape", &[track("one.wav")]).unwrap();
        assert!(queue_exists(&root, "mixtape"));

        let v1 = next_queue_version(&root, "mixtape");
        assert_eq!(v1, "mixtape_1");
        save_queue_named(&root, &v1, &[track("one.wav")]).unwrap();

        let v2 = next_queue_version(&root, "mixtape");
        assert_eq!(v2, "mixtape_2");

        fs::remove_dir_all(root).unwrap();
    }
}
