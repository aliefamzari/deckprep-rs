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
