use std::{fs, path::Path};

use serde::{Deserialize, Serialize};

use crate::config::{CounterMode, RuntimeProfile};

const TAPE_THICKNESS_MM: f64 = 0.016;
const HUB_RADIUS_MM: f64 = 10.0;
const TAPE_SPEED_MM_PER_SECOND: f64 = 47.625;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct CalibrationPoint {
    pub time_seconds: f64,
    pub counter: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "lowercase")]
pub enum TapeCounter {
    Static {
        rate: f64,
    },
    Linear,
    Manual {
        checkpoints: Vec<CalibrationPoint>,
        fallback_rate: f64,
    },
    Auto {
        rate: f64,
        side_duration_seconds: f64,
    },
}

impl TapeCounter {
    pub fn from_profile(profile: &RuntimeProfile) -> Result<Self, String> {
        match profile.counter_mode {
            CounterMode::Linear => Ok(Self::Linear),
            CounterMode::Static => Ok(Self::Static {
                rate: profile.counter_rate,
            }),
            CounterMode::Auto => Ok(Self::Auto {
                rate: profile.counter_rate,
                side_duration_seconds: f64::from(profile.side_duration_minutes * 60),
            }),
            CounterMode::Manual => {
                let path = profile
                    .counter_config
                    .as_deref()
                    .ok_or_else(|| "manual counter mode requires counter_config".to_owned())?;
                load_manual(path, profile.counter_rate)
            }
        }
    }

    pub fn calculate(&self, elapsed_seconds: f64) -> u32 {
        let elapsed = elapsed_seconds.max(0.0);
        let value = match self {
            Self::Static { rate } => elapsed * rate,
            Self::Linear => elapsed,
            Self::Manual {
                checkpoints,
                fallback_rate,
            } => calculate_manual(elapsed, checkpoints, *fallback_rate),
            Self::Auto {
                rate,
                side_duration_seconds,
            } => calculate_auto(elapsed, *rate, *side_duration_seconds),
        };
        value.max(0.0) as u32
    }
}

#[derive(Debug, Deserialize)]
struct CalibrationFile {
    checkpoints: Vec<CalibrationPoint>,
}

fn load_manual(path: impl AsRef<Path>, fallback_rate: f64) -> Result<TapeCounter, String> {
    let path = path.as_ref();
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("failed to read calibration {}: {error}", path.display()))?;
    let calibration: CalibrationFile = serde_json::from_str(&contents)
        .map_err(|error| format!("invalid calibration {}: {error}", path.display()))?;
    if calibration.checkpoints.is_empty() {
        return Err(format!("calibration {} has no checkpoints", path.display()));
    }
    Ok(TapeCounter::Manual {
        checkpoints: calibration.checkpoints,
        fallback_rate,
    })
}

fn calculate_manual(
    elapsed_seconds: f64,
    checkpoints: &[CalibrationPoint],
    fallback_rate: f64,
) -> f64 {
    let mut points = checkpoints.to_vec();
    points.sort_by(|left, right| left.time_seconds.total_cmp(&right.time_seconds));

    let Some(first) = points.first() else {
        return elapsed_seconds * fallback_rate;
    };
    if elapsed_seconds <= first.time_seconds {
        let rate = if first.time_seconds > 0.0 {
            first.counter / first.time_seconds
        } else {
            fallback_rate
        };
        return elapsed_seconds * rate;
    }

    if let Some([left, right]) = points
        .windows(2)
        .find(|pair| elapsed_seconds <= pair[1].time_seconds)
    {
        let span = right.time_seconds - left.time_seconds;
        if span <= 0.0 {
            return right.counter;
        }
        let factor = (elapsed_seconds - left.time_seconds) / span;
        return left.counter + (right.counter - left.counter) * factor;
    }

    if points.len() == 1 {
        return elapsed_seconds * first.counter / first.time_seconds;
    }
    let left = points[points.len() - 2];
    let right = points[points.len() - 1];
    let span = right.time_seconds - left.time_seconds;
    if span <= 0.0 {
        return right.counter;
    }
    right.counter + ((right.counter - left.counter) / span) * (elapsed_seconds - right.time_seconds)
}

fn calculate_auto(elapsed_seconds: f64, rate: f64, side_duration_seconds: f64) -> f64 {
    let tape_length = side_duration_seconds * TAPE_SPEED_MM_PER_SECOND;
    let consumed = (elapsed_seconds * TAPE_SPEED_MM_PER_SECOND).min(tape_length);
    let takeup_radius =
        (HUB_RADIUS_MM.powi(2) + consumed * TAPE_THICKNESS_MM / std::f64::consts::PI).sqrt();
    let midpoint_radius = (HUB_RADIUS_MM.powi(2)
        + (tape_length / 2.0) * TAPE_THICKNESS_MM / std::f64::consts::PI)
        .sqrt();
    elapsed_seconds * rate * midpoint_radius / takeup_radius
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_and_linear_match_python_behavior() {
        assert_eq!(TapeCounter::Static { rate: 1.408 }.calculate(120.0), 168);
        assert_eq!(TapeCounter::Linear.calculate(125.9), 125);
    }

    #[test]
    fn manual_interpolates_and_extrapolates() {
        let counter = TapeCounter::Manual {
            checkpoints: vec![
                CalibrationPoint {
                    time_seconds: 60.0,
                    counter: 85.0,
                },
                CalibrationPoint {
                    time_seconds: 300.0,
                    counter: 422.0,
                },
            ],
            fallback_rate: 1.0,
        };
        assert_eq!(counter.calculate(30.0), 42);
        assert_eq!(counter.calculate(180.0), 253);
        assert_eq!(counter.calculate(420.0), 590);
    }

    #[test]
    fn auto_rate_is_normalized_at_tape_midpoint() {
        let counter = TapeCounter::Auto {
            rate: 1.4,
            side_duration_seconds: 1_800.0,
        };
        assert_eq!(counter.calculate(900.0), 1_260);
    }

    #[test]
    fn manual_profile_loads_existing_checkpoint_shape() {
        let path = std::env::temp_dir().join("deckprep-counter-test.json");
        std::fs::write(
            &path,
            r#"{"checkpoints":[{"time_seconds":60,"counter":85}]}"#,
        )
        .unwrap();
        let counter = load_manual(&path, 1.0).unwrap();
        assert_eq!(counter.calculate(30.0), 42);
        std::fs::remove_file(path).unwrap();
    }
}
