use std::{
    fs::{self, File},
    io::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{counter::TapeCounter, timing::SessionPlan};

pub fn write_tracklist(
    plan: &SessionPlan,
    counter: &TapeCounter,
    output_folder: impl AsRef<Path>,
    normalization: &str,
    target_lufs: f64,
) -> Result<PathBuf, String> {
    let output_folder = output_folder.as_ref();
    fs::create_dir_all(output_folder)
        .map_err(|error| format!("failed to create {}: {error}", output_folder.display()))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("system clock error: {error}"))?
        .as_secs();
    let filename = format!("deck_tracklist_{timestamp}.txt");
    let path = output_folder.join(filename);
    let temporary_path = output_folder.join(format!(".deck_tracklist_{timestamp}.tmp"));
    let timeline = plan.timeline(counter);

    let result = (|| -> Result<(), String> {
        let mut file = File::create(&temporary_path)
            .map_err(|error| format!("failed to create tracklist: {error}"))?;
        writeln!(file, "DeckPrep Tracklist")
            .and_then(|_| writeln!(file, "=================="))
            .and_then(|_| {
                writeln!(
                    file,
                    "Tape side capacity: {}",
                    format_duration(plan.side_capacity_seconds)
                )
            })
            .and_then(|_| {
                writeln!(
                    file,
                    "Leader gap: {}",
                    format_duration(plan.leader_gap_seconds)
                )
            })
            .and_then(|_| {
                writeln!(
                    file,
                    "Track gap: {}",
                    format_duration(plan.track_gap_seconds)
                )
            })
            .and_then(|_| {
                if normalization == "NONE" {
                    return writeln!(file, "Normalization: NONE (original files)");
                }
                writeln!(
                    file,
                    "Normalization: {normalization} (target {target_lufs:+.1} LUFS)"
                )
            })
            .and_then(|_| {
                writeln!(
                    file,
                    "Total recording time: {}",
                    format_duration(plan.total_seconds())
                )
            })
            .and_then(|_| {
                writeln!(
                    file,
                    "Remaining capacity: {}",
                    format_signed_duration(plan.remaining_seconds())
                )
            })
            .and_then(|_| writeln!(file))
            .and_then(|_| writeln!(file, "TRACKS"))
            .and_then(|_| writeln!(file, "------"))
            .map_err(|error| format!("failed to write tracklist: {error}"))?;

        for (index, timing) in timeline.iter().enumerate() {
            writeln!(
                file,
                "{:02}. {}\n    Start: {}  End: {}  Duration: {}\n    Counter: {:04} - {:04}",
                index + 1,
                timing.name,
                format_duration(timing.start_seconds),
                format_duration(timing.end_seconds),
                format_duration(timing.end_seconds - timing.start_seconds),
                timing.counter_start,
                timing.counter_end,
            )
            .map_err(|error| format!("failed to write tracklist: {error}"))?;
        }
        file.sync_all()
            .map_err(|error| format!("failed to flush tracklist: {error}"))?;
        fs::rename(&temporary_path, &path)
            .map_err(|error| format!("failed to finalize tracklist: {error}"))?;
        Ok(())
    })();

    if result.is_err() {
        let _ = fs::remove_file(&temporary_path);
    }
    result.map(|()| path)
}

fn format_duration(seconds: u32) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

fn format_signed_duration(seconds: i64) -> String {
    let sign = if seconds < 0 { "-" } else { "+" };
    let absolute = seconds.unsigned_abs() as u32;
    format!("{sign}{}", format_duration(absolute))
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use super::*;
    use crate::timing::Track;

    #[test]
    fn writes_timed_tracklist_atomically() {
        let root = std::env::temp_dir().join(format!(
            "deckprep-tracklist-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let plan = SessionPlan {
            side_capacity_seconds: 1_800,
            leader_gap_seconds: 10,
            track_gap_seconds: 5,
            tracks: vec![Track {
                name: "Opening.wav".to_owned(),
                duration_seconds: 180,
            }],
        };
        let path = write_tracklist(&plan, &TapeCounter::Linear, &root, "LUFS", -14.0).unwrap();
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("Opening.wav"));
        assert!(content.contains("Start: 00:10"));
        assert!(content.contains("End: 03:10"));
        assert!(content.contains("Counter: 0010 - 0190"));
        assert!(!root.join(".deck_tracklist.tmp").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
