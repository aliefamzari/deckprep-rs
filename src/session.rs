use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use sdl2::{render::Canvas, video::Window};

use crate::{
    audio::AudioPlayer,
    config::{NormalizationMethod, RuntimeProfile},
    counter::TapeCounter,
    library::AudioTrack,
    meter::StereoMeter,
    normalize::{NormalizationSettings, NormalizedTrack, clean_stale_cache_files, normalize_track},
    timing::SessionPlan,
    tracklist::write_tracklist,
    ui::AppUi,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionTickResult {
    Continuing,
    Complete,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepPhase {
    Countdown {
        deadline: Instant,
    },
    LeaderGap {
        session_start: Instant,
        deadline: Instant,
    },
    RecordingTrack {
        track_index: usize,
        session_start: Instant,
    },
    TrackGap {
        next_index: usize,
        session_start: Instant,
        deadline: Instant,
    },
    Complete {
        session_start: Instant,
    },
}

#[derive(Debug, Clone)]
pub struct PrepSession {
    pub plan: SessionPlan,
    pub counter: TapeCounter,
    pub normalized: Vec<NormalizedTrack>,
    pub session_meters: Vec<StereoMeter>,
    pub track_names: Vec<String>,
    pub tracklist_path: PathBuf,
    pub total_seconds: u32,
    pub phase: PrepPhase,
}

impl PrepSession {
    pub fn start(selected: &[AudioTrack], profile: &RuntimeProfile) -> Result<Self, String> {
        Self::start_with_durations(selected, profile, Duration::from_secs(10))
    }

    pub fn start_with_durations(
        selected: &[AudioTrack],
        profile: &RuntimeProfile,
        countdown_duration: Duration,
    ) -> Result<Self, String> {
        if selected.is_empty() {
            return Err("PREP BLOCKED: SELECT TRACKS".to_owned());
        }
        let plan = SessionPlan::from_audio_tracks(profile, selected);
        if !plan.fits() {
            return Err(format!(
                "PREP BLOCKED: OVER CAPACITY BY {}S",
                -plan.remaining_seconds()
            ));
        }

        let track_names: Vec<String> = selected.iter().map(|track| track.name.clone()).collect();
        let total_seconds = plan.total_seconds();
        let counter = TapeCounter::from_profile(profile)?;
        let output_folder = Path::new(&profile.tracks_folder).join(&profile.normalized_folder);
        let _ = clean_stale_cache_files(&output_folder);

        let settings = NormalizationSettings {
            method: profile.normalization,
            target_lufs: profile.target_lufs,
        };
        let mut normalized = Vec::with_capacity(selected.len());
        for track in selected {
            normalized.push(normalize_track(track, &output_folder, settings)?);
        }
        let session_meters: Vec<StereoMeter> = normalized
            .iter()
            .map(|track| StereoMeter::analyze(&track.path))
            .collect::<Result<_, _>>()?;

        let tracklist_path = write_tracklist(
            &plan,
            &counter,
            &profile.tracks_folder,
            match profile.normalization {
                NormalizationMethod::None => "NONE",
                NormalizationMethod::Peak => "PEAK",
                NormalizationMethod::Lufs => "LUFS",
            },
            profile.target_lufs,
        )?;

        let deadline = Instant::now() + countdown_duration;
        let phase = PrepPhase::Countdown { deadline };

        Ok(Self {
            plan,
            counter,
            normalized,
            session_meters,
            track_names,
            tracklist_path,
            total_seconds,
            phase,
        })
    }

    pub fn tick(
        &mut self,
        player: &mut AudioPlayer,
        profile: &RuntimeProfile,
    ) -> Result<SessionTickResult, String> {
        match self.phase {
            PrepPhase::Countdown { deadline } => {
                if Instant::now() >= deadline {
                    let session_start = Instant::now();
                    if profile.leader_gap_seconds > 0 {
                        self.phase = PrepPhase::LeaderGap {
                            session_start,
                            deadline: session_start
                                + Duration::from_secs(u64::from(profile.leader_gap_seconds)),
                        };
                    } else if !self.normalized.is_empty() {
                        player.play(&self.normalized[0].path)?;
                        self.phase = PrepPhase::RecordingTrack {
                            track_index: 0,
                            session_start,
                        };
                    } else {
                        self.phase = PrepPhase::Complete { session_start };
                        return Ok(SessionTickResult::Complete);
                    }
                }
                Ok(SessionTickResult::Continuing)
            }
            PrepPhase::LeaderGap {
                session_start,
                deadline,
            } => {
                if Instant::now() >= deadline {
                    if !self.normalized.is_empty() {
                        player.play(&self.normalized[0].path)?;
                        self.phase = PrepPhase::RecordingTrack {
                            track_index: 0,
                            session_start,
                        };
                    } else {
                        self.phase = PrepPhase::Complete { session_start };
                        return Ok(SessionTickResult::Complete);
                    }
                }
                Ok(SessionTickResult::Continuing)
            }
            PrepPhase::RecordingTrack {
                track_index,
                session_start,
            } => {
                player.refresh()?;
                if player.current_path().is_none() {
                    if track_index + 1 < self.normalized.len() {
                        if profile.track_gap_seconds > 0 {
                            self.phase = PrepPhase::TrackGap {
                                next_index: track_index + 1,
                                session_start,
                                deadline: Instant::now()
                                    + Duration::from_secs(u64::from(profile.track_gap_seconds)),
                            };
                        } else {
                            player.play(&self.normalized[track_index + 1].path)?;
                            self.phase = PrepPhase::RecordingTrack {
                                track_index: track_index + 1,
                                session_start,
                            };
                        }
                    } else {
                        self.phase = PrepPhase::Complete { session_start };
                        return Ok(SessionTickResult::Complete);
                    }
                }
                Ok(SessionTickResult::Continuing)
            }
            PrepPhase::TrackGap {
                next_index,
                session_start,
                deadline,
            } => {
                if Instant::now() >= deadline {
                    player.play(&self.normalized[next_index].path)?;
                    self.phase = PrepPhase::RecordingTrack {
                        track_index: next_index,
                        session_start,
                    };
                }
                Ok(SessionTickResult::Continuing)
            }
            PrepPhase::Complete { .. } => Ok(SessionTickResult::Complete),
        }
    }

    pub fn cancel(&mut self, player: &mut AudioPlayer) {
        let _ = player.stop();
    }

    pub fn is_complete(&self) -> bool {
        matches!(self.phase, PrepPhase::Complete { .. })
    }

    pub fn current_track_index(&self) -> Option<usize> {
        match self.phase {
            PrepPhase::RecordingTrack { track_index, .. } => Some(track_index),
            _ => None,
        }
    }

    pub fn render(
        &self,
        canvas: &mut Canvas<Window>,
        ui: &AppUi,
        profile: &RuntimeProfile,
        player: &AudioPlayer,
    ) -> Result<(), String> {
        match self.phase {
            PrepPhase::Countdown { deadline } => {
                let remaining = deadline.saturating_duration_since(Instant::now()).as_secs() + 1;
                ui.render_recording(
                    canvas,
                    profile,
                    &format!("PREP {:02}S - Q CANCEL", remaining),
                    &self.track_names,
                    None,
                    0,
                    self.total_seconds,
                    self.counter.calculate(0.0),
                    (0, 0),
                    Duration::ZERO,
                    false,
                )
            }
            PrepPhase::LeaderGap {
                session_start,
                deadline,
            } => {
                let remaining = deadline.saturating_duration_since(Instant::now()).as_secs() + 1;
                let elapsed = session_start.elapsed();
                ui.render_recording(
                    canvas,
                    profile,
                    &format!("LEADER GAP {:02}S - Q CANCEL", remaining),
                    &self.track_names,
                    None,
                    elapsed.as_secs() as u32,
                    self.total_seconds,
                    self.counter.calculate(elapsed.as_secs_f64()),
                    (0, 0),
                    elapsed,
                    true,
                )
            }
            PrepPhase::RecordingTrack {
                track_index,
                session_start,
            } => {
                let elapsed = session_start.elapsed();
                let meter_levels = self
                    .session_meters
                    .get(track_index)
                    .map(|m| m.level_at(player.position_seconds()))
                    .unwrap_or((0, 0));
                ui.render_recording(
                    canvas,
                    profile,
                    &format!("RECORDING {}/{}", track_index + 1, self.normalized.len()),
                    &self.track_names,
                    Some(track_index),
                    elapsed.as_secs() as u32,
                    self.total_seconds,
                    self.counter.calculate(elapsed.as_secs_f64()),
                    meter_levels,
                    elapsed,
                    true,
                )
            }
            PrepPhase::TrackGap {
                next_index: _,
                session_start,
                deadline,
            } => {
                let remaining = deadline.saturating_duration_since(Instant::now()).as_secs() + 1;
                let elapsed = session_start.elapsed();
                ui.render_recording(
                    canvas,
                    profile,
                    &format!("TRACK GAP {:02}S - Q CANCEL", remaining),
                    &self.track_names,
                    None,
                    elapsed.as_secs() as u32,
                    self.total_seconds,
                    self.counter.calculate(elapsed.as_secs_f64()),
                    (0, 0),
                    elapsed,
                    true,
                )
            }
            PrepPhase::Complete { session_start } => ui.render_recording(
                canvas,
                profile,
                "PREP COMPLETE",
                &self.track_names,
                None,
                self.total_seconds,
                self.total_seconds,
                self.counter.calculate(f64::from(self.total_seconds)),
                (0, 0),
                session_start.elapsed(),
                false,
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_track(name: &str) -> AudioTrack {
        AudioTrack {
            name: name.to_owned(),
            path: PathBuf::from(format!("tracks/{name}")),
            duration_seconds: 60,
            codec: "wav".to_owned(),
        }
    }

    #[test]
    fn rejects_empty_tracks_on_start() {
        let profile = RuntimeProfile::default();
        let result = PrepSession::start(&[], &profile);
        assert_eq!(result.err(), Some("PREP BLOCKED: SELECT TRACKS".to_owned()));
    }

    #[test]
    fn rejects_over_capacity_on_start() {
        let mut profile = RuntimeProfile::default();
        profile.side_duration_minutes = 1; // 60s max
        let tracks = vec![test_track("one.wav"), test_track("two.wav")];
        let result = PrepSession::start(&tracks, &profile);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("OVER CAPACITY"));
    }

    #[test]
    fn state_machine_transitions_through_phases() {
        let now = Instant::now();
        let mut session = PrepSession {
            plan: SessionPlan {
                side_capacity_seconds: 1800,
                leader_gap_seconds: 1,
                track_gap_seconds: 1,
                tracks: vec![],
            },
            counter: TapeCounter::Linear,
            normalized: vec![
                NormalizedTrack {
                    source: test_track("one.wav"),
                    path: PathBuf::from("normalized/one.wav"),
                    cache_hit: true,
                },
                NormalizedTrack {
                    source: test_track("two.wav"),
                    path: PathBuf::from("normalized/two.wav"),
                    cache_hit: true,
                },
            ],
            session_meters: vec![],
            track_names: vec!["one.wav".to_owned(), "two.wav".to_owned()],
            tracklist_path: PathBuf::from("deck_tracklist.txt"),
            total_seconds: 130,
            phase: PrepPhase::Countdown {
                deadline: now - Duration::from_millis(1), // already expired
            },
        };

        let mut player = AudioPlayer::new("dummy", "dummy");
        let mut profile = RuntimeProfile::default();
        profile.leader_gap_seconds = 1;
        profile.track_gap_seconds = 1;

        // 1. Countdown -> LeaderGap
        let tick1 = session.tick(&mut player, &profile).unwrap();
        assert_eq!(tick1, SessionTickResult::Continuing);
        match session.phase {
            PrepPhase::LeaderGap { .. } => {}
            _ => panic!("expected LeaderGap phase, got {:?}", session.phase),
        }

        // 2. LeaderGap expiration -> RecordingTrack 0
        let session_start = match session.phase {
            PrepPhase::LeaderGap { session_start, .. } => session_start,
            _ => unreachable!(),
        };
        session.phase = PrepPhase::LeaderGap {
            session_start,
            deadline: now - Duration::from_millis(1),
        };
        let tick2 = session.tick(&mut player, &profile).unwrap();
        assert_eq!(tick2, SessionTickResult::Continuing);
        assert_eq!(session.current_track_index(), Some(0));

        // 3. Track 0 completes -> TrackGap (next_index: 1)
        player.stop().unwrap();
        let tick3 = session.tick(&mut player, &profile).unwrap();
        assert_eq!(tick3, SessionTickResult::Continuing);
        match session.phase {
            PrepPhase::TrackGap { next_index, .. } => assert_eq!(next_index, 1),
            _ => panic!("expected TrackGap phase, got {:?}", session.phase),
        }

        // 4. TrackGap expiration -> RecordingTrack 1
        session.phase = PrepPhase::TrackGap {
            next_index: 1,
            session_start,
            deadline: now - Duration::from_millis(1),
        };
        let tick4 = session.tick(&mut player, &profile).unwrap();
        assert_eq!(tick4, SessionTickResult::Continuing);
        assert_eq!(session.current_track_index(), Some(1));

        // 5. Track 1 completes (last track) -> Complete
        player.stop().unwrap();
        let tick5 = session.tick(&mut player, &profile).unwrap();
        assert_eq!(tick5, SessionTickResult::Complete);
        assert!(session.is_complete());
    }

    #[test]
    fn cancellation_stops_player() {
        let mut session = PrepSession {
            plan: SessionPlan {
                side_capacity_seconds: 1800,
                leader_gap_seconds: 1,
                track_gap_seconds: 1,
                tracks: vec![],
            },
            counter: TapeCounter::Linear,
            normalized: vec![],
            session_meters: vec![],
            track_names: vec![],
            tracklist_path: PathBuf::from("deck_tracklist.txt"),
            total_seconds: 130,
            phase: PrepPhase::Countdown {
                deadline: Instant::now() + Duration::from_secs(10),
            },
        };
        let mut player = AudioPlayer::new("dummy", "dummy");
        session.cancel(&mut player);
        assert!(!player.is_playing());
    }
}
