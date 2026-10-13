use std::{
    path::Path,
    thread,
    time::{Duration, Instant},
};

use deckprep_rs::audio::AudioPlayer;
use deckprep_rs::library::scan_tracks;
use deckprep_rs::meter::StereoMeter;
use deckprep_rs::queue::{list_queues, load_queue, save_queue_named};
use deckprep_rs::ui::{
    AppUi, LegendContext, NormalizationReview, ProfileEditor, QueueBrowser, QueueNameEditor,
    render_legend,
};
use deckprep_rs::{
    config::{NormalizationMethod, RuntimeProfile},
    counter::TapeCounter,
    normalize::{NormalizationSettings, normalize_track},
    timing::SessionPlan,
    tracklist::write_tracklist,
};
use sdl2::{
    EventPump,
    event::Event,
    keyboard::{Keycode, Mod},
    render::Canvas,
    video::Window,
};

fn main() -> Result<(), String> {
    let profile_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "profiles/desktop-dev.json".to_owned());
    if profile_path == "--create-profile" {
        let output = std::env::args().nth(2).ok_or_else(|| {
            "usage: cargo run -- --create-profile profiles/my-deck.json".to_owned()
        })?;
        return deckprep_rs::profile_wizard::run(output);
    }
    let mut profile = RuntimeProfile::load(&profile_path)?;
    let (tracks, library_error) = match scan_tracks(&profile.tracks_folder) {
        Ok(tracks) => (tracks, None),
        Err(error) => (Vec::new(), Some(error)),
    };

    let sdl = sdl2::init()?;
    let video = sdl.video()?;
    let mut window_builder = video.window(
        &format!("DeckPrep - {}", profile.name),
        profile.logical_width * profile.window_scale,
        profile.logical_height * profile.window_scale,
    );
    window_builder.position_centered().resizable();
    if profile.fullscreen {
        window_builder.fullscreen_desktop();
    }
    let window = window_builder.build().map_err(|error| error.to_string())?;
    let mut canvas = window
        .into_canvas()
        .present_vsync()
        .build()
        .map_err(|error| error.to_string())?;
    canvas
        .set_logical_size(profile.logical_width, profile.logical_height)
        .map_err(|error| error.to_string())?;
    canvas
        .set_integer_scale(true)
        .map_err(|error| error.to_string())?;

    let mut events = sdl.event_pump()?;
    video.text_input().start();
    let mut ui = AppUi::new(tracks, library_error);
    let mut player = AudioPlayer::new(&profile.audio_backend, &profile.audio_device);
    let mut preview_meter = None;
    let mut editor: Option<ProfileEditor> = None;
    let mut queue_editor: Option<QueueNameEditor> = None;
    let mut queue_browser: Option<QueueBrowser> = None;
    let mut review: Option<NormalizationReview> = None;
    let mut legend: Option<LegendContext> = None;

    'running: loop {
        let mut start_prep = false;
        let mut start_reviewed_prep = false;
        for event in events.poll_iter() {
            match event {
                Event::TextInput { ref text, .. } => {
                    if let Some(active_editor) = editor.as_mut() {
                        active_editor.handle_text_input(text);
                    } else if let Some(active_queue_editor) = queue_editor.as_mut() {
                        active_queue_editor.handle_text_input(text);
                    }
                }
                Event::Quit { .. } => break 'running,
                Event::KeyDown {
                    keycode: Some(Keycode::Escape | Keycode::Q),
                    ..
                } => {
                    if legend.is_some() {
                        legend = None;
                    } else if editor.is_some() {
                        editor = None;
                    } else if queue_editor.is_some() {
                        queue_editor = None;
                    } else if queue_browser.is_some() {
                        queue_browser = None;
                    } else if review.is_some() {
                        review = None;
                    } else if ui.current_status().is_some() {
                        ui.clear_normalization_status();
                    } else {
                        break 'running;
                    }
                }
                Event::KeyDown {
                    keycode: Some(key),
                    keymod,
                    repeat: false,
                    ..
                } => {
                    let help_key =
                        key == Keycode::Slash && keymod.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD);
                    if help_key {
                        if legend.is_some() {
                            legend = None;
                        } else {
                            legend = Some(if editor.is_some() {
                                LegendContext::ProfileEditor
                            } else if review.is_some() {
                                LegendContext::Review
                            } else {
                                LegendContext::Main
                            });
                        }
                        continue;
                    }
                    if legend.is_some() {
                        continue;
                    }
                    if let Some(browser) = queue_browser.as_mut() {
                        if let Some(accept) = browser.handle_key(key) {
                            if accept {
                                if let Some(path) = browser.selected_path().map(Path::to_path_buf) {
                                    match load_queue(&path) {
                                        Ok(saved_paths) => {
                                            let missing = ui.load_selection(&saved_paths);
                                            let selected = ui.selected_tracks();
                                            let plan =
                                                SessionPlan::from_audio_tracks(&profile, &selected);
                                            if !plan.fits() {
                                                ui.set_normalization_status(format!(
                                                    "QUEUE LOADED (OVER CAPACITY BY {}S)",
                                                    -plan.remaining_seconds()
                                                ));
                                            } else {
                                                ui.set_normalization_status(format!(
                                                    "QUEUE LOADED {}{}",
                                                    saved_paths.len().saturating_sub(missing),
                                                    if missing > 0 {
                                                        format!(" MISSING {missing}")
                                                    } else {
                                                        String::new()
                                                    }
                                                ));
                                            }
                                        }
                                        Err(error) => ui.set_normalization_status(error),
                                    }
                                }
                            }
                            queue_browser = None;
                        }
                        continue;
                    }
                    if let Some(active_queue_editor) = queue_editor.as_mut() {
                        if let Some(save) = active_queue_editor.handle_key(key) {
                            if save {
                                let selected = ui.selected_tracks();
                                match save_queue_named(
                                    "queues",
                                    &active_queue_editor.name,
                                    &selected,
                                ) {
                                    Ok(path) => ui.set_normalization_status(format!(
                                        "QUEUE SAVED {}",
                                        path.display()
                                    )),
                                    Err(error) => ui.set_normalization_status(error),
                                }
                            }
                            queue_editor = None;
                        }
                        continue;
                    }
                    if review.is_some() {
                        match key {
                            Keycode::Up
                            | Keycode::Down
                            | Keycode::K
                            | Keycode::J
                            | Keycode::PageUp
                            | Keycode::PageDown
                            | Keycode::Home
                            | Keycode::End => {
                                if let Some(active_review) = review.as_mut() {
                                    active_review.handle_key(key);
                                }
                            }
                            Keycode::P => {
                                let selected = ui.selected_tracks();
                                let review_index = review
                                    .as_ref()
                                    .map(NormalizationReview::current_index)
                                    .unwrap_or(0);
                                if let Some(track) = selected.get(review_index) {
                                    let path = normalized_playback_path(track, &profile)?;
                                    if player.current_path() == Some(path.as_path()) {
                                        player.toggle(&path)?;
                                    } else {
                                        player.play(&path)?;
                                    }
                                    preview_meter = Some(StereoMeter::analyze(&path)?);
                                    if let Some(active_review) = review.as_mut() {
                                        active_review.preview_status = "PLAY".to_owned();
                                    }
                                }
                            }
                            Keycode::X => {
                                player.stop()?;
                                preview_meter = None;
                                if let Some(active_review) = review.as_mut() {
                                    active_review.preview_status = "STOP".to_owned();
                                }
                            }
                            Keycode::Left | Keycode::H => {
                                if let Some(track) = ui.current_track() {
                                    player.seek_by(-10.0, track.duration_seconds)?;
                                }
                            }
                            Keycode::Right | Keycode::L => {
                                if let Some(track) = ui.current_track() {
                                    player.seek_by(10.0, track.duration_seconds)?;
                                }
                            }
                            Keycode::Return => {
                                review = None;
                                start_reviewed_prep = true;
                            }
                            Keycode::Escape | Keycode::Q => review = None,
                            _ => {}
                        }
                        continue;
                    }
                    if let Some(active_editor) = editor.as_mut() {
                        if key == Keycode::S && keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD) {
                            active_editor.profile.validate()?;
                            profile = active_editor.profile.clone();
                            let path = profile.save_snapshot("profiles")?;
                            ui.set_normalization_status(format!(
                                "PROFILE SAVED {}",
                                path.file_name()
                                    .and_then(|name| name.to_str())
                                    .unwrap_or("PROFILE.JSON")
                            ));
                            editor = None;
                        } else if active_editor.handle_key(key) {
                            editor = None;
                        }
                        continue;
                    }
                    match key {
                        Keycode::W => editor = Some(ProfileEditor::new(&profile)),
                        Keycode::S if keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD) => {
                            if ui.selected_tracks().is_empty() {
                                ui.set_normalization_status("QUEUE EMPTY");
                            } else {
                                queue_editor = Some(QueueNameEditor::new());
                            }
                        }
                        Keycode::O if keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD) => {
                            match list_queues("queues") {
                                Ok(paths) if paths.is_empty() => {
                                    ui.set_normalization_status("NO SAVED QUEUES")
                                }
                                Ok(paths) => queue_browser = Some(QueueBrowser::new(paths)),
                                Err(error) => ui.set_normalization_status(error),
                            }
                        }
                        Keycode::P => {
                            if let Some(track) = ui.current_track().cloned() {
                                ui.set_selected_panel(false);
                                let path = normalized_playback_path(&track, &profile)?;
                                if player.current_path() == Some(path.as_path()) {
                                    player.toggle(&path)?;
                                } else {
                                    player.play(&path)?;
                                }
                                preview_meter = Some(StereoMeter::analyze(&path)?);
                                ui.set_normalization_status("NORMALIZED PREVIEW");
                            }
                        }
                        Keycode::X => {
                            player.stop()?;
                            preview_meter = None;
                        }
                        Keycode::Left | Keycode::H => {
                            if let Some(track) = ui.current_track() {
                                player.seek_by(-10.0, track.duration_seconds)?;
                            }
                        }
                        Keycode::Right | Keycode::L => {
                            if let Some(track) = ui.current_track() {
                                player.seek_by(10.0, track.duration_seconds)?;
                            }
                        }
                        Keycode::N => normalize_selected(&mut ui, &profile)?,
                        Keycode::S => {
                            let path = profile.save_snapshot("profiles")?;
                            ui.set_normalization_status(format!(
                                "PROFILE SAVED {}",
                                path.file_name()
                                    .and_then(|name| name.to_str())
                                    .unwrap_or("PROFILE.JSON")
                            ));
                        }
                        Keycode::C if keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD) => {}
                        _ => {
                            ui.handle_key(key, &profile);
                            start_prep = ui.take_prep_request();
                        }
                    }
                }
                _ => {}
            }
        }

        if start_reviewed_prep {
            run_prep_session(&mut canvas, &mut events, &mut ui, &mut player, &profile)?;
        } else if start_prep {
            if review.is_none() {
                review = build_normalization_review(&mut ui, &profile)?;
            } else {
                run_prep_session(&mut canvas, &mut events, &mut ui, &mut player, &profile)?;
            }
        }

        player.refresh()?;
        if player.current_path().is_none() {
            preview_meter = None;
        }
        ui.set_previewing(player.current_path().is_some(), player.is_paused());
        let meter_levels = preview_meter
            .as_ref()
            .map(|meter| meter.level_at(player.position_seconds()));
        if let Some(browser) = queue_browser.as_ref() {
            browser.render(&mut canvas)?;
        } else if let Some(active_queue_editor) = queue_editor.as_ref() {
            active_queue_editor.render(&mut canvas)?;
        } else if let Some(active_editor) = editor.as_ref() {
            active_editor.render(&mut canvas)?;
        } else if let Some(active_review) = review.as_ref() {
            active_review.render(&mut canvas, &profile, meter_levels)?;
        } else {
            ui.render(&mut canvas, &profile, meter_levels)?;
        }
        if let Some(context) = legend {
            render_legend(&mut canvas, context)?;
        }
        canvas.present();
        thread::sleep(Duration::from_millis(8));
    }

    Ok(())
}

fn normalize_selected(ui: &mut AppUi, profile: &RuntimeProfile) -> Result<(), String> {
    let selected = ui.selected_tracks();
    if selected.is_empty() {
        ui.set_normalization_status("SELECT TRACKS FIRST");
        return Ok(());
    }
    let plan = SessionPlan::from_audio_tracks(profile, &selected);
    if !plan.fits() {
        ui.set_normalization_status(format!("OVER CAPACITY BY {}S", -plan.remaining_seconds()));
        return Ok(());
    }
    let output_folder = Path::new(&profile.tracks_folder).join(&profile.normalized_folder);
    let settings = NormalizationSettings {
        method: profile.normalization,
        target_lufs: profile.target_lufs,
    };
    let mut cache_hits = 0;
    for track in &selected {
        if normalize_track(track, &output_folder, settings)?.cache_hit {
            cache_hits += 1;
        }
    }
    let method = match profile.normalization {
        NormalizationMethod::Peak => "PEAK",
        NormalizationMethod::Lufs => "LUFS",
    };
    ui.set_normalization_status(format!(
        "{method} READY {}/{} CACHE {cache_hits}",
        selected.len(),
        selected.len()
    ));
    Ok(())
}

fn build_normalization_review(
    ui: &mut AppUi,
    profile: &RuntimeProfile,
) -> Result<Option<NormalizationReview>, String> {
    let selected = ui.selected_tracks();
    if selected.is_empty() {
        ui.set_normalization_status("PREP BLOCKED: SELECT TRACKS");
        return Ok(None);
    }
    let plan = SessionPlan::from_audio_tracks(profile, &selected);
    if !plan.fits() {
        ui.set_normalization_status(format!(
            "PREP BLOCKED: OVER CAPACITY BY {}S",
            -plan.remaining_seconds()
        ));
        return Ok(None);
    }
    let output_folder = Path::new(&profile.tracks_folder).join(&profile.normalized_folder);
    let settings = NormalizationSettings {
        method: profile.normalization,
        target_lufs: profile.target_lufs,
    };
    let mut summaries = Vec::with_capacity(selected.len());
    for track in &selected {
        let normalized = normalize_track(track, &output_folder, settings)?;
        summaries.push((
            track.name.clone(),
            track.duration_seconds,
            normalized.cache_hit,
        ));
    }
    Ok(Some(NormalizationReview {
        tracks: summaries,
        total_seconds: plan.total_seconds(),
        capacity_seconds: plan.side_capacity_seconds,
        preview_status: String::new(),
        cursor: 0,
        viewport_start: 0,
        focus_changed_at: Instant::now(),
    }))
}

fn run_prep_session(
    canvas: &mut Canvas<Window>,
    events: &mut EventPump,
    ui: &mut AppUi,
    player: &mut AudioPlayer,
    profile: &RuntimeProfile,
) -> Result<(), String> {
    let selected = ui.selected_tracks();
    if selected.is_empty() {
        ui.set_normalization_status("PREP BLOCKED: SELECT TRACKS");
        return Ok(());
    }

    let plan = SessionPlan::from_audio_tracks(profile, &selected);
    if !plan.fits() {
        ui.set_normalization_status(format!(
            "PREP BLOCKED: OVER CAPACITY BY {}S",
            -plan.remaining_seconds()
        ));
        return Ok(());
    }

    let track_names: Vec<String> = selected.iter().map(|track| track.name.clone()).collect();
    let total_seconds = plan.total_seconds();
    let counter = TapeCounter::from_profile(profile)?;
    player.stop()?;
    let output_folder = Path::new(&profile.tracks_folder).join(&profile.normalized_folder);
    let settings = NormalizationSettings {
        method: profile.normalization,
        target_lufs: profile.target_lufs,
    };
    let mut normalized = Vec::with_capacity(selected.len());
    for track in &selected {
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
            NormalizationMethod::Peak => "PEAK",
            NormalizationMethod::Lufs => "LUFS",
        },
        profile.target_lufs,
    )?;
    if !wait_phase(
        canvas,
        events,
        ui,
        profile,
        Duration::from_secs(10),
        "PREP",
        &track_names,
        total_seconds,
        None,
        None,
        &counter,
    )? {
        return Ok(());
    }
    let session_start = Instant::now();
    if !wait_phase(
        canvas,
        events,
        ui,
        profile,
        Duration::from_secs(u64::from(profile.leader_gap_seconds)),
        "LEADER GAP",
        &track_names,
        total_seconds,
        Some(session_start),
        None,
        &counter,
    )? {
        return Ok(());
    }
    ui.set_normalization_status(format!(
        "TRACKLIST {}",
        tracklist_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("WRITTEN")
    ));

    for (index, track) in normalized.iter().enumerate() {
        if cancelled(events) {
            player.stop()?;
            ui.set_normalization_status("PREP CANCELLED");
            return Ok(());
        }
        player.play(&track.path)?;
        while player.current_path().is_some() {
            if cancelled(events) {
                player.stop()?;
                ui.set_normalization_status("PREP CANCELLED");
                return Ok(());
            }
            player.refresh()?;
            ui.set_previewing(player.current_path().is_some(), player.is_paused());
            ui.render_recording(
                canvas,
                profile,
                &format!("RECORDING {}/{}", index + 1, normalized.len()),
                &track_names,
                Some(index),
                session_start.elapsed().as_secs() as u32,
                total_seconds,
                counter.calculate(session_start.elapsed().as_secs_f64()),
                session_meters[index].level_at(player.position_seconds()),
                session_start.elapsed(),
                true,
            )?;
            canvas.present();
            thread::sleep(Duration::from_millis(50));
        }
        if index + 1 < normalized.len()
            && !wait_phase(
                canvas,
                events,
                ui,
                profile,
                Duration::from_secs(u64::from(profile.track_gap_seconds)),
                "TRACK GAP",
                &track_names,
                total_seconds,
                Some(session_start),
                None,
                &counter,
            )?
        {
            return Ok(());
        }
    }

    ui.set_previewing(false, false);
    ui.set_normalization_status("PREP COMPLETE");
    ui.render_recording(
        canvas,
        profile,
        "PREP COMPLETE",
        &track_names,
        None,
        total_seconds,
        total_seconds,
        counter.calculate(f64::from(total_seconds)),
        (0, 0),
        session_start.elapsed(),
        false,
    )?;
    canvas.present();
    Ok(())
}

fn wait_phase(
    canvas: &mut Canvas<Window>,
    events: &mut EventPump,
    ui: &mut AppUi,
    profile: &RuntimeProfile,
    duration: Duration,
    label: &str,
    track_names: &[String],
    total_seconds: u32,
    timeline_start: Option<Instant>,
    current_track: Option<usize>,
    counter: &TapeCounter,
) -> Result<bool, String> {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        if cancelled(events) {
            ui.set_normalization_status("PREP CANCELLED");
            return Ok(false);
        }
        let remaining = deadline.saturating_duration_since(Instant::now()).as_secs() + 1;
        let elapsed = timeline_start
            .map(|start| start.elapsed())
            .unwrap_or(Duration::ZERO);
        ui.render_recording(
            canvas,
            profile,
            &format!("{label} {remaining:02}S - Q CANCEL"),
            track_names,
            current_track,
            elapsed.as_secs() as u32,
            total_seconds,
            counter.calculate(elapsed.as_secs_f64()),
            (0, 0),
            elapsed,
            timeline_start.is_some(),
        )?;
        canvas.present();
        thread::sleep(Duration::from_millis(50));
    }
    Ok(true)
}

fn cancelled(events: &mut EventPump) -> bool {
    events.poll_iter().any(|event| {
        matches!(
            event,
            Event::Quit { .. }
                | Event::KeyDown {
                    keycode: Some(Keycode::Q | Keycode::Escape),
                    ..
                }
        )
    })
}

fn normalized_playback_path(
    track: &deckprep_rs::library::AudioTrack,
    profile: &RuntimeProfile,
) -> Result<std::path::PathBuf, String> {
    let output_folder = Path::new(&profile.tracks_folder).join(&profile.normalized_folder);
    let settings = NormalizationSettings {
        method: profile.normalization,
        target_lufs: profile.target_lufs,
    };
    Ok(normalize_track(track, output_folder, settings)?.path)
}
