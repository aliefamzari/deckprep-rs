use std::{
    path::Path,
    thread,
    time::{Duration, Instant},
};

use deckprep_rs::audio::{AudioPlayer, diagnose_audio};
use deckprep_rs::library::scan_tracks;
use deckprep_rs::meter::StereoMeter;
use deckprep_rs::normalize::{
    NormalizationEvent, NormalizationJob, NormalizationSettings, clean_stale_cache_files,
    normalize_track,
};
use deckprep_rs::queue::{list_queues, load_queue, save_queue_named};
use deckprep_rs::session::{PrepSession, SessionTickResult};
use deckprep_rs::tone::{TestTone, generate_test_tone};
use deckprep_rs::ui::{
    AppUi, LegendContext, NormalizationProgress, NormalizationReview, ProfileBrowser,
    ProfileEditor, QueueBrowser, QueueNameEditor, TrackFolderBrowser, render_legend,
};
use deckprep_rs::{config::RuntimeProfile, timing::SessionPlan};
use sdl2::{
    event::Event,
    keyboard::{Keycode, Mod},
};

fn main() -> Result<(), String> {
    let profile_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "profiles/desktop-dev.json".to_owned());
    if profile_path == "--audio-diagnostics" {
        let target_profile = std::env::args()
            .nth(2)
            .unwrap_or_else(|| "profiles/desktop-dev.json".to_owned());
        let (backend, device) = if let Ok(prof) = RuntimeProfile::load(&target_profile) {
            (prof.audio_backend, prof.audio_device)
        } else {
            ("sdl".to_owned(), "default".to_owned())
        };
        let diag = diagnose_audio(&backend, &device);
        println!("{}", diag.report());
        return Ok(());
    }
    if profile_path == "--create-profile" {
        let output = std::env::args().nth(2).ok_or_else(|| {
            "usage: cargo run -- --create-profile profiles/my-deck.json".to_owned()
        })?;
        return deckprep_rs::profile_wizard::run(output);
    }
    let mut profile = RuntimeProfile::load(&profile_path)?;
    let output_folder = Path::new(&profile.tracks_folder).join(&profile.normalized_folder);
    let _ = clean_stale_cache_files(&output_folder);
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
    let mut profile_browser: Option<ProfileBrowser> = None;
    let mut track_folder_browser: Option<TrackFolderBrowser> = None;
    let mut review: Option<NormalizationReview> = None;
    let mut preparation: Option<TrackPreparation> = None;
    let mut prep_session: Option<PrepSession> = None;
    let mut legend: Option<LegendContext> = None;
    let mut shortcut_consumed_key = false;

    'running: loop {
        let mut start_prep = false;
        let mut start_reviewed_prep = false;
        for event in events.poll_iter() {
            if let Some(task) = preparation.as_mut() {
                match event {
                    Event::Quit { .. } => break 'running,
                    Event::KeyDown {
                        keycode: Some(Keycode::Escape | Keycode::Q),
                        ..
                    } => {
                        task.job.cancel();
                        task.progress.cancelling = true;
                    }
                    _ => {}
                }
                continue;
            }
            if let Event::KeyDown {
                keycode,
                repeat: false,
                ..
            } = &event
            {
                shortcut_consumed_key = track_folder_browser.is_some()
                    || (editor.is_none() && queue_editor.is_none() && *keycode == Some(Keycode::W));
            }
            match event {
                Event::TextInput { ref text, .. } => {
                    if track_folder_browser.is_some() || shortcut_consumed_key {
                        continue;
                    }
                    if let Some(active_editor) = editor.as_mut() {
                        active_editor.handle_text_input(text);
                    } else if let Some(active_queue_editor) = queue_editor.as_mut() {
                        active_queue_editor.handle_text_input(text);
                    }
                }
                Event::Quit { .. } => break 'running,
                Event::KeyDown {
                    keycode: Some(Keycode::Escape),
                    ..
                } => {
                    if legend.is_some() {
                        legend = None;
                    } else if track_folder_browser.is_some() {
                        track_folder_browser = None;
                    } else if let Some(mut session) = prep_session.take() {
                        let was_complete = session.is_complete();
                        session.cancel(&mut player);
                        ui.set_previewing(false, false);
                        if !was_complete {
                            ui.set_normalization_status("PREP CANCELLED");
                        }
                    } else if editor.is_some() {
                        editor = None;
                    } else if queue_editor.is_some() {
                        queue_editor = None;
                    } else if queue_browser.is_some() {
                        queue_browser = None;
                    } else if profile_browser.is_some() {
                        profile_browser = None;
                    } else if review.is_some() {
                        leave_review(&mut review, &mut player, &mut preview_meter, &mut ui)?;
                    } else if ui.current_status().is_some() {
                        ui.clear_normalization_status();
                    } else {
                        break 'running;
                    }
                }
                Event::KeyDown {
                    keycode: Some(Keycode::Q),
                    ..
                } if queue_editor.is_none()
                    && editor
                        .as_ref()
                        .map_or(true, |active| !active.is_text_field()) =>
                {
                    if legend.is_some() {
                        legend = None;
                    } else if track_folder_browser.is_some() {
                        track_folder_browser = None;
                    } else if let Some(mut session) = prep_session.take() {
                        let was_complete = session.is_complete();
                        session.cancel(&mut player);
                        ui.set_previewing(false, false);
                        if !was_complete {
                            ui.set_normalization_status("PREP CANCELLED");
                        }
                    } else if editor.is_some() {
                        editor = None;
                    } else if queue_browser.is_some() {
                        queue_browser = None;
                    } else if profile_browser.is_some() {
                        profile_browser = None;
                    } else if review.is_some() {
                        leave_review(&mut review, &mut player, &mut preview_meter, &mut ui)?;
                    } else {
                        break 'running;
                    }
                }
                Event::KeyDown {
                    keycode: Some(key),
                    keymod,
                    repeat,
                    ..
                } if !repeat
                    || (legend.is_none()
                        && (track_folder_browser.is_some()
                            || profile_browser.is_some()
                            || queue_browser.is_some())
                        && is_browser_scroll_key(key)) =>
                {
                    if let Some(session) = prep_session.as_mut() {
                        if key == Keycode::Return && session.is_complete() {
                            prep_session = None;
                            ui.set_previewing(false, false);
                        }
                        continue;
                    }
                    if let Some(browser) = track_folder_browser.as_mut() {
                        if let Some(accept) = browser.handle_key(key) {
                            if accept {
                                let folder =
                                    browser.selected_folder().to_string_lossy().into_owned();
                                if let Some(active_editor) = editor.as_mut() {
                                    active_editor.profile.tracks_folder = folder;
                                } else {
                                    player.stop()?;
                                    preview_meter = None;
                                    profile.tracks_folder = folder;
                                    reload_library(&mut ui, &profile);
                                }
                            }
                            track_folder_browser = None;
                        }
                        continue;
                    }
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
                    if let Some(browser) = profile_browser.as_mut() {
                        if let Some(accept) = browser.handle_key(key) {
                            if accept {
                                if let Some(path) = browser.selected_path().map(Path::to_path_buf) {
                                    match RuntimeProfile::load(&path) {
                                        Ok(new_profile) => {
                                            let folder_changed =
                                                new_profile.tracks_folder != profile.tracks_folder;
                                            let audio_changed = new_profile.audio_backend
                                                != profile.audio_backend
                                                || new_profile.audio_device != profile.audio_device;
                                            profile = new_profile;
                                            if audio_changed {
                                                player = AudioPlayer::new(
                                                    &profile.audio_backend,
                                                    &profile.audio_device,
                                                );
                                            }
                                            if folder_changed {
                                                let (new_tracks, new_error) =
                                                    match scan_tracks(&profile.tracks_folder) {
                                                        Ok(tracks) => (tracks, None),
                                                        Err(error) => (Vec::new(), Some(error)),
                                                    };
                                                ui.reset_library(new_tracks, new_error);
                                            }
                                            ui.set_normalization_status(format!(
                                                "PROFILE LOADED {}",
                                                path.file_name()
                                                    .and_then(|name| name.to_str())
                                                    .unwrap_or("PROFILE.JSON")
                                            ));
                                        }
                                        Err(error) => ui.set_normalization_status(error),
                                    }
                                }
                            }
                            profile_browser = None;
                        }
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
                                        path.file_name()
                                            .and_then(|name| name.to_str())
                                            .unwrap_or("QUEUE.JSON")
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
                                leave_review(
                                    &mut review,
                                    &mut player,
                                    &mut preview_meter,
                                    &mut ui,
                                )?;
                                start_reviewed_prep = true;
                            }
                            Keycode::Num1 | Keycode::Kp1 => play_test_tone_preview(
                                TestTone::Hz400,
                                &mut player,
                                &mut preview_meter,
                                &mut ui,
                                &mut review,
                                &profile,
                            )?,
                            Keycode::Num2 | Keycode::Kp2 => play_test_tone_preview(
                                TestTone::Hz1000,
                                &mut player,
                                &mut preview_meter,
                                &mut ui,
                                &mut review,
                                &profile,
                            )?,
                            Keycode::Num3 | Keycode::Kp3 => play_test_tone_preview(
                                TestTone::Hz10000,
                                &mut player,
                                &mut preview_meter,
                                &mut ui,
                                &mut review,
                                &profile,
                            )?,
                            Keycode::Num4 | Keycode::Kp4 => play_test_tone_preview(
                                TestTone::Hz15000,
                                &mut player,
                                &mut preview_meter,
                                &mut ui,
                                &mut review,
                                &profile,
                            )?,
                            Keycode::Num5 | Keycode::Kp5 => play_test_tone_preview(
                                TestTone::Sweep,
                                &mut player,
                                &mut preview_meter,
                                &mut ui,
                                &mut review,
                                &profile,
                            )?,
                            Keycode::Escape | Keycode::Q => {
                                leave_review(
                                    &mut review,
                                    &mut player,
                                    &mut preview_meter,
                                    &mut ui,
                                )?;
                            }
                            _ => {}
                        }
                        continue;
                    }
                    if let Some(active_editor) = editor.as_mut() {
                        if key == Keycode::F2 {
                            match TrackFolderBrowser::new(&active_editor.profile.tracks_folder) {
                                Ok(browser) => track_folder_browser = Some(browser),
                                Err(error) => ui.set_normalization_status(error),
                            }
                        } else if key == Keycode::S
                            && keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD)
                        {
                            active_editor.profile.validate()?;
                            let folder_changed =
                                active_editor.profile.tracks_folder != profile.tracks_folder;
                            let path = active_editor.profile.save_snapshot("profiles")?;
                            profile = active_editor.profile.clone();
                            if folder_changed {
                                player.stop()?;
                                preview_meter = None;
                                reload_library(&mut ui, &profile);
                            }
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
                        Keycode::B if ui.current_track().is_none() => {
                            match TrackFolderBrowser::new(&profile.tracks_folder) {
                                Ok(browser) => track_folder_browser = Some(browser),
                                Err(error) => ui.set_normalization_status(error),
                            }
                        }
                        Keycode::W => editor = Some(ProfileEditor::new(&profile)),
                        Keycode::S if keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD) => {
                            if ui.selected_tracks().is_empty() {
                                ui.set_normalization_status("QUEUE EMPTY");
                            } else {
                                queue_editor = Some(QueueNameEditor::new("queues"));
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
                        Keycode::L => match RuntimeProfile::list_profiles("profiles") {
                            Ok(paths) if paths.is_empty() => {
                                ui.set_normalization_status("NO SAVED PROFILES")
                            }
                            Ok(paths) => profile_browser = Some(ProfileBrowser::new(paths)),
                            Err(error) => ui.set_normalization_status(error),
                        },
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
                        Keycode::Left => {
                            if let Some(track) = ui.current_track() {
                                player.seek_by(-10.0, track.duration_seconds)?;
                            }
                        }
                        Keycode::Right => {
                            if let Some(track) = ui.current_track() {
                                player.seek_by(10.0, track.duration_seconds)?;
                            }
                        }
                        Keycode::N => preparation = start_preparation(&mut ui, &profile, false),
                        Keycode::S => {
                            let path = profile.save_snapshot("profiles")?;
                            ui.set_normalization_status(format!(
                                "PROFILE SAVED {}",
                                path.file_name()
                                    .and_then(|name| name.to_str())
                                    .unwrap_or("PROFILE.JSON")
                            ));
                        }
                        Keycode::Num1 | Keycode::Kp1 => play_test_tone_preview(
                            TestTone::Hz400,
                            &mut player,
                            &mut preview_meter,
                            &mut ui,
                            &mut review,
                            &profile,
                        )?,
                        Keycode::Num2 | Keycode::Kp2 => play_test_tone_preview(
                            TestTone::Hz1000,
                            &mut player,
                            &mut preview_meter,
                            &mut ui,
                            &mut review,
                            &profile,
                        )?,
                        Keycode::Num3 | Keycode::Kp3 => play_test_tone_preview(
                            TestTone::Hz10000,
                            &mut player,
                            &mut preview_meter,
                            &mut ui,
                            &mut review,
                            &profile,
                        )?,
                        Keycode::Num4 | Keycode::Kp4 => play_test_tone_preview(
                            TestTone::Hz15000,
                            &mut player,
                            &mut preview_meter,
                            &mut ui,
                            &mut review,
                            &profile,
                        )?,
                        Keycode::Num5 | Keycode::Kp5 => play_test_tone_preview(
                            TestTone::Sweep,
                            &mut player,
                            &mut preview_meter,
                            &mut ui,
                            &mut review,
                            &profile,
                        )?,
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

        let mut preparation_finished = None;
        if let Some(task) = preparation.as_mut() {
            loop {
                match task.job.poll() {
                    Ok(NormalizationEvent::Processing { index, name }) => {
                        task.progress.current_index = index;
                        task.progress.current_track = name;
                    }
                    Ok(NormalizationEvent::Prepared(track)) => {
                        task.progress.completed += 1;
                        task.progress.cache_hits += usize::from(track.cache_hit);
                        task.summaries.push((
                            track.source.name,
                            track.source.duration_seconds,
                            track.cache_hit,
                        ));
                    }
                    Ok(NormalizationEvent::Finished(result)) => {
                        preparation_finished = Some(if task.progress.cancelling {
                            Err("PREPARATION CANCELLED".to_owned())
                        } else {
                            result
                        });
                        break;
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        preparation_finished = Some(Err("PREPARATION WORKER STOPPED".to_owned()));
                        break;
                    }
                }
            }
        }
        if let Some(result) = preparation_finished {
            if let Some(task) = preparation.take() {
                match result {
                    Ok(()) if task.review_after => {
                        review = Some(NormalizationReview {
                            tracks: task.summaries,
                            total_seconds: task.total_seconds,
                            capacity_seconds: task.capacity_seconds,
                            preview_status: String::new(),
                            cursor: 0,
                            viewport_start: 0,
                            focus_changed_at: Instant::now(),
                        });
                    }
                    Ok(()) => ui.set_normalization_status(format!(
                        "{:?} READY {}/{} CACHE {}",
                        task.progress.method,
                        task.progress.completed,
                        task.progress.total,
                        task.progress.cache_hits
                    )),
                    Err(error) => ui.set_normalization_status(error),
                }
            }
        }

        if start_reviewed_prep {
            leave_review(&mut review, &mut player, &mut preview_meter, &mut ui)?;
            match PrepSession::start(&ui.selected_tracks(), &profile) {
                Ok(session) => {
                    ui.set_normalization_status(format!(
                        "TRACKLIST {}",
                        session
                            .tracklist_path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("WRITTEN")
                    ));
                    prep_session = Some(session);
                }
                Err(error) => ui.set_normalization_status(error),
            }
        } else if start_prep {
            if review.is_none() {
                preparation = start_preparation(&mut ui, &profile, true);
            } else {
                leave_review(&mut review, &mut player, &mut preview_meter, &mut ui)?;
                match PrepSession::start(&ui.selected_tracks(), &profile) {
                    Ok(session) => {
                        ui.set_normalization_status(format!(
                            "TRACKLIST {}",
                            session
                                .tracklist_path
                                .file_name()
                                .and_then(|name| name.to_str())
                                .unwrap_or("WRITTEN")
                        ));
                        prep_session = Some(session);
                    }
                    Err(error) => ui.set_normalization_status(error),
                }
            }
        }

        if let Some(session) = prep_session.as_mut() {
            match session.tick(&mut player, &profile) {
                Ok(SessionTickResult::Continuing) => {
                    ui.set_previewing(player.current_path().is_some(), player.is_paused());
                }
                Ok(SessionTickResult::Complete) => {
                    ui.set_previewing(false, false);
                    ui.set_normalization_status("PREP COMPLETE");
                }
                Err(error) => {
                    session.cancel(&mut player);
                    ui.set_normalization_status(format!("PREP ERROR: {error}"));
                    prep_session = None;
                }
            }
        } else {
            player.refresh()?;
            if player.current_path().is_none() {
                preview_meter = None;
            }
            ui.set_previewing(player.current_path().is_some(), player.is_paused());
        }

        let meter_levels = preview_meter
            .as_ref()
            .map(|meter| meter.level_at(player.position_seconds()));
        if let Some(task) = preparation.as_ref() {
            task.progress.render(&mut canvas)?;
        } else if let Some(session) = prep_session.as_ref() {
            session.render(&mut canvas, &ui, &profile, &player)?;
        } else if let Some(browser) = track_folder_browser.as_ref() {
            browser.render(&mut canvas)?;
        } else if let Some(browser) = profile_browser.as_ref() {
            browser.render(&mut canvas)?;
        } else if let Some(browser) = queue_browser.as_ref() {
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

fn is_browser_scroll_key(key: Keycode) -> bool {
    matches!(
        key,
        Keycode::Up
            | Keycode::Down
            | Keycode::J
            | Keycode::K
            | Keycode::PageUp
            | Keycode::PageDown
            | Keycode::Home
            | Keycode::End
    )
}

fn leave_review(
    review: &mut Option<NormalizationReview>,
    player: &mut AudioPlayer,
    preview_meter: &mut Option<StereoMeter>,
    ui: &mut AppUi,
) -> Result<(), String> {
    player.stop()?;
    *preview_meter = None;
    ui.set_previewing(false, false);
    *review = None;
    Ok(())
}

fn reload_library(ui: &mut AppUi, profile: &RuntimeProfile) {
    let (tracks, error) = match scan_tracks(&profile.tracks_folder) {
        Ok(tracks) => (tracks, None),
        Err(error) => (Vec::new(), Some(error)),
    };
    ui.reset_library(tracks, error);
}

struct TrackPreparation {
    job: NormalizationJob,
    progress: NormalizationProgress,
    summaries: Vec<(String, u32, bool)>,
    total_seconds: u32,
    capacity_seconds: u32,
    review_after: bool,
}

fn start_preparation(
    ui: &mut AppUi,
    profile: &RuntimeProfile,
    review_after: bool,
) -> Option<TrackPreparation> {
    let selected = ui.selected_tracks();
    if selected.is_empty() {
        ui.set_normalization_status("PREP BLOCKED: SELECT TRACKS");
        return None;
    }
    let plan = SessionPlan::from_audio_tracks(profile, &selected);
    if !plan.fits() {
        ui.set_normalization_status(format!(
            "PREP BLOCKED: OVER CAPACITY BY {}S",
            -plan.remaining_seconds()
        ));
        return None;
    }
    let output_folder = Path::new(&profile.tracks_folder).join(&profile.normalized_folder);
    let settings = NormalizationSettings {
        method: profile.normalization,
        target_lufs: profile.target_lufs,
    };
    let progress = NormalizationProgress {
        total: selected.len(),
        completed: 0,
        current_index: 0,
        current_track: selected[0].name.clone(),
        cache_hits: 0,
        method: profile.normalization,
        cancelling: false,
        started_at: Instant::now(),
    };
    let job = match NormalizationJob::start(selected, output_folder, settings) {
        Ok(job) => job,
        Err(error) => {
            ui.set_normalization_status(error);
            return None;
        }
    };
    Some(TrackPreparation {
        job,
        progress,
        summaries: Vec::new(),
        total_seconds: plan.total_seconds(),
        capacity_seconds: plan.side_capacity_seconds,
        review_after,
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

fn play_test_tone_preview(
    tone: TestTone,
    player: &mut AudioPlayer,
    preview_meter: &mut Option<StereoMeter>,
    ui: &mut AppUi,
    review: &mut Option<NormalizationReview>,
    profile: &RuntimeProfile,
) -> Result<(), String> {
    let output_folder = Path::new(&profile.tracks_folder)
        .join(&profile.normalized_folder)
        .join("tones");
    let tone_path = generate_test_tone(tone, &output_folder)?;
    if player.current_path() == Some(tone_path.as_path()) {
        player.toggle(&tone_path)?;
    } else {
        player.play(&tone_path)?;
    }
    *preview_meter = Some(StereoMeter::analyze(&tone_path)?);
    let label = format!("TONE {}", tone.label());
    ui.set_normalization_status(format!("TEST {}", label));
    if let Some(active_review) = review.as_mut() {
        active_review.preview_status = label;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_repeat_only_accepts_list_navigation() {
        for key in [
            Keycode::Up,
            Keycode::Down,
            Keycode::J,
            Keycode::K,
            Keycode::PageUp,
            Keycode::PageDown,
            Keycode::Home,
            Keycode::End,
        ] {
            assert!(is_browser_scroll_key(key));
        }
        for key in [
            Keycode::Return,
            Keycode::Space,
            Keycode::Left,
            Keycode::Right,
            Keycode::Backspace,
            Keycode::Escape,
            Keycode::Q,
            Keycode::W,
            Keycode::F2,
        ] {
            assert!(!is_browser_scroll_key(key));
        }
    }

    #[test]
    fn leaving_review_stops_preview_and_clears_view_and_meter() {
        let root = std::env::temp_dir().join(format!(
            "deckprep-review-exit-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let tone = generate_test_tone(TestTone::Hz400, &root).unwrap();
        let mut meter = Some(StereoMeter::analyze(&tone).unwrap());
        let mut player = AudioPlayer::new("sdl", "default");
        player.play(&root.join("missing-preview.wav")).unwrap();
        assert!(player.is_playing());
        let mut ui = AppUi::new(Vec::new(), None);
        ui.set_previewing(true, false);
        let mut review = Some(NormalizationReview {
            tracks: Vec::new(),
            total_seconds: 0,
            capacity_seconds: 1800,
            preview_status: "PLAY".to_owned(),
            cursor: 0,
            viewport_start: 0,
            focus_changed_at: Instant::now(),
        });

        leave_review(&mut review, &mut player, &mut meter, &mut ui).unwrap();

        assert!(review.is_none());
        assert!(meter.is_none());
        assert!(!ui.previewing());
        assert!(!player.is_playing());
        assert!(!player.is_paused());
        assert_eq!(player.current_path(), None);
        assert_eq!(player.position_seconds(), 0.0);
        leave_review(&mut review, &mut player, &mut meter, &mut ui).unwrap();
        std::fs::remove_dir_all(root).unwrap();
    }
}
