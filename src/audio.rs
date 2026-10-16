use std::{
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Instant,
};

pub struct AudioPlayer {
    child: Option<Child>,
    current_path: Option<PathBuf>,
    audio_backend: String,
    audio_device: String,
    position_seconds: f64,
    started_at: Option<Instant>,
    paused: bool,
}

impl AudioPlayer {
    pub fn new(audio_backend: impl Into<String>, audio_device: impl Into<String>) -> Self {
        Self {
            child: None,
            current_path: None,
            audio_backend: audio_backend.into(),
            audio_device: audio_device.into(),
            position_seconds: 0.0,
            started_at: None,
            paused: false,
        }
    }

    pub fn toggle(&mut self, path: &Path) -> Result<(), String> {
        self.refresh()?;
        if self.current_path.as_deref() == Some(path) {
            if self.paused {
                return self.resume();
            }
            return self.pause();
        }
        self.play(path)
    }

    pub fn play(&mut self, path: &Path) -> Result<(), String> {
        self.play_at(path, 0.0)
    }

    fn play_at(&mut self, path: &Path, position_seconds: f64) -> Result<(), String> {
        self.stop()?;
        let mut command = Command::new("ffplay");
        command.args(["-nodisp", "-autoexit", "-loglevel", "error"]);
        if position_seconds > 0.0 {
            command.args(["-ss", &format!("{position_seconds:.3}")]);
        }
        command
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        if self.audio_backend == "alsa" {
            command.env("SDL_AUDIODRIVER", "alsa");
            if self.audio_device != "default" {
                command.env("AUDIODEV", &self.audio_device);
            }
        }
        self.child = Some(
            command
                .spawn()
                .map_err(|error| format!("failed to start ffplay: {error}"))?,
        );
        self.current_path = Some(path.to_path_buf());
        self.position_seconds = position_seconds;
        self.started_at = Some(Instant::now());
        self.paused = false;
        Ok(())
    }

    pub fn pause(&mut self) -> Result<(), String> {
        self.refresh()?;
        if self.paused || self.child.is_none() {
            return Ok(());
        }
        self.update_position();
        self.kill_child()?;
        self.started_at = None;
        self.paused = true;
        Ok(())
    }

    pub fn resume(&mut self) -> Result<(), String> {
        let Some(path) = self.current_path.clone() else {
            return Ok(());
        };
        if !self.paused {
            return Ok(());
        }
        self.play_at(&path, self.position_seconds)
    }

    pub fn seek_by(&mut self, seconds: f64, duration_seconds: u32) -> Result<(), String> {
        self.refresh()?;
        if self.current_path.is_none() {
            return Ok(());
        }
        self.update_position();
        self.position_seconds = (self.position_seconds + seconds)
            .max(0.0)
            .min(f64::from(duration_seconds));
        let path = self.current_path.clone().expect("checked above");
        if self.paused {
            return Ok(());
        }
        self.play_at(&path, self.position_seconds)
    }

    pub fn stop(&mut self) -> Result<(), String> {
        self.kill_child()?;
        self.current_path = None;
        self.position_seconds = 0.0;
        self.started_at = None;
        self.paused = false;
        Ok(())
    }

    fn kill_child(&mut self) -> Result<(), String> {
        if let Some(mut child) = self.child.take()
            && child
                .try_wait()
                .map_err(|error| format!("failed to query ffplay: {error}"))?
                .is_none()
        {
            child
                .kill()
                .map_err(|error| format!("failed to stop ffplay: {error}"))?;
            child
                .wait()
                .map_err(|error| format!("failed to reap ffplay: {error}"))?;
        }
        Ok(())
    }

    fn update_position(&mut self) {
        if let Some(started_at) = self.started_at {
            self.position_seconds += started_at.elapsed().as_secs_f64();
            self.started_at = Some(Instant::now());
        }
    }

    pub fn refresh(&mut self) -> Result<(), String> {
        if let Some(child) = self.child.as_mut()
            && child
                .try_wait()
                .map_err(|error| format!("failed to query ffplay: {error}"))?
                .is_some()
        {
            self.child = None;
            self.current_path = None;
            self.started_at = None;
            self.position_seconds = 0.0;
            self.paused = false;
        }
        Ok(())
    }

    pub fn is_playing(&self) -> bool {
        self.child.is_some()
    }

    pub fn is_paused(&self) -> bool {
        self.paused
    }

    pub fn current_path(&self) -> Option<&Path> {
        self.current_path.as_deref()
    }

    pub fn position_seconds(&self) -> f64 {
        self.position_seconds
            + self
                .started_at
                .map(|started| started.elapsed().as_secs_f64())
                .unwrap_or(0.0)
    }
}

impl Drop for AudioPlayer {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDiagnostics {
    pub backend: String,
    pub device: String,
    pub ffplay_available: bool,
    pub ffplay_version: Option<String>,
    pub ffmpeg_available: bool,
    pub ffmpeg_version: Option<String>,
    pub alsa_cards: Vec<String>,
    pub recommended_devices: Vec<String>,
}

impl AudioDiagnostics {
    pub fn report(&self) -> String {
        let mut lines = Vec::new();
        lines.push("=== DECKPREP AUDIO HARDWARE DIAGNOSTICS ===".to_owned());
        lines.push(format!("Configured Audio Backend: {}", self.backend));
        lines.push(format!("Configured Audio Device:  {}", self.device));
        lines.push(format!(
            "FFplay binary:            {}",
            if self.ffplay_available {
                self.ffplay_version.as_deref().unwrap_or("Available")
            } else {
                "NOT FOUND (install ffmpeg/ffplay)"
            }
        ));
        lines.push(format!(
            "FFmpeg binary:            {}",
            if self.ffmpeg_available {
                self.ffmpeg_version.as_deref().unwrap_or("Available")
            } else {
                "NOT FOUND (install ffmpeg)"
            }
        ));

        if !self.alsa_cards.is_empty() {
            lines.push(String::new());
            lines.push("Detected ALSA Sound Cards:".to_owned());
            for card in &self.alsa_cards {
                lines.push(format!("  • {card}"));
            }
        }

        if !self.recommended_devices.is_empty() {
            lines.push(String::new());
            lines.push("Recommended ALSA Device Strings for Profile:".to_owned());
            for rec in &self.recommended_devices {
                lines.push(format!("  • {rec}"));
            }
        } else if self.backend == "alsa" {
            lines.push(String::new());
            lines.push(
                "Notice: For iBasso D-Zero on Raspberry Pi, common ALSA devices are:".to_owned(),
            );
            lines.push(
                "  • plughw:CARD=DZero,DEV=0 (recommended with auto format conversion)".to_owned(),
            );
            lines.push("  • hw:CARD=DZero,DEV=0     (direct hardware bit-exact access)".to_owned());
        }

        lines.join("\n")
    }
}

pub fn parse_alsa_cards(contents: &str) -> (Vec<String>, Vec<String>) {
    let mut cards = Vec::new();
    let mut recommended = Vec::new();
    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some((idx_part, rest)) = trimmed.split_once('[') {
            if let Some((card_id, desc)) = rest.split_once(']') {
                let card_id = card_id.trim();
                let desc = desc.trim_start_matches(':').trim();
                let full = format!("Card {} [{}]: {}", idx_part.trim(), card_id, desc);
                cards.push(full);
                if card_id.eq_ignore_ascii_case("dzero")
                    || desc.to_ascii_lowercase().contains("ibasso")
                    || desc.to_ascii_lowercase().contains("usb")
                {
                    recommended.push(format!("plughw:CARD={card_id},DEV=0 (Plug ALSA wrapper)"));
                    recommended.push(format!("hw:CARD={card_id},DEV=0 (Direct hardware)"));
                }
            }
        }
    }
    (cards, recommended)
}

pub fn diagnose_audio(backend: &str, device: &str) -> AudioDiagnostics {
    let ffplay_cmd = Command::new("ffplay").arg("-version").output().ok();
    let ffplay_available = ffplay_cmd
        .as_ref()
        .map_or(false, |out| out.status.success());
    let ffplay_version = ffplay_cmd.and_then(|out| {
        String::from_utf8(out.stdout)
            .ok()
            .and_then(|s| s.lines().next().map(|l| l.to_owned()))
    });

    let ffmpeg_cmd = Command::new("ffmpeg").arg("-version").output().ok();
    let ffmpeg_available = ffmpeg_cmd
        .as_ref()
        .map_or(false, |out| out.status.success());
    let ffmpeg_version = ffmpeg_cmd.and_then(|out| {
        String::from_utf8(out.stdout)
            .ok()
            .and_then(|s| s.lines().next().map(|l| l.to_owned()))
    });

    let (alsa_cards, recommended_devices) = std::fs::read_to_string("/proc/asound/cards")
        .map(|contents| parse_alsa_cards(&contents))
        .unwrap_or_default();

    AudioDiagnostics {
        backend: backend.to_owned(),
        device: device.to_owned(),
        ffplay_available,
        ffplay_version,
        ffmpeg_available,
        ffmpeg_version,
        alsa_cards,
        recommended_devices,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_initial_state() {
        let player = AudioPlayer::new("sdl", "default");
        assert!(!player.is_playing());
        assert!(!player.is_paused());
        assert_eq!(player.current_path(), None);
        assert_eq!(player.position_seconds(), 0.0);
    }

    #[test]
    fn stopping_paused_preview_clears_resumable_state() {
        let mut player = AudioPlayer::new("sdl", "default");
        player.current_path = Some(PathBuf::from("review-preview.flac"));
        player.position_seconds = 12.0;
        player.paused = true;
        player.stop().unwrap();
        assert!(!player.is_playing());
        assert!(!player.is_paused());
        assert_eq!(player.current_path(), None);
        assert_eq!(player.position_seconds(), 0.0);
        player.resume().unwrap();
        assert!(!player.is_playing());
    }

    #[test]
    fn parse_alsa_proc_cards_detects_ibasso_dzero() {
        let mock_proc = " 0 [bcm2835_hdmi  ]: bcm2835_hdmi - bcm2835 HDMI 1\n 1 [DZero          ]: USB-Audio - iBasso D-Zero\n";
        let (cards, recs) = parse_alsa_cards(mock_proc);
        assert_eq!(cards.len(), 2);
        assert!(cards[0].contains("bcm2835_hdmi"));
        assert!(cards[1].contains("iBasso D-Zero"));
        assert!(recs.iter().any(|r| r.contains("plughw:CARD=DZero,DEV=0")));
        assert!(recs.iter().any(|r| r.contains("hw:CARD=DZero,DEV=0")));
    }

    #[test]
    fn audio_diagnostics_report_formats() {
        let diag = diagnose_audio("alsa", "plughw:CARD=DZero,DEV=0");
        let report = diag.report();
        assert!(report.contains("=== DECKPREP AUDIO HARDWARE DIAGNOSTICS ==="));
        assert!(report.contains("Configured Audio Backend: alsa"));
        assert!(report.contains("Configured Audio Device:  plughw:CARD=DZero,DEV=0"));
    }
}
