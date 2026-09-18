use crate::counter::TapeCounter;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    pub name: String,
    pub duration_seconds: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackTiming {
    pub name: String,
    pub start_seconds: u32,
    pub end_seconds: u32,
    pub counter_start: u32,
    pub counter_end: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionPlan {
    pub side_capacity_seconds: u32,
    pub leader_gap_seconds: u32,
    pub track_gap_seconds: u32,
    pub tracks: Vec<Track>,
}

impl SessionPlan {
    pub fn total_seconds(&self) -> u32 {
        let track_time: u32 = self.tracks.iter().map(|track| track.duration_seconds).sum();
        let gap_count = self.tracks.len().saturating_sub(1) as u32;
        self.leader_gap_seconds + track_time + self.track_gap_seconds * gap_count
    }

    pub fn remaining_seconds(&self) -> i64 {
        i64::from(self.side_capacity_seconds) - i64::from(self.total_seconds())
    }

    pub fn fits(&self) -> bool {
        self.total_seconds() <= self.side_capacity_seconds
    }

    pub fn timeline(&self, counter: &TapeCounter) -> Vec<TrackTiming> {
        let mut cursor = self.leader_gap_seconds;
        self.tracks
            .iter()
            .map(|track| {
                let start = cursor;
                let end = start + track.duration_seconds;
                cursor = end + self.track_gap_seconds;
                TrackTiming {
                    name: track.name.clone(),
                    start_seconds: start,
                    end_seconds: end,
                    counter_start: counter.calculate(f64::from(start)),
                    counter_end: counter.calculate(f64::from(end)),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plan() -> SessionPlan {
        SessionPlan {
            side_capacity_seconds: 1_800,
            leader_gap_seconds: 10,
            track_gap_seconds: 5,
            tracks: vec![
                Track {
                    name: "Opening.wav".to_owned(),
                    duration_seconds: 180,
                },
                Track {
                    name: "Finale.flac".to_owned(),
                    duration_seconds: 240,
                },
            ],
        }
    }

    #[test]
    fn total_includes_one_gap_between_two_tracks() {
        assert_eq!(plan().total_seconds(), 435);
        assert_eq!(plan().remaining_seconds(), 1_365);
        assert!(plan().fits());
    }

    #[test]
    fn timeline_starts_after_leader_and_uses_counter() {
        let timeline = plan().timeline(&TapeCounter::Static { rate: 1.5 });
        assert_eq!(timeline[0].start_seconds, 10);
        assert_eq!(timeline[0].counter_start, 15);
        assert_eq!(timeline[1].start_seconds, 195);
        assert_eq!(timeline[1].counter_end, 652);
    }

    #[test]
    fn detects_capacity_overrun() {
        let mut session = plan();
        session.side_capacity_seconds = 400;
        assert!(!session.fits());
        assert_eq!(session.remaining_seconds(), -35);
    }
}
