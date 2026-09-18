use std::{
    fs,
    io::{self, Write},
    path::Path,
};

use crate::config::{CounterMode, DisplayBackend, NormalizationMethod, RuntimeProfile};

pub fn run(output: impl AsRef<Path>) -> Result<(), String> {
    let output = output.as_ref();
    println!("DeckPrep profile wizard");
    println!("Press Enter to keep the value shown in brackets.\n");

    let name = prompt("Profile name", "my-deck")?;
    let tracks_folder = prompt("Tracks folder", "tracks")?;
    let audio_device = prompt("Audio device", "default")?;
    let side_duration_minutes = prompt_u32("Tape side duration (minutes)", 30)?;
    let leader_gap_seconds = prompt_u32("Leader gap (seconds)", 10)?;
    let track_gap_seconds = prompt_u32("Track gap (seconds)", 5)?;
    let normalization = match prompt("Normalization (lufs/peak)", "lufs")?
        .to_ascii_lowercase()
        .as_str()
    {
        "peak" => NormalizationMethod::Peak,
        "lufs" => NormalizationMethod::Lufs,
        value => return Err(format!("unsupported normalization method: {value}")),
    };
    let target_lufs = prompt_f64("Target LUFS", -14.0)?;
    let counter_mode = match prompt("Counter mode (linear/static/auto/manual)", "linear")?
        .to_ascii_lowercase()
        .as_str()
    {
        "linear" => CounterMode::Linear,
        "static" => CounterMode::Static,
        "auto" => CounterMode::Auto,
        "manual" => CounterMode::Manual,
        value => return Err(format!("unsupported counter mode: {value}")),
    };
    let counter_rate = prompt_f64("Counter rate", 1.0)?;
    let counter_config = if counter_mode == CounterMode::Manual {
        Some(prompt(
            "Calibration JSON path",
            "profiles/counter_calibration.json",
        )?)
    } else {
        None
    };

    let profile = RuntimeProfile {
        name,
        display_backend: DisplayBackend::DesktopWindow,
        logical_width: 320,
        logical_height: 240,
        window_scale: 3,
        fullscreen: false,
        keyboard_enabled: true,
        audio_backend: "alsa".to_owned(),
        audio_device,
        tracks_folder,
        normalization,
        target_lufs,
        normalized_folder: "normalized".to_owned(),
        side_duration_minutes,
        leader_gap_seconds,
        track_gap_seconds,
        counter_mode,
        counter_rate,
        counter_config,
    };
    profile.validate()?;
    let json = serde_json::to_string_pretty(&profile)
        .map_err(|error| format!("failed to serialize profile: {error}"))?;
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    fs::write(output, format!("{json}\n"))
        .map_err(|error| format!("failed to write {}: {error}", output.display()))?;
    println!("\nProfile saved to {}", output.display());
    Ok(())
}

fn prompt(label: &str, default: &str) -> Result<String, String> {
    print!("{label} [{default}]: ");
    io::stdout().flush().map_err(|error| error.to_string())?;
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .map_err(|error| error.to_string())?;
    let value = input.trim();
    Ok(if value.is_empty() {
        default.to_owned()
    } else {
        value.to_owned()
    })
}

fn prompt_u32(label: &str, default: u32) -> Result<u32, String> {
    prompt(label, &default.to_string())?
        .parse()
        .map_err(|_| format!("{label} must be a number"))
}

fn prompt_f64(label: &str, default: f64) -> Result<f64, String> {
    prompt(label, &default.to_string())?
        .parse()
        .map_err(|_| format!("{label} must be a number"))
}
